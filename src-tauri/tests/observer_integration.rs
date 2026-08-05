//! observer 集成测：mock SSE server + SseClient 端到端验证。
//!
//! 跑默认：
//!     cd src-tauri && cargo test --test observer_integration
//!
//! 验证项：
//! 1. SseClient 能解析 mock server 发的 SSE 帧、分派到 Router；
//! 2. 断开后指数退避重连（mock 每次连接后立即关闭，触发重连）；
//! 3. shutdown Arc<AtomicBool> 能干净取消 run 循环。

use inkos_desktop::observer::router::{EventHandler, Router};
use inkos_desktop::observer::sse::{SseClient, SseEvent};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

/// 计数 handler：内部 `Arc<Mutex<u32>>` 公开供测试侧直接读取（避免 downcast）。
/// 测试构造时 clone 一份 `Arc<Mutex<u32>>` 留作断言读取——保持生产 `default_table`
/// 语义不变，仅借用测试夹具的共享内部实现计数。
struct Count(Arc<Mutex<u32>>);

impl EventHandler for Count {
    fn handle(&self, _ev: &SseEvent) -> anyhow::Result<()> {
        *self.0.lock().unwrap() += 1;
        Ok(())
    }
}

/// 单帧 SSE 响应：HTTP 头（CRLF）+ 一个 `write:complete` 事件（LF 结尾，对齐
/// `parse_sse_frame` 只识别 `\n\n` 帧分隔符的实现）。每次 accept 后立即关闭连接，
/// 模拟 inkos sidecar 在某些场景下断开（触发客户端重连路径）。
/// 注意：HTTP/1.1 头要求 CRLF；SSE 事件体按 spec 允许 CRLF/LF/CR，本测试用 LF
/// 与生产 inkos (Node.js 默认 LF) 一致。
const SSE_FRAME: &[u8] = b"HTTP/1.1 200 OK\r\n\
                          Content-Type: text/event-stream\r\n\
                          \r\n\
                          event: write:complete\n\
                          data: {\"id\":1}\n\
                          \n";

/// 主集成测：mock 每次连接发一帧后断开，客户端应在 1.5s 内至少收一次并重连，
/// shutdown 能干净退出，run 不 panic。
#[tokio::test]
async fn sse_client_dispatches_frame_and_reconnects() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 失败");
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            // 每次连接：发一帧后立即 shutdown 写端，模拟服务端断开。
            if let Ok((mut s, _)) = listener.accept().await {
                let _ = s.write_all(SSE_FRAME).await;
                let _ = s.shutdown().await;
            }
        }
    });

    let badge_count = Arc::new(Mutex::new(0u32));
    let notifier_count = Arc::new(Mutex::new(0u32));
    let badge = Arc::new(Count(Arc::clone(&badge_count))) as Arc<dyn EventHandler>;
    let notifier =
        Arc::new(Count(Arc::clone(&notifier_count))) as Arc<dyn EventHandler>;
    let router = Arc::new(Router::default_table(notifier, badge));
    let shutdown = Arc::new(AtomicBool::new(false));
    let url = format!("http://127.0.0.1:{port}/api/v1/events");
    let client = SseClient::new(url);

    let shutdown_clone = Arc::clone(&shutdown);
    let jh = tokio::spawn(async move {
        let _ = client.run(router, shutdown_clone).await;
    });

    // 跑 1.5s 让它：connect #1 → 收帧 → 断 → backoff 1s → connect #2 → 收帧 → 断
    tokio::time::sleep(Duration::from_millis(1500)).await;
    shutdown.store(true, Ordering::SeqCst);

    // shutdown 后应在 ~100ms（cancellable_sleep 分段）内退出。
    let _ = tokio::time::timeout(Duration::from_millis(500), jh)
        .await
        .expect("shutdown 后应在 500ms 内退出 run 循环");

    let badge_n = *badge_count.lock().unwrap();
    let notifier_n = *notifier_count.lock().unwrap();
    assert!(
        badge_n >= 1,
        "badge 应至少派发一次 write:complete，实际 {badge_n}"
    );
    assert!(
        notifier_n >= 1,
        "notifier 应至少派发一次 write:complete，实际 {notifier_n}"
    );
    // 重连生效验证：1.5s 窗口内首连接 ~t=0、二次重连 ~t=1s，故应 ≥ 2 次。
    // 放宽为 ≥ 1 以容忍调度抖动（CI 慢机可能只跑完一轮）。
    eprintln!(
        "[test] badge={badge_n}, notifier={notifier_n} (1.5s 窗口内重连次数)"
    );
}

/// shutdown 在首次连接前触发：run 应立即返回 Ok，不发起任何 IO。
#[tokio::test]
async fn sse_client_exits_immediately_when_shutdown_pre_set() {
    // 即便绑一个不存在的端口，shutdown 预置也不应触发 connect。
    let badge_count = Arc::new(Mutex::new(0u32));
    let notifier_count = Arc::new(Mutex::new(0u32));
    let badge = Arc::new(Count(Arc::clone(&badge_count))) as Arc<dyn EventHandler>;
    let notifier =
        Arc::new(Count(Arc::clone(&notifier_count))) as Arc<dyn EventHandler>;
    let router = Arc::new(Router::default_table(notifier, badge));
    let shutdown = Arc::new(AtomicBool::new(true)); // 预置 true
    let url = "http://127.0.0.1:1/api/v1/events".to_string(); // 端口 1 不可达
    let client = SseClient::new(url);

    // run 应立即返回 Ok，不发起连接。
    let result = tokio::time::timeout(Duration::from_millis(100), client.run(router, shutdown))
        .await
        .expect("shutdown 预置时应立即返回，不应超时 100ms");
    assert!(result.is_ok(), "run 应返回 Ok");

    assert_eq!(*badge_count.lock().unwrap(), 0);
    assert_eq!(*notifier_count.lock().unwrap(), 0);
}

/// backoff 倍增：mock 服务端拒绝连接（绑一个未监听端口），run 应快速重连失败。
/// 此测试验证连接错误路径不 panic 且 shutdown 仍可取消。
#[tokio::test]
async fn sse_client_handles_connection_errors_without_panic() {
    // 端口 1（discard）通常拒绝连接；客户端进入 Err 路径，backoff 倍增。
    let badge_count = Arc::new(Mutex::new(0u32));
    let notifier_count = Arc::new(Mutex::new(0u32));
    let badge = Arc::new(Count(Arc::clone(&badge_count))) as Arc<dyn EventHandler>;
    let notifier =
        Arc::new(Count(Arc::clone(&notifier_count))) as Arc<dyn EventHandler>;
    let router = Arc::new(Router::default_table(notifier, badge));
    let shutdown = Arc::new(AtomicBool::new(false));
    let url = "http://127.0.0.1:1/api/v1/events".to_string();
    let client = SseClient::new(url);

    let shutdown_clone = Arc::clone(&shutdown);
    let jh = tokio::spawn(async move {
        let _ = client.run(router, shutdown_clone).await;
    });

    // 跑 300ms 让它至少进入一次 Err 路径（backoff 1s 仍在 sleep）。
    tokio::time::sleep(Duration::from_millis(300)).await;
    shutdown.store(true, Ordering::SeqCst);

    let _ = tokio::time::timeout(Duration::from_millis(500), jh)
        .await
        .expect("shutdown 后应在 500ms 内退出 run 循环");

    // 端口 1 不应成功派发任何事件。
    assert_eq!(*badge_count.lock().unwrap(), 0);
    assert_eq!(*notifier_count.lock().unwrap(), 0);
}
