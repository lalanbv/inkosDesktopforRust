//! SSE 广播面（write:start / write:complete / write:error / ping）。
//!
//! 移植自 `packages/studio/src/api/server.ts` 的 subscribers/broadcast +
//! `/api/v1/events` SSE 端点（hono streamSSE → axum Sse）。
//!
//! 契约（前端零改动）：
//! - 事件负载 JSON 序列化为 `data:` 字段
//! - 连接建立即发 `ping`；30s keep-alive ping
//! - `?sessionId=` 存在时补发 `task:snapshot`（task-store 快照）

use std::convert::Infallible;
use std::sync::Arc;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::Stream;
use tokio::sync::broadcast;

/// 事件通道承载单元（事件名 + JSON 负载）。
#[derive(Debug, Clone)]
pub struct SsePayload {
    pub event: String,
    pub data: String,
}

/// 进程内广播总线（TS subscribers Set 的等价物）。
#[derive(Clone)]
pub struct BroadcastHub {
    sender: broadcast::Sender<SsePayload>,
}

impl Default for BroadcastHub {
    fn default() -> Self {
        let (sender, _) = broadcast::channel(256);
        BroadcastHub { sender }
    }
}

impl BroadcastHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// 广播事件（无订阅者时静默——TS 同语义）。
    pub fn broadcast(&self, event: &str, data: &serde_json::Value) {
        let _ = self.sender.send(SsePayload {
            event: event.to_string(),
            data: serde_json::to_string(data).unwrap_or_default(),
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SsePayload> {
        self.sender.subscribe()
    }
}

/// SSE 流：订阅广播 + 连接即 ping + sessionId 快照 + 30s keep-alive。
pub async fn events_handler(
    axum::extract::State(hub): axum::extract::State<Arc<BroadcastHub>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = hub.subscribe();
    let session_id = params.get("sessionId").cloned();
    let project_root = params.get("projectRoot").cloned();

    let stream = async_stream::stream! {
        // 连接建立即 ping（TS await stream.writeSSE({ event: "ping", data: "" })）。
        yield Ok(Event::default().event("ping").data(""));

        // task:snapshot（sessionId 存在时）。
        if let (Some(session_id), Some(project_root)) = (&session_id, &project_root) {
            if let Some(snapshot) = crate::server::task_store::load_studio_task_snapshot(
                std::path::Path::new(project_root),
                session_id,
            )
            .await
            {
                if let Ok(data) = serde_json::to_string(&snapshot) {
                    yield Ok(Event::default().event("task:snapshot").data(data));
                }
            }
        }

        loop {
            match receiver.recv().await {
                Ok(payload) => {
                    yield Ok(Event::default().event(payload.event).data(payload.data));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // 慢消费者丢帧——发 ping 保活并继续。
                    yield Ok(Event::default().event("ping").data(""));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(30)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn events_stream_carries_broadcast_events() {
        let hub = Arc::new(BroadcastHub::new());
        let app = axum::Router::new()
            .route("/api/v1/events", axum::routing::get(events_handler))
            .with_state(hub.clone());

        // 先建立连接（流消费前广播一条）。
        let response = app
            .clone()
            .oneshot(axum::http::Request::builder().uri("/api/v1/events").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        hub.broadcast(
            "write:start",
            &serde_json::json!({ "bookId": "b1" }),
        );

        // SSE 是无限流——有界帧消费（不做 to_bytes 的 EOF 等待）。
        let mut stream = futures_util::StreamExt::boxed(response.into_body().into_data_stream());
        let mut text = String::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while text.matches("event:").count() < 4 && std::time::Instant::now() < deadline {
            let next = futures_util::StreamExt::next(&mut stream);
            match tokio::time::timeout(std::time::Duration::from_secs(1), next).await {
                Ok(Some(Ok(chunk))) => text.push_str(&String::from_utf8_lossy(&chunk)),
                Ok(Some(Err(_))) | Ok(None) => break,
                Err(_) => continue,
            }
        }
        // axum SSE 帧格式 "event: <name>"（冒号后有空格）。
        assert!(text.contains("event: ping"));
        assert!(text.contains("event: write:start"));
        assert!(text.contains("{\"bookId\":\"b1\"}"));
    }

    #[tokio::test]
    async fn broadcast_without_subscribers_is_silent() {
        let hub = BroadcastHub::new();
        hub.broadcast("write:error", &serde_json::json!({ "error": "x" }));
        // 不 panic 即通过。
    }
}
