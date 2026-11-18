//! 1575 号红路活体：死代理 env 下，回环嵌入端点（`embed_batch`）与自建本机
//! 检测端点（`detection_http_client`）必须直连成功（旁路生效），默认
//! `reqwest::Client`（外网语义）必须真走代理而失败——控制红证明 env 代理在
//! 本进程真实在效，防「测试恒绿」假防线。独立测试文件 = 独立进程，env 注入
//! 不外泄（同 1573 号形态）。
//!
//! 1575 号普查定性：reqwest 默认读 env/系统代理 → `embed_batch`（Ollama
//! 本地嵌入主场景）与检测链（`api_url` 可自建于本机）是回环劫持族漏网面；
//! DOCTOR `/models` 无条件 `no_proxy` 为反向过度旁路（同轮谓词化，由
//! loopback_proxy 17 案表测覆盖判定面）。

use inkos_engine::models::project::{DetectionConfig, DetectionProvider};
use inkos_engine::pipeline::detection_runner::detection_http_client;
use inkos_engine::utils::semantic_retrieval::{embed_batch, EmbeddingConfig, EmbeddingProvider};

#[tokio::test]
async fn loopback_embed_and_detection_endpoints_bypass_dead_proxy() {
    // ── 死代理 env 注入（须在任何 client 构建前）──────────────────
    // 127.0.0.1:1 本机必拒绝连接；未旁路请求连接即失败（毫秒级红）。
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:1");
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");

    // ── 本地回环 mock：POST /embeddings + POST /detect ───────────
    let app = axum::Router::new()
        .route(
            "/embeddings",
            axum::routing::post(|| async {
                axum::Json(serde_json::json!({
                    "data": [{ "embedding": [0.1, 0.2, 0.3] }]
                }))
            }),
        )
        .route(
            "/detect",
            axum::routing::post(|| async {
                axum::Json(serde_json::json!({
                    "documents": [{ "completely_generated_prob": 0.87 }]
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let detect_url = format!("http://{addr}/detect");

    // ── 控制红：默认 reqwest::Client（外网语义）必走死代理 ─────────
    // 证明 env 代理在本进程真实在效（旁路不是「环境恰好无代理」假绿）。
    let plain = reqwest::Client::new();
    let control = plain
        .post(format!("http://{addr}/embeddings"))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
    assert!(
        control.is_err(),
        "控制红：默认 client 必须被死代理劫持而失败，实测 {control:?}"
    );

    // ── 主绿 A：embed_batch 回环旁路（本地 Ollama 嵌入主场景）───────
    let config = EmbeddingConfig {
        provider: EmbeddingProvider::OpenAICompatible,
        base_url: format!("http://{addr}"),
        model: "mock-embed".to_string(),
        api_key_env: None,
        timeout_ms: Some(5_000),
    };
    let vectors = embed_batch(&config, &["正文".to_string()], None)
        .await
        .expect("回环嵌入端点必须在死代理 env 下直连成功（旁路生效）");
    assert_eq!(vectors, vec![vec![0.1, 0.2, 0.3]]);

    // ── 主绿 B：detection_http_client 回环旁路（自建本机检测端点）────
    let client = detection_http_client(&detect_url);
    let detection_config = DetectionConfig {
        provider: DetectionProvider::Gptzero,
        api_url: detect_url.clone(),
        api_key_env: "INKOS_TEST_LLM_KEY".to_string(),
        threshold: 0.9,
        enabled: true,
        auto_rewrite: false,
        max_retries: 1,
        protected_names: Vec::new(),
    };
    let outcome = detect_chapter_via(&client, &detection_config)
        .await
        .expect("自建本机检测端点必须在死代理 env 下直连成功（旁路生效）");
    assert!(
        outcome.passed,
        "score 0.87 <= threshold 0.9 必须判过（连通性是本测主证，判过逻辑为辅证）"
    );
}

/// `detect_chapter` 薄封装：`inkos_engine::pipeline::detection_runner` 未在
/// crate 根再导出时走完整路径（保持集成测试只依赖 pub 面）。
async fn detect_chapter_via(
    client: &reqwest::Client,
    config: &DetectionConfig,
) -> Result<inkos_engine::pipeline::detection_runner::DetectChapterOutcome, String> {
    // 占位内容不含专名泄露（protected_names 为空必无泄露），检测判过与否
    // 只由 score 驱动——红路语义聚焦连通性。
    inkos_engine::pipeline::detection_runner::detect_chapter(client, config, "正文内容", 1).await
}

/// 1578 号逐功能补测：embed_batch 的 **Ollama 分支**行为级锁定（1575 号活体
/// 只覆盖了 OpenAICompatible 分支）。三面：URL 拼法 `/api/embed`（mock 路由
/// 即证）、Ollama 响应形态 `{"embeddings":[[...]]}`（非 OpenAI 的
/// `data[].embedding`）解析正确、多输入行数守恒。
#[tokio::test]
async fn embed_batch_ollama_branch_url_and_response_shape() {
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:1");
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");

    let app = axum::Router::new().route(
        "/api/embed",
        axum::routing::post(|| async {
            axum::Json(serde_json::json!({
                "embeddings": [[0.4, 0.5], [0.6, 0.7]]
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let config = EmbeddingConfig {
        provider: EmbeddingProvider::Ollama,
        base_url: format!("http://{addr}"),
        model: "mock-embed".to_string(),
        api_key_env: None,
        timeout_ms: Some(5_000),
    };
    let vectors = embed_batch(&config, &["甲".to_string(), "乙".to_string()], None)
        .await
        .expect("Ollama 分支必须在死代理 env 下直连成功且解析 embeddings 形态");
    assert_eq!(
        vectors,
        vec![vec![0.4, 0.5], vec![0.6, 0.7]],
        "Ollama embeddings[][] 形态解析 + 输入 2 行输出 2 行守恒"
    );
}

/// 1578 号：embed_batch 错误传播面——上游 5xx 必须报错而非静默空列表
/// （调用方按降级契约回退 FTS5，错误路径是被依赖的契约面）。
#[tokio::test]
async fn embed_batch_error_path_propagates() {
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:1");
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");

    let app = axum::Router::new().route(
        "/embeddings",
        axum::routing::post(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let config = EmbeddingConfig {
        provider: EmbeddingProvider::OpenAICompatible,
        base_url: format!("http://{addr}"),
        model: "mock-embed".to_string(),
        api_key_env: None,
        timeout_ms: Some(5_000),
    };
    let result = embed_batch(&config, &["正文".to_string()], None).await;
    assert!(result.is_err(), "上游 5xx 必须报错（降级契约依赖错误面）");
}
