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

use crate::server::sse::BroadcastHub;
use crate::server::task_store::{
    save_studio_task_snapshot, StudioTaskExecution, StudioTaskExecutionStatus,
    StudioTaskSnapshot,
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
                if let Some(session_id) = &body.session_id {
                    persist_task_snapshot(
                        &task_runtime,
                        session_id,
                        &task_book_id,
                        StudioTaskExecutionStatus::Completed,
                        None,
                    )
                    .await;
                }
            }
            Err(error) => {
                let message = error.to_string();
                task_runtime.hub.broadcast(
                    "write:error",
                    &serde_json::json!({ "bookId": task_book_id, "error": message }),
                );
                if let Some(session_id) = &body.session_id {
                    persist_task_snapshot(
                        &task_runtime,
                        session_id,
                        &task_book_id,
                        StudioTaskExecutionStatus::Error,
                        Some(&message),
                    )
                    .await;
                }
            }
        }
    });

    (
        StatusCode::OK,
        Json(WriteNextResponse { status: "writing", book_id }),
    )
}

async fn persist_task_snapshot(
    runtime: &WriteNextRuntime,
    session_id: &str,
    book_id: &str,
    status: StudioTaskExecutionStatus,
    error: Option<&str>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as f64;
    let snapshot = StudioTaskSnapshot {
        version: 1,
        session_id: session_id.to_string(),
        source_request_id: None,
        requested_intent: "write_next".to_string(),
        execution: StudioTaskExecution {
            id: format!("write-next-{book_id}"),
            tool: "pipeline".to_string(),
            agent: None,
            label: format!("撰写下一章（{book_id}）"),
            status,
            args: None,
            result: None,
            details: None,
            error: error.map(String::from),
            stages: None,
            logs: None,
            started_at: now,
            completed_at: Some(now),
        },
        updated_at: now,
    };
    let _ = save_studio_task_snapshot(&runtime.project_root, &snapshot).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::sse::BroadcastHub;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn write_next_returns_writing_immediately_and_pushes_sse() {
        let hub = Arc::new(BroadcastHub::new());
        let state = Arc::new(StateManager::new("/tmp/inkos-test-nonexistent"));
        let runtime = WriteNextRuntime {
            hub: hub.clone(),
            state,
            // runner 直接失败——spawn 内推 write:error。
            runner: Arc::new(|_state, _book, _wc, _temp| {
                Box::pin(async { Err("book config unavailable".to_string()) })
            }),
            project_root: "/tmp/inkos-test-nonexistent".into(),
        };
        let app = axum::Router::new()
            .route("/api/v1/books/:id/write-next", axum::routing::post(write_next))
            .with_state(runtime);

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
}
