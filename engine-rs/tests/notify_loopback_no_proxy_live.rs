//! 1578 号红路活体：notify webhook 回环旁路（1573 号四站接入中此前无行为级
//! 活体的站——`notify_client(url)` 谓词面）。标记代理法两臂闭环：
//! 回环 webhook「通且计数=0」证明直连（本地 ntfy/Apprise 主场景），非回环
//! webhook「败且计数+1」证明真走代理；顺带锁定 `X-InkOS-Signature` HMAC 头
//! 真传递（mock 校验格式）。独立进程防 env 污染，同 1573/1575/1577 形态。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use inkos_engine::notify::dispatcher::{send_webhook, WebhookPayload};

async fn spawn_marker_proxy() -> (std::net::SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits_for_task = hits.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let hits = hits_for_task.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut total = 0usize;
                // 1.100 while_let_loop：loop+let-else+零长守卫 → while let
                // （Ok(0)=对端关闭照旧 break，语义不变）。
                while let Ok(n) = sock.read(&mut buf[total..]).await {
                    if n == 0 {
                        break;
                    }
                    total += n;
                    if buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                hits.fetch_add(1, Ordering::SeqCst);
                let _ = sock
                    .write_all(b"HTTP/1.1 599 Via-Proxy\r\ncontent-length: 0\r\n\r\n")
                    .await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, hits)
}

fn payload(event: &str) -> WebhookPayload {
    WebhookPayload {
        event: event.to_string(),
        book_id: "b1".to_string(),
        chapter_number: Some(1),
        timestamp: "2026-11-19T00:00:00Z".to_string(),
        data: None,
    }
}

#[tokio::test]
async fn notify_webhook_loopback_bypasses_and_remote_goes_through_proxy() {
    // ── 标记代理 env 注入（须在任何 client 构建前）────────────────
    let (proxy_addr, hits) = spawn_marker_proxy().await;
    let proxy = format!("http://{proxy_addr}");
    std::env::set_var("HTTPS_PROXY", &proxy);
    std::env::set_var("HTTP_PROXY", &proxy);

    // ── 回环 webhook mock：校验 HMAC 签名头格式 + 200 ─────────────
    let app = axum::Router::new().route(
        "/webhook",
        axum::routing::post(|headers: axum::http::HeaderMap| async move {
            match headers.get("X-InkOS-Signature") {
                Some(value) if value.to_str().unwrap_or("").starts_with("sha256=") => {
                    axum::http::StatusCode::OK
                }
                _ => axum::http::StatusCode::UNAUTHORIZED,
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let loopback_url = format!("http://{mock_addr}/webhook");

    // ── 回环臂：直连成功 + HMAC 头真传递 + 计数 0（旁路证明）────────
    send_webhook(
        &loopback_url,
        Some("test-key-1578"),
        &[],
        &payload("chapter.done"),
    )
    .await
    .expect("回环 webhook 必须旁路直连成功（HMAC 头校验通过）");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "回环 webhook 不得经过代理（计数必须为 0）"
    );

    // ── 非回环臂：TEST-NET-3 走标记代理 → 599 → Err + 计数 +1 ──────
    let remote_url = "http://192.198.152.10:8080/webhook";
    let outcome = send_webhook(remote_url, None, &[], &payload("chapter.done")).await;
    assert!(
        outcome.is_err(),
        "非回环 webhook 经标记代理收到 599 必须报错"
    );
    let message = outcome.unwrap_err();
    assert!(
        message.contains("599"),
        "错误须来自标记代理响应（真走代理），实测 {message}"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "非回环 webhook 必须真实经过代理（计数 +1）"
    );
}
