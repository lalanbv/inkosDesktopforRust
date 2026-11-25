//! 交互聊天（`POST /api/v1/agent`）发给上游的 temperature——产品引擎侧。
//!
//! 两腿共用对照表 `packages/studio/src/__tests__/golden/chat-temperature-parity.json`
//! （Node 腿 `packages/studio/src/__tests__/chat-temperature.flow.test.ts` 读同一文件）：
//! 所选服务项配置了就发、没配置就不发（用模型默认）；模型卡标了服务端固定温度的
//! （kimi-k2.5 系列只接受 1）按卡钳制。
//!
//! 取证（修复前，真实二进制）：交互聊天恒发 0.7——界面「随机性（可选）/ 模型默认」
//! 的设置对聊天不生效；卡上固定温度的模型还会被上游 400（产品引擎此前没有任何
//! 一处消费模型卡的固定温度）。
//!
//! 运行：`cargo test --test chat_temperature`。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::Json;
use axum::http::{Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig, LLM_UNCONFIGURED_BASE_URL};
use inkos_engine::server::audit_route::AuditRuntime;
use inkos_engine::server::books_routes::BooksRuntime;
use inkos_engine::server::sse::BroadcastHub;
use inkos_engine::server::write_next_route::{WriteNextRunner, WriteNextRuntime};
use inkos_engine::server::{router_books, AppState};
use inkos_engine::state::manager::StateManager;

const TABLE: &str =
    include_str!("../../packages/studio/src/__tests__/golden/chat-temperature-parity.json");

/// 测试凭据一律 env 注入 + 非凭据回退。
fn test_key() -> String {
    std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key-placeholder".into())
}

/// 回环上游：记下每个请求体的 `temperature`（`None` = 不带该键；带了却不是数值
/// ——如 `null`——记成 NaN，很多上游会直接拒收），回一段正文（产品引擎的聊天
/// 循环以不带工具调用的正文收尾，一轮恰一次请求）。
async fn spawn_capture_llm() -> (String, Arc<Mutex<Vec<Option<f64>>>>) {
    type Sink = Arc<Mutex<Vec<Option<f64>>>>;
    async fn capture(
        axum::extract::State(sink): axum::extract::State<Sink>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let temperature = body
            .get("temperature")
            .map(|value| value.as_f64().unwrap_or(f64::NAN));
        sink.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(temperature);
        Json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "ok" }, "finish_reason": "stop" }],
        }))
    }
    let captured: Sink = Arc::new(Mutex::new(Vec::new()));
    let app = axum::Router::new()
        .route("/v1/chat/completions", axum::routing::post(capture))
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/v1"), captured)
}

/// 桌壳直启形态：启动路由器 = bin 的未配置占位。
fn unconfigured() -> Arc<AgentRouter> {
    Arc::new(AgentRouter::new(
        LlmEndpointConfig {
            base_url: LLM_UNCONFIGURED_BASE_URL.into(),
            api_key: String::new(),
            model: "default".into(),
            max_tokens: 8192,
            extra_headers: HashMap::new(),
            context_window_tokens: 128_000,
        },
        HashMap::new(),
    ))
}

fn app(root: &Path) -> axum::Router {
    let hub = Arc::new(BroadcastHub::new());
    let state = Arc::new(StateManager::new(root.to_path_buf()));
    let runner: WriteNextRunner =
        Arc::new(|_, _, _, _, _, _| Box::pin(async { Err("unused".to_string()) }));
    let write_next = WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: root.to_path_buf(),
    };
    let audit = AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: unconfigured(),
        builtin_genres_dir: root.join("assets").join("genres"),
    };
    let books = BooksRuntime {
        hub,
        state,
        router: unconfigured(),
        builtin_genres_dir: root.join("assets").join("genres"),
        revision_gate: Default::default(),
    };
    router_books(
        AppState {
            version: "temperature".to_string(),
        },
        BroadcastHub::new().into(),
        write_next,
        audit,
        books,
    )
}

async fn call(
    app: axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let request = builder
        .body(
            body.map(|value| Body::from(value.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn chat_temperature_matches_the_shared_table() {
    let table: Value = serde_json::from_str(TABLE).unwrap();
    let cases = table["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 7, "对照表用例被删");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let (base_url, captured) = spawn_capture_llm().await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let services: Vec<Value> = case["services"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let mut entry = entry.clone();
                entry["baseUrl"] = json!(base_url);
                entry
            })
            .collect();
        let mut llm = json!({ "defaultModel": case["defaultModel"], "services": services });
        if let Some(temperature) = case.get("topLevelTemperature") {
            llm["temperature"] = temperature.clone();
        }
        std::fs::write(
            root.join("inkos.json"),
            json!({ "version": "0.1.0", "name": "temperature", "language": "en", "llm": llm })
                .to_string(),
        )
        .unwrap();
        inkos_engine::llm::secrets::save_secrets(
            root,
            &inkos_engine::llm::secrets::SecretsFile {
                services: [(
                    "custom:fixture".to_string(),
                    inkos_engine::llm::secrets::ServiceSecret {
                        api_key: test_key(),
                    },
                )]
                .into_iter()
                .collect(),
            },
        )
        .unwrap();

        let app = app(root);
        let (status, body) = call(
            app.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(json!({ "sessionKind": "chat" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{name}: {body}");
        let session_id = body["session"]["sessionId"].as_str().unwrap();
        let mut request = json!({ "instruction": "Explain a scene.", "sessionId": session_id });
        for (key, value) in case["request"].as_object().unwrap() {
            request[key] = value.clone();
        }
        let (status, body) = call(app, Method::POST, "/api/v1/agent", Some(request)).await;
        assert_eq!(status, StatusCode::OK, "{name}: {body}");

        let seen = captured.lock().unwrap().clone();
        assert!(!seen.is_empty(), "{name}: 聊天轮没有打到上游");
        // null = 不带该键。
        let expected = case["expect"].as_f64();
        assert_eq!(seen, vec![expected; seen.len()], "{name}");
    }
}
