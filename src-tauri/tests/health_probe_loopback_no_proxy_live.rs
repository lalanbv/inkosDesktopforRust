//! 1581 号红路活体：`health_probe` 回环旁路（1572 号最初修复面，八站中
//! src-tauri 三站此前无死代理行为级活体——当时只有真环境红转绿实证）。死代
//! 理 env 下回环探活必须直连成功；若 `.no_proxy()` 回潮被删，探活请求经死
//! 代理连接即败 → deadline 内全败返回 false → 红。独立测试文件 = 独立进程，
//! env 注入不外泄（同 engine-rs 1573 起活体形态）。

use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

#[tokio::test]
async fn health_probe_loopback_bypasses_dead_proxy() {
    // ── 死代理 env 注入（须在任何 client 构建前）──────────────────
    // 127.0.0.1:1 本机必拒绝连接；reqwest 构建时 from_env 读 HTTPS_PROXY，
    // 未旁路请求连接即失败（1572 号「启动门恒超时」形态的恒定化）。
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:1");
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");

    // ── 回环 mock（同 supervisor_integration 手写 200 形态）─────────
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind 失败");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            if let Ok((mut sock, _)) = listener.accept().await {
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        }
    });

    assert!(
        inkos_desktop::supervisor::health_probe(port, Duration::from_secs(3), "/").await,
        "死代理 env 下回环探活必须直连成功（.no_proxy() 旁路生效，1572 号面）"
    );
}
