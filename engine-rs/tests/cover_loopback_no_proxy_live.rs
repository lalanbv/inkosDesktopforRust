//! 1578 号红路活体：封面下载**代理谓词面**（1574 号修正——`download_generated_
//! cover_image` 的 client 谓词必须钉实际下载目标 `url` 而非 provider 基址）。
//! 既有跨源单测只覆盖 origin 守卫链路，两种谓词形态下回环 CDN 下载同样成功、
//! 不可区分；本活体用标记代理法（env 代理回 599 并计数，独立进程防 env 污染
//! ——单测进程内与同键 EXTRA_ORIGINS 既有测试并行互踩，本轮实证）做可区分
//! 断言。同 1573/1575/1577 活体形态。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use inkos_engine::llm::cover::download_generated_cover_image;

/// 标记代理：任何请求读到头结束即计数 +1 并回 599（「真过代理」的确定性
/// 证明，无外部网络依赖）。
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
                loop {
                    let Ok(n) = sock.read(&mut buf[total..]).await else {
                        break;
                    };
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
async fn cover_download_proxy_predicate_targets_actual_url() {
    // ── 标记代理 env 注入（须在任何 client 构建前）────────────────
    let (proxy_addr, hits) = spawn_marker_proxy().await;
    let proxy = format!("http://{proxy_addr}");
    std::env::set_var("HTTPS_PROXY", &proxy);
    std::env::set_var("HTTP_PROXY", &proxy);

    // ── 同源回环 CDN 对照：直连成功（旁路臂），计数不变 ─────────────
    let cdn_app = axum::Router::new().route(
        "/cdn.png",
        axum::routing::get(|| async {
            (
                [(axum::http::header::CONTENT_TYPE, "image/png")],
                b"\x89PNGfake".to_vec(),
            )
        }),
    );
    let cdn_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cdn_addr = cdn_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(cdn_listener, cdn_app).await.unwrap();
    });
    let provider_base = format!("http://{cdn_addr}/v1");
    let same_origin_url = format!("http://{cdn_addr}/cdn.png");
    let ok = download_generated_cover_image(&same_origin_url, "k", &provider_base)
        .await
        .expect("同源回环 CDN 必须旁路直连成功");
    assert_eq!(ok.extension, "png");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "回环下载不得经过代理（计数必须为 0）"
    );

    // ── 跨源非回环 CDN（EXTRA_ORIGINS 放行）：必须真走代理 ──────────
    // TEST-NET-3（RFC 5737）合法 IPv4 文本、非回环、路由必败——经标记代理
    // 时代理侧即回 599，不触真实网络。10s 护栏：走标记代理毫秒级返回；
    // 若谓词回潮成 provider 基址判定（回环→旁路）则直连 TEST-NET-3 挂到
    // 护栏超时 → 红。
    std::env::set_var(
        inkos_engine::llm::cover_origin::ENV_COVER_DOWNLOAD_EXTRA_ORIGINS,
        "http://192.198.152.10:8080",
    );
    let remote_url = "http://192.198.152.10:8080/cdn.png";
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        download_generated_cover_image(remote_url, "k", &provider_base),
    )
    .await;
    std::env::remove_var(inkos_engine::llm::cover_origin::ENV_COVER_DOWNLOAD_EXTRA_ORIGINS);
    std::env::remove_var("HTTPS_PROXY");
    std::env::remove_var("HTTP_PROXY");
    let message = match outcome {
        Ok(Err(message)) => message,
        Ok(Ok(image)) => panic!("非回环 CDN 在标记代理下必须失败，实测成功 {image:?}"),
        Err(_) => panic!(
            "非回环 CDN 走标记代理应毫秒级 599 返回；10s 超时=谓词可能回潮成 provider 基址判定而直连 TEST-NET-3"
        ),
    };
    assert!(
        message.contains("599"),
        "错误须来自标记代理响应（真走代理），实测 {message}"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "非回环下载必须真实经过代理（计数 +1）——1574 号谓词面"
    );
}
