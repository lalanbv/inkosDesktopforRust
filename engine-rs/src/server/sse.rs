//! SSE 广播面（write:start / write:complete / write:error / ping）。
//!
//! 移植自 `packages/studio/src/api/server.ts` 的 subscribers/broadcast +
//! `/api/v1/events` SSE 端点（hono streamSSE → axum Sse）。
//!
//! 契约（前端零改动）：
//! - 事件负载 JSON 序列化为 `data:` 字段
//! - 连接建立即发 `ping`；30s keep-alive ping
//! - `?sessionId=` 存在时补发 `task:snapshot`（重启对账后的快照）

use std::convert::Infallible;
use std::sync::Arc;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::Stream;
use tokio::sync::{broadcast, watch};

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
    /// 优雅停机信号（172 号 W-B2）：独立 watch 通道而非广播特殊事件——
    /// broadcast 的 `RecvError::Closed` 依赖全部 sender drop（hub 被 Clone
    /// 持有永不发生）；且"流终结"是生命周期信号不是业务事件，语义分层。
    shutdown: watch::Sender<bool>,
}

impl Default for BroadcastHub {
    fn default() -> Self {
        let (sender, _) = broadcast::channel(256);
        let (shutdown, _) = watch::channel(false);
        BroadcastHub { sender, shutdown }
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

    /// 通知全部 SSE 流终结（服务优雅停机路径调用）。
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    pub(crate) fn subscribe_shutdown(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }
}

/// SSE 路由状态：广播总线 + 引擎项目根（快照对账的落盘面）。
///
/// 前端 EventSource 只传 `?sessionId=`（use-sse.ts / chat action.ts），从不传
/// projectRoot——快照恢复的生产链路依赖装配点注入的引擎项目根；query 里的
/// `projectRoot`（历史形态/多项目直连）仍可覆盖。
#[derive(Clone)]
pub struct EventsState {
    pub hub: Arc<BroadcastHub>,
    pub project_root: std::path::PathBuf,
}

/// SSE 流：订阅广播 + 连接即 ping + sessionId 快照 + 30s keep-alive。
pub async fn events_handler(
    axum::extract::State(events): axum::extract::State<EventsState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = events.hub.subscribe();
    // 优雅停机分支（172 号 W-B2）：hub.shutdown() 后流正常 EOF——
    // 连接随之关闭，axum graceful drain 才能完成（无限流否则永不排空）。
    let mut shutdown_rx = events.hub.subscribe_shutdown();
    let session_id = params.get("sessionId").cloned();
    let project_root = params
        .get("projectRoot")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| events.project_root.clone());

    let stream = async_stream::stream! {
        // 连接建立即 ping（TS await stream.writeSSE({ event: "ping", data: "" })）。
        yield Ok(Event::default().event("ping").data(""));

        // task:snapshot（sessionId 存在时）。用对账读取（TS loadReconciledTaskSnapshot
        // 同款，174 号 W-C5）：running 快照若本进程无存活任务（引擎重启遗留），
        // 先改写中断终态再下发——否则重连会复活一张永远运行中的死任务卡。
        if let Some(session_id) = &session_id {
            if let Some(snapshot) = crate::server::agent_production::load_reconciled_task_snapshot(
                &project_root,
                session_id,
            )
            .await
            {
                if let Ok(data) = serde_json::to_string(&snapshot) {
                    yield Ok(Event::default().event("task:snapshot").data(data));
                }
            }
        }

        // select 分支只做控制流（yield 一律在 stream! 直接支持的
        // match 里，避免嵌套宏内 yield 的展开顺序问题）。
        enum Step {
            Event(SsePayload),
            Ping,
            Closed,
            Shutdown,
        }
        loop {
            let step = tokio::select! {
                biased;
                _ = shutdown_rx.changed() => Step::Shutdown,
                result = receiver.recv() => match result {
                    Ok(payload) => Step::Event(payload),
                    Err(broadcast::error::RecvError::Lagged(_)) => Step::Ping,
                    Err(broadcast::error::RecvError::Closed) => Step::Closed,
                },
            };
            match step {
                Step::Shutdown => {
                    // 停机信号到达：先排空管道内已广播事件（含 bin 停机
                    // 路径先发的 engine:shutdown——biased 下它可能尚未被
                    // recv，不排空会丢帧），再终结流（EOF）。
                    while let Ok(payload) = receiver.try_recv() {
                        yield Ok(Event::default().event(payload.event).data(payload.data));
                    }
                    break;
                }
                Step::Event(payload) => {
                    yield Ok(Event::default().event(payload.event).data(payload.data));
                }
                Step::Ping => {
                    // 慢消费者丢帧——发 ping 保活并继续。
                    yield Ok(Event::default().event("ping").data(""));
                }
                Step::Closed => {
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
            .with_state(EventsState { hub: hub.clone(), project_root: std::env::temp_dir() });

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

    #[tokio::test]
    async fn graceful_shutdown_notifies_then_ends_stream() {
        let hub = Arc::new(BroadcastHub::new());
        let app = axum::Router::new()
            .route("/api/v1/events", axum::routing::get(events_handler))
            .with_state(EventsState { hub: hub.clone(), project_root: std::env::temp_dir() });

        let response = app
            .oneshot(axum::http::Request::builder().uri("/api/v1/events").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();

        // 停机序列（bin 停机路径同序）：先广播业务事件，再关流。
        hub.broadcast("engine:shutdown", &serde_json::json!({ "reason": "signal" }));
        hub.shutdown();

        let mut stream = futures_util::StreamExt::boxed(response.into_body().into_data_stream());
        let mut text = String::new();
        loop {
            match futures_util::StreamExt::next(&mut stream).await {
                Some(Ok(chunk)) => text.push_str(&String::from_utf8_lossy(&chunk)),
                // 流在 shutdown 后必须正常终结（EOF），graceful drain 才能完成。
                Some(Err(e)) => panic!("stream errored before EOF: {e}"),
                None => break,
            }
        }
        assert!(text.contains("event: ping"));
        assert!(text.contains("event: engine:shutdown"));
        assert!(text.contains("\"reason\":\"signal\""));
    }

    #[tokio::test]
    async fn shutdown_without_pending_events_also_ends_stream() {
        let hub = Arc::new(BroadcastHub::new());
        let app = axum::Router::new()
            .route("/api/v1/events", axum::routing::get(events_handler))
            .with_state(EventsState { hub: hub.clone(), project_root: std::env::temp_dir() });

        let response = app
            .oneshot(axum::http::Request::builder().uri("/api/v1/events").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();

        // 订阅发生在 handler 建流时——先取流再 shutdown，保证 changed() 可见。
        let mut stream = futures_util::StreamExt::boxed(response.into_body().into_data_stream());
        // 等到首帧（ping）确认流已启动。
        let first = futures_util::StreamExt::next(&mut stream).await;
        assert!(first.is_some());
        hub.shutdown();
        // 有限时间内必须 EOF（而非无限 keep-alive）。
        let drained = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            futures_util::StreamExt::collect::<Vec<_>>(stream),
        )
        .await
        .expect("stream should end after shutdown");
        assert!(drained.iter().all(|r| r.is_ok()));
    }

    /// 174 号 W-C5：重连补发的快照走对账读取——重启遗留的死 running 快照
    /// 先改写中断终态再下发（不对账则会复活永远转圈的任务卡）。
    #[tokio::test]
    async fn reconnect_snapshot_is_reconciled_before_send() {
        let dir = tempfile::tempdir().unwrap();
        let dead = serde_json::json!({
            "version": 1,
            "sessionId": "sess-dead",
            "requestedIntent": "write_next",
            "updatedAt": 1_000.0,
            "execution": {
                "id": "write-next-deadbook",
                "tool": "pipeline",
                "label": "撰写下一章（deadbook）",
                "status": "running",
                "startedAt": 900.0
            }
        });
        // 旧进程遗留的 running 快照（直接落原始 JSON，模拟上次进程来不及收尾）。
        let tasks_dir = dir.path().join(".inkos/tasks");
        tokio::fs::create_dir_all(&tasks_dir).await.unwrap();
        tokio::fs::write(tasks_dir.join("sess-dead.json"), format!("{dead}\n"))
            .await
            .unwrap();

        let hub = Arc::new(BroadcastHub::new());
        let app = axum::Router::new()
            .route("/api/v1/events", axum::routing::get(events_handler))
            .with_state(EventsState { hub: hub.clone(), project_root: dir.path().to_path_buf() });
        // 生产形态：EventSource 只带 sessionId——项目根由装配点注入。
        let response = app
            .oneshot(axum::http::Request::builder().uri("/api/v1/events?sessionId=sess-dead").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();

        let mut stream = futures_util::StreamExt::boxed(response.into_body().into_data_stream());
        let mut text = String::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !text.contains("task:snapshot") && std::time::Instant::now() < deadline {
            let next = futures_util::StreamExt::next(&mut stream);
            match tokio::time::timeout(std::time::Duration::from_secs(1), next).await {
                Ok(Some(Ok(chunk))) => text.push_str(&String::from_utf8_lossy(&chunk)),
                Ok(Some(Err(_))) | Ok(None) => break,
                Err(_) => continue,
            }
        }
        assert!(text.contains("event: task:snapshot"), "未收到快照事件: {text}");
        assert!(text.contains("\"status\":\"error\""), "应是对账后的中断终态: {text}");
        assert!(text.contains("任务已中断"));
        // 改写已持久化。
        let persisted = crate::server::task_store::load_studio_task_snapshot(dir.path(), "sess-dead")
            .await
            .unwrap();
        assert_eq!(
            persisted.execution.status,
            crate::server::task_store::StudioTaskExecutionStatus::Error
        );
    }
}
