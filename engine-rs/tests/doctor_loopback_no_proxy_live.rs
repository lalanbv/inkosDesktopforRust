//! 1576 号红路活体：DOCTOR `/models` 快探腿（`doctor_models_probe`）谓词行为
//! 级两臂闭环——1575 号谓词化时本腿只有编译保护（判定面靠 loopback_proxy
//! 17 案表测），本轮补齐运行时验证。死代理 env 变体=标记代理法：代理回 599
//! 并对每笔连接计数，回环臂「通且计数=0」证明直连（未过代理），远端臂「败
//! 且计数+1」证明真走了代理。独立测试文件 = 独立进程，env 注入不外泄（同
//! 1573/1575 形态）。
//!
//! 撤修锚：谓词撤成默认 builder（1575 前的漏网形态，reqwest 默认读 env 代理）
//! → 回环臂也过标记代理被 599 → 红；复原 → 绿。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use inkos_engine::server::ops_routes::doctor_models_probe;

/// 标记代理：任何请求读到头结束即计数 +1 并回 599（比转发失败更确定的
/// 「真过代理」证明，无外部网络依赖——1573 号活体同法）。
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

#[tokio::test]
async fn doctor_models_probe_loopback_bypasses_and_remote_goes_through_proxy() {
    // ── 标记代理 env 注入（须在任何 client 构建前）────────────────
    let (proxy_addr, hits) = spawn_marker_proxy().await;
    let proxy = format!("http://{proxy_addr}");
    std::env::set_var("HTTPS_PROXY", &proxy);
    std::env::set_var("HTTP_PROXY", &proxy);

    // ── 本地回环 mock /models：带 key 用例校验 Authorization 真传递 ──
    let app = axum::Router::new().route(
        "/models",
        axum::routing::get(|headers: axum::http::HeaderMap| async move {
            match headers.get(axum::http::header::AUTHORIZATION) {
                None => axum::Json(serde_json::json!({ "data": [] })),
                Some(value) => {
                    if value == "Bearer test-key-1576" {
                        axum::Json(serde_json::json!({ "data": [] }))
                    } else {
                        axum::Json(serde_json::json!({ "error": "bad key" }))
                    }
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let loopback_base = format!("http://{mock_addr}");

    // ── 回环臂：直连成功且零代理计数（旁路证明）────────────────────
    assert!(
        doctor_models_probe(&loopback_base, "").await,
        "回环端点必须在标记代理 env 下直连成功（旁路生效）"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "回环探测不得经过代理（计数必须为 0）"
    );
    // api_key 分支：Bearer 头真传递（mock 校验不匹配即失败）。
    assert!(
        doctor_models_probe(&loopback_base, "test-key-1576").await,
        "带 api_key 的回环探测必须成功（Bearer 头校验通过）"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);

    // ── 远端臂：非回环目标必经标记代理（599 → 探测失败，计数 +1）────
    // 192.198.152.10 = TEST-NET-3 保留地址（RFC 5737）：合法 IPv4 文本、
    // 非回环、路由必败——经标记代理时代理侧即回 599，不触真实网络。
    let remote_base = "http://192.198.152.10:8080";
    assert!(
        !doctor_models_probe(remote_base, "").await,
        "远端端点经标记代理收到 599 必须判探测失败"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "远端探测必须真实经过代理（计数必须 +1）"
    );
}
