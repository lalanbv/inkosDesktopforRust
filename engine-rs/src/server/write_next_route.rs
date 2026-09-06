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

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::interaction::agent_loop::AbortHandle;
use crate::server::agent_production::active_confirmed_tasks;
use crate::server::sse::BroadcastHub;
use crate::server::task_store::{
    save_studio_task_snapshot, StudioTaskExecution, StudioTaskExecutionStatus, StudioTaskSnapshot,
};
use crate::state::manager::StateManager;

/// 执行器闭包：构造九路端口聚合并在其生命周期内跑完 write-next。
///
/// 第五参为中止句柄（175 号）：实现方须注入 `WriteNextConfig.abort`——
/// 管线在阶段边界轮询 `check_aborted`，stop 端点置位后任务在安全点停止。
/// 第六参为规划输入（183 号）：非空时作为 `external_context` 替换自动 plan。
pub type WriteNextRunner = Arc<
    dyn Fn(
            Arc<StateManager>,
            String,
            Option<u32>,
            Option<f64>,
            AbortHandle,
            Option<String>,
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
    /// 规划输入（183 号）：非空时替换 write-next 的自动 plan——时间线节拍
    /// 「按此节拍写下一章」的引擎侧出口。TS 回退端同名键收下但忽略。
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteNextResponse {
    pub status: &'static str,
    #[serde(rename = "bookId")]
    pub book_id: String,
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
        // 检查点（174 号 W-C5）+ 可停止（175 号）：sessionId 给定时任务全程
        // 留痕且真实可中止——启动先注册确认任务表（句柄被管线真实消费，
        // stop 端点与重启对账同一来源）再落 Running 快照（对账窗口语义与
        // TS 确认任务一致），终态快照落盘后才注销。
        let checkpoint = body
            .session_id
            .as_deref()
            .map(|session_id| WriteNextCheckpoint::start(session_id, &task_book_id, &body));
        let abort: AbortHandle = Arc::new(std::sync::Mutex::new(false));
        if let Some(entry) = &checkpoint {
            active_confirmed_tasks()
                .lock()
                .unwrap()
                .insert(entry.execution_id.clone(), abort.clone());
            entry.persist_running(&task_runtime).await;
        }

        let result = (task_runtime.runner)(
            task_runtime.state.clone(),
            task_book_id.clone(),
            body.word_count,
            body.temperature,
            abort,
            body.context,
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
                    active_confirmed_tasks()
                        .lock()
                        .unwrap()
                        .remove(&entry.execution_id);
                }
            }
            Err(error) => {
                let message = error.to_string();
                // 188 号：用户主动停止（175 号 abort）不是「失败」——快照与
                // 广播改用中性文案，UI 据此以「已停止」而非红色失败呈现。
                let user_message = if message.contains("Operation aborted") {
                    "写作已按您的要求停止。".to_string()
                } else {
                    message.clone()
                };
                task_runtime.hub.broadcast(
                    "write:error",
                    &serde_json::json!({ "bookId": task_book_id, "error": user_message }),
                );
                if let Some(entry) = &checkpoint {
                    entry.persist_finished(
                        &task_runtime,
                        StudioTaskExecutionStatus::Error,
                        Some(&user_message),
                    )
                    .await;
                    active_confirmed_tasks()
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
            Arc::new(|_state, _book, _wc, _temp, _abort, _ctx| {
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
            Arc::new(move |_state, _book, _wc, _temp, _abort, _ctx| {
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
        assert!(active_confirmed_tasks()
            .lock()
            .unwrap()
            .contains_key("write-next-bck"));

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
        assert!(!active_confirmed_tasks()
            .lock()
            .unwrap()
            .contains_key("write-next-bck"));
    }

    /// 175 号：stop 端点路径真实停止 write-next——find_running_task_controller
    /// 找到注册句柄 → 置位（abort_session 同款）→ runner 在检查点退出 →
    /// error 终态 + 注册表释放。
    #[tokio::test]
    async fn stop_handle_registered_and_setting_it_stops_runner() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, hub) = runtime_with(
            dir.path(),
            Arc::new(|_state, _book, _wc, _temp, abort, _ctx| {
                Box::pin(async move {
                    // 模拟管线阶段边界轮询：置位即停，10s 兜底防挂死。
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    loop {
                        if *abort.lock().unwrap() {
                            return Err::<crate::pipeline::write_next::ChapterPipelineResult, _>(
                                "Operation aborted: the user requested to stop this task."
                                    .to_string(),
                            );
                        }
                        assert!(std::time::Instant::now() < deadline, "abort 未到达");
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                })
            }),
        );
        let app = app_for(runtime.clone());

        let mut subscriber = hub.subscribe();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/stp/write-next")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"sessionId":"sess-stp"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Running 快照落盘后，stop 端点同款查找必须命中。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            assert!(std::time::Instant::now() < deadline, "Running 快照未落盘");
            if load_studio_task_snapshot(&runtime.project_root, "sess-stp")
                .await
                .is_some()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let handle = crate::server::agent_production::find_running_task_controller(
            &runtime.project_root,
            "sess-stp",
        )
        .await
        .expect("stop 端点应找到 write-next 句柄");

        // abort_session 的置位动作 → runner 在检查点退出。
        *handle.lock().unwrap() = true;
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), subscriber.recv()).await {
                Ok(Ok(payload)) if payload.event == "write:error" => {
                    // 188 号：用户停止的广播文案已友好化（不再含 "Operation aborted" 前缀包装）。
                    assert!(payload.data.contains("写作已按您的要求停止"));
                    break;
                }
                Ok(Ok(_)) => continue,
                other => panic!("write:error 事件未到达: {other:?}"),
            }
        }
        let finished = loop {
            if let Some(snapshot) =
                load_studio_task_snapshot(&runtime.project_root, "sess-stp").await
            {
                if snapshot.execution.status == StudioTaskExecutionStatus::Error {
                    break snapshot;
                }
            }
            assert!(
                std::time::Instant::now() < deadline + std::time::Duration::from_secs(5),
                "终态快照未落盘"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        assert!(finished.execution.error.as_deref().unwrap().contains("已按您的要求停止"));
        assert!(!active_confirmed_tasks()
            .lock()
            .unwrap()
            .contains_key("write-next-stp"));
    }

    /// 不传 sessionId（UI 现状）零磁盘副作用——检查点完全休眠。
    #[tokio::test]
    async fn no_session_id_leaves_no_task_files() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, hub) = runtime_with(
            dir.path(),
            Arc::new(|_state, _book, _wc, _temp, _abort, _ctx| {
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
        assert!(!active_confirmed_tasks()
            .lock()
            .unwrap()
            .contains_key("write-next-b0"));
    }
}
