//! `/api/v1/books/:id/write-next` 路由（strangler 首个核心业务端点）。
//!
//! 移植 Node 侧契约（server.ts L3512-3530）：fire-and-forget——
//! 立即返回 `{status:"writing", bookId}`，进度/完成/错误经 SSE 推送：
//! - `write:start {bookId}`
//! - `write:complete {bookId, chapterNumber, status, title, wordCount}`
//! - `write:error {bookId, error}`
//!
//! write-next 需要真实 LLM 端口（writer/planner/composer/reviser/auditor/
//! normalizer/analyzer/state-validator/settler 九路）——生产实现经
//! [`WriteNextRuntime`] 注入（LLM 域路由器之上）；测试注入全 mock。
//! 任务快照（39 号 task-store）按会话落盘供 SSE 重连恢复。

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::server::sse::BroadcastHub;
use crate::server::task_store::{
    save_studio_task_snapshot, StudioTaskExecution, StudioTaskExecutionStatus, StudioTaskSnapshot,
};
use crate::state::manager::StateManager;

/// 执行器闭包：构造九路端口聚合并在其生命周期内跑完 write-next。
pub type WriteNextRunner = Arc<
    dyn Fn(
            Arc<StateManager>,
            String,
            Option<u32>,
            Option<f64>,
        ) -> futures_util::future::BoxFuture<
            'static,
            Result<crate::pipeline::write_next::ChapterPipelineResult, String>,
        > + Send
        + Sync,
>;

/// 生产/测试共用的运行时句柄。
#[derive(Clone)]
pub struct WriteNextRuntime {
    pub hub: Arc<BroadcastHub>,
    pub state: Arc<StateManager>,
    /// write-next 执行器（LLM 端口聚合的构造与消费都在闭包内）。
    pub runner: WriteNextRunner,
    pub project_root: std::path::PathBuf,
}

#[derive(Debug, Default, Deserialize)]
pub struct WriteNextBody {
    #[serde(rename = "wordCount", default)]
    pub word_count: Option<u32>,
    #[serde(rename = "temperature", default)]
    pub temperature: Option<f64>,
    #[serde(rename = "sessionId", default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteNextResponse {
    pub status: &'static str,
    #[serde(rename = "bookId")]
    pub book_id: String,
}

/// 本进程活跃 write-next 任务（执行 id 集合，174 号 W-C5）。
///
/// 对账活跃性的第二个来源：确认式生产任务用 [`active_confirmed_tasks`]，
/// write-next 是无控制器消费的 fire-and-forget——若复用确认任务表，abort
/// 端点会找到句柄并宣称 `aborted:true` 而管线实际无法取消（假停止）。独立
/// 集合让「停止」保持诚实的「无任务可停」，同时重启对账能识别存活的
/// running 快照不误杀。
pub fn active_write_next_tasks() -> &'static Mutex<HashSet<String>> {
    static REGISTRY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

/// POST /api/v1/books/:id/write-next
pub async fn write_next(
    State(runtime): State<WriteNextRuntime>,
    Path(book_id): Path<String>,
    body: Option<Json<WriteNextBody>>,
) -> impl IntoResponse {
    let Json(body) = body.unwrap_or_default();

    runtime.hub.broadcast("write:start", &serde_json::json!({ "bookId": book_id }));

    // fire-and-forget：完成/错误经 SSE 推送。
    let task_runtime = runtime.clone();
    let task_book_id = book_id.clone();
    tokio::spawn(async move {
        // 检查点（174 号 W-C5）：sessionId 给定时任务全程留痕——启动先注册
        // 活跃表再落 Running 快照（对账窗口语义与 TS 确认任务一致：注册先于
        // 首次持久化），终态快照落盘后才注销。
        let checkpoint = body
            .session_id
            .as_deref()
            .map(|session_id| WriteNextCheckpoint::start(session_id, &task_book_id, &body));
        if let Some(entry) = &checkpoint {
            active_write_next_tasks().lock().unwrap().insert(entry.execution_id.clone());
            entry.persist_running(&task_runtime).await;
        }

        let result = (task_runtime.runner)(
            task_runtime.state.clone(),
            task_book_id.clone(),
            body.word_count,
            body.temperature,
        )
        .await
        .map_err(|message| {
            crate::pipeline::write_next::WriteNextError::Write(message)
        });
        match result {
            Ok(outcome) => {
                task_runtime.hub.broadcast(
                    "write:complete",
                    &serde_json::json!({
                        "bookId": task_book_id,
                        "chapterNumber": outcome.chapter_number,
                        "status": outcome.status,
                        "title": outcome.title,
                        "wordCount": outcome.word_count,
                    }),
                );
                if let Some(entry) = &checkpoint {
                    entry.persist_finished(&task_runtime, StudioTaskExecutionStatus::Completed, None)
                        .await;
                    active_write_next_tasks()
                        .lock()
                        .unwrap()
                        .remove(&entry.execution_id);
                }
            }
            Err(error) => {
                let message = error.to_string();
                task_runtime.hub.broadcast(
                    "write:error",
                    &serde_json::json!({ "bookId": task_book_id, "error": message }),
                );
                if let Some(entry) = &checkpoint {
                    entry.persist_finished(
                        &task_runtime,
                        StudioTaskExecutionStatus::Error,
                        Some(&message),
                    )
                    .await;
                    active_write_next_tasks()
                        .lock()
                        .unwrap()
                        .remove(&entry.execution_id);
                }
            }
        }
    });

    (
        StatusCode::OK,
        Json(WriteNextResponse { status: "writing", book_id }),
    )
}

/// write-next 会话级检查点：一次任务的 Running/终态快照共用同一执行 id 与
/// startedAt（TS 确认任务「同一 exec 对象演化」的最小等价形态）。
struct WriteNextCheckpoint {
    session_id: String,
    execution_id: String,
    book_id: String,
    started_at: f64,
    args: Option<serde_json::Map<String, serde_json::Value>>,
}

impl WriteNextCheckpoint {
    fn start(session_id: &str, book_id: &str, body: &WriteNextBody) -> Self {
        let mut args = serde_json::Map::new();
        if let Some(word_count) = body.word_count {
            args.insert("wordCount".into(), serde_json::json!(word_count));
        }
        if let Some(temperature) = body.temperature {
            args.insert("temperature".into(), serde_json::json!(temperature));
        }
        Self {
            session_id: session_id.to_string(),
            execution_id: format!("write-next-{book_id}"),
            book_id: book_id.to_string(),
            started_at: now_ms(),
            args: (!args.is_empty()).then_some(args),
        }
    }

    fn snapshot(&self, status: StudioTaskExecutionStatus, error: Option<&str>) -> StudioTaskSnapshot {
        let now = now_ms();
        StudioTaskSnapshot {
            version: 1,
            session_id: self.session_id.clone(),
            source_request_id: None,
            requested_intent: "write_next".to_string(),
            execution: StudioTaskExecution {
                id: self.execution_id.clone(),
                tool: "pipeline".to_string(),
                agent: None,
                label: format!("撰写下一章（{}）", self.book_id),
                status,
                args: self.args.clone(),
                result: None,
                details: None,
                error: error.map(String::from),
                stages: None,
                logs: None,
                started_at: self.started_at,
                completed_at: (status != StudioTaskExecutionStatus::Running).then_some(now),
            },
            updated_at: now,
        }
    }

    async fn persist_running(&self, runtime: &WriteNextRuntime) {
        let _ = save_studio_task_snapshot(
            &runtime.project_root,
            &self.snapshot(StudioTaskExecutionStatus::Running, None),
        )
        .await;
    }

    async fn persist_finished(
        &self,
        runtime: &WriteNextRuntime,
        status: StudioTaskExecutionStatus,
        error: Option<&str>,
    ) {
        let _ = save_studio_task_snapshot(&runtime.project_root, &self.snapshot(status, error)).await;
    }
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::task_store::load_studio_task_snapshot;
    use crate::server::sse::BroadcastHub;
    use tower::util::ServiceExt;

    fn runtime_with(root: &std::path::Path, runner: WriteNextRunner) -> (WriteNextRuntime, Arc<BroadcastHub>) {
        let hub = Arc::new(BroadcastHub::new());
        let runtime = WriteNextRuntime {
            hub: hub.clone(),
            state: Arc::new(StateManager::new(root.display().to_string().as_str())),
            runner,
            project_root: root.to_path_buf(),
        };
        (runtime, hub)
    }

    fn app_for(runtime: WriteNextRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/books/:id/write-next", axum::routing::post(write_next))
            .with_state(runtime)
    }

    #[tokio::test]
    async fn write_next_returns_writing_immediately_and_pushes_sse() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, hub) = runtime_with(
            dir.path(),
            // runner 直接失败——spawn 内推 write:error。
            Arc::new(|_state, _book, _wc, _temp| {
                Box::pin(async { Err("book config unavailable".to_string()) })
            }),
        );
        let app = app_for(runtime);

        let mut subscriber = hub.subscribe();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/write-next")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        #[derive(Deserialize)]
        struct TestDto {
            status: String,
            #[serde(rename = "bookId")]
            book_id: String,
        }
        let parsed: TestDto = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.status, "writing");
        assert_eq!(parsed.book_id, "b1");

        // SSE：start 即刻；error 随 spawn 到达。
        let first = subscriber.recv().await.unwrap();
        assert_eq!(first.event, "write:start");
        let second = tokio::time::timeout(std::time::Duration::from_secs(5), subscriber.recv())
            .await
            .expect("error 事件应到达")
            .unwrap();
        assert_eq!(second.event, "write:error");
        assert!(second.data.contains("b1"));
    }

    /// 174 号 W-C5：sessionId 给定时任务全程留痕——注册活跃表 → Running
    /// 快照（completedAt 缺席、startedAt 同源）→ 终态快照 → 注销。
    #[tokio::test]
    async fn session_checkpoint_lifecycle_running_then_error() {
        let dir = tempfile::tempdir().unwrap();
        let gate = Arc::new(tokio::sync::Notify::new());
        let gate_for_runner = gate.clone();
        let (runtime, hub) = runtime_with(
            dir.path(),
            Arc::new(move |_state, _book, _wc, _temp| {
                let gate = gate_for_runner.clone();
                Box::pin(async move {
                    gate.notified().await;
                    Err::<crate::pipeline::write_next::ChapterPipelineResult, _>("boom".to_string())
                })
            }),
        );
        let app = app_for(runtime.clone());

        let mut subscriber = hub.subscribe();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/bck/write-next")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"sessionId":"sess-ck","wordCount":800}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Running 快照先行落盘（轮询等待 spawn 执行到 runner 门前）。
        let running = {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                assert!(std::time::Instant::now() < deadline, "Running 快照未落盘");
                if let Some(snapshot) =
                    load_studio_task_snapshot(&runtime.project_root, "sess-ck").await
                {
                    break snapshot;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        };
        assert_eq!(running.execution.id, "write-next-bck");
        assert_eq!(running.execution.status, StudioTaskExecutionStatus::Running);
        assert!(running.execution.completed_at.is_none());
        assert_eq!(running.requested_intent, "write_next");
        assert_eq!(
            running.execution.args.as_ref().unwrap().get("wordCount"),
            Some(&serde_json::json!(800))
        );
        assert!(active_write_next_tasks()
            .lock()
            .unwrap()
            .contains("write-next-bck"));

        // 放行 runner → 失败 → 终态快照 + 注销。
        gate.notify_one();
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), subscriber.recv()).await {
                Ok(Ok(payload)) if payload.event == "write:error" => break,
                Ok(Ok(_)) => continue,
                other => panic!("write:error 事件未到达: {other:?}"),
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let finished = loop {
            if let Some(snapshot) = load_studio_task_snapshot(&runtime.project_root, "sess-ck").await {
                if snapshot.execution.status == StudioTaskExecutionStatus::Error {
                    break snapshot;
                }
            }
            assert!(std::time::Instant::now() < deadline, "终态快照未落盘");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        assert_eq!(finished.execution.started_at, running.execution.started_at);
        assert!(finished.execution.completed_at.is_some());
        // WriteNextError Display 包一层「write chapter failed: …」——断言消息本体。
        assert!(finished.execution.error.as_deref().unwrap().contains("boom"));
        assert!(!active_write_next_tasks()
            .lock()
            .unwrap()
            .contains("write-next-bck"));
    }

    /// 不传 sessionId（UI 现状）零磁盘副作用——检查点完全休眠。
    #[tokio::test]
    async fn no_session_id_leaves_no_task_files() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, hub) = runtime_with(
            dir.path(),
            Arc::new(|_state, _book, _wc, _temp| {
                Box::pin(async { Err("nope".to_string()) })
            }),
        );
        let app = app_for(runtime);
        let mut subscriber = hub.subscribe();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b0/write-next")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), subscriber.recv()).await {
                Ok(Ok(payload)) if payload.event == "write:error" => break,
                Ok(Ok(_)) => continue,
                other => panic!("write:error 事件未到达: {other:?}"),
            }
        }
        assert!(!dir.path().join(".inkos/tasks").exists());
        // 共享静态表——只断言本用例的 id 未入表（并行用例各自持有自己的 id）。
        assert!(!active_write_next_tasks()
            .lock()
            .unwrap()
            .contains("write-next-b0"));
    }
}
