//! 1573 号红路活体：死代理 env 下，回环 LLM 端点必须直连成功（旁路生效），
//! 默认 builder（外网语义）必须真走代理而失败——控制红证明 env 代理在本进程
//! 真实在效，防「测试恒绿」假防线。独立测试文件 = 独立进程，env 注入不外泄。
//!
//! 1572/1573 号机制链：reqwest `auto_sys_proxy` 无条件读 env/系统代理
//! （hyper-util `from_env` 恒编译、`system-proxy` 特性额外读 macOS
//! SCDynamicStore 且不读 ExceptionsList）→ 回环请求过代理守护进程；
//! 代理健康时能转发（直连规则），降级窗口全灭。本测试把「降级窗口」
//! 恒定化为死代理端口，行为级锁定旁路。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use inkos_engine::llm::provider::{LLMMessage, LLMRole};
use inkos_engine::llm::providers::TransportApiFormat;
use inkos_engine::llm::streaming_client::{
    ChatCompletionParams, StreamDeadlineSpec, StreamingChatClient,
};

/// SSE 单块线格式（对齐 `streaming_client.rs` 单测夹具线格式）。
fn sse(json: &serde_json::Value) -> String {
    format!("data: {json}\n\n")
}

#[tokio::test]
async fn loopback_llm_endpoint_bypasses_dead_proxy_and_streams() {
    // ── 死代理 env 注入（须在任何 client 构建前）──────────────────
    // 127.0.0.1:1 = discard 协议端口，本机必拒绝连接；reqwest 构建时
    // from_env 读 HTTPS_PROXY → 所有未旁路请求连接即失败（毫秒级红）。
    // 整个文件独占一个测试进程，set_var 不外泄其他测试。
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:1");
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");

    // ── 本地回环 LLM mock（axum SSE）───────────────────────────
    let sse_frame = sse(&serde_json::json!({
        "choices": [{ "delta": { "content": "正文" } }]
    }));
    let finish = sse(&serde_json::json!({
        "choices": [{ "delta": {}, "finish_reason": "stop" }]
    }));
    let parts = Arc::new(vec![sse_frame, finish, "data: [DONE]\n\n".to_string()]);
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(move || {
            let parts = parts.clone();
            async move {
                let body = axum::body::Body::from_stream(async_stream::stream! {
                    for part in parts.iter() {
                        yield Ok::<_, std::convert::Infallible>(part.clone());
                    }
                });
                (
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    body,
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // ── 控制红：默认 builder（未旁路）必须真走死代理而失败────────
    // 若此请求意外成功，说明 env 代理未被 reqwest 采纳（上游行为漂移
    // 或环境异常）——旁路测试的前提失效，必须红着退出强制复核。
    let control = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let control_result = control
        .get(format!("http://{addr}/chat/completions"))
        .send()
        .await;
    assert!(
        control_result.is_err(),
        "控制红失效：默认 builder 竟直连成功——env 代理未生效，本测试前提坍塌（上游 reqwest 行为漂移？），须复核机制链后重写测试"
    );

    // ── 主绿：StreamingChatClient（回环旁路）必须直连成功并流出正文──
    let base = format!("http://{addr}");
    let client = StreamingChatClient::new(base, "key".to_string(), HashMap::new());
    static MSG: std::sync::OnceLock<LLMMessage> = std::sync::OnceLock::new();
    let message = MSG.get_or_init(|| LLMMessage {
        role: LLMRole::User,
        content: "ping".into(),
        tool_calls: None,
        tool_call_id: None,
    });
    let params = ChatCompletionParams {
        model: "test-model",
        messages: std::slice::from_ref(message),
        temperature: Some(0.7),
        max_tokens: 32,
        stream: true,
        api_format: TransportApiFormat::Chat,
        extra: None,
        tools: None,
        images: None,
        progress: None,
        deadline: StreamDeadlineSpec::INTERACTIVE,
        trajectory: None,
    };
    let completion = client
        .stream_chat(&params)
        .await
        .expect("回环端点须旁路死代理直连成功");
    assert_eq!(completion.content, "正文");
}
