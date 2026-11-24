//! 1670 号：`/agent` 的模型覆盖（界面显式选定的 service / model）必须对确认式
//! 生产动作生效。
//!
//! 97 号：命中即以 per-request router 覆盖本次请求的**全部**代理（TS
//! pipelineClient——子代理同用前端选定的模型）。Rust 的 `post_agent` 据此构造
//! 一个「覆盖运行时」（router = 选定端点），聊天轮直接用 `runtime.router`；
//! 但确认动作的执行器一律经 `effective_router()` 取路由器——项目配置可用时它
//! 返回的是**项目默认**路由器，界面选定被忽略。项目默认与选定一致时看不出
//! 差异（覆盖解析第 2 层正等于项目默认），显式选了别的服务/模型才暴露。
//!
//! 运行：`cargo test --test agent_model_override`。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig, LLM_UNCONFIGURED_BASE_URL};
use inkos_engine::server::audit_route::AuditRuntime;
use inkos_engine::server::books_routes::BooksRuntime;
use inkos_engine::server::sse::BroadcastHub;
use inkos_engine::server::write_next_route::WriteNextRuntime;
use inkos_engine::server::{router_books, AppState};
use inkos_engine::state::manager::StateManager;

/// 起草结构的模型应答：一份最小的合法故事骨架。
const GRAPH: &str = r#"{"nodes":[{"id":"n-start","title":"开场","type":"start","sceneDesc":"雨夜。"},{"id":"n-end","title":"结局","type":"ending","sceneDesc":"天亮了。"}]}"#;

/// 测试键：env 注入 + 非凭据回退（源码不落可用凭据字面量）。
fn test_key() -> String {
    std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key-placeholder".to_string())
}

/// 进程内 mock LLM：记录命中数，非流式请求回一份合法骨架。
async fn spawn_mock_llm() -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(move || async move {
            counter.fetch_add(1, Ordering::SeqCst);
            axum::Json(json!({
                "choices": [{ "message": { "role": "assistant", "content": GRAPH }, "finish_reason": "stop" }],
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), hits)
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
    let runner: inkos_engine::server::WriteNextRunner =
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
            version: "override".to_string(),
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
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
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

/// 两个服务：`custom:Default`（项目默认，mock A）与 `custom:Chosen`（界面选定，
/// mock B）。回环端点免 key，配置即「可用」。
async fn project_with_two_services(root: &Path) -> (Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let (default_base, default_hits) = spawn_mock_llm().await;
    let (chosen_base, chosen_hits) = spawn_mock_llm().await;
    std::fs::write(
        root.join("inkos.json"),
        format!(
            r#"{{"version":"0.1.0","name":"覆盖项目","language":"zh","llm":{{"service":"custom:Default","services":{{"custom:Default":{{"service":"custom","name":"Default","baseUrl":"{default_base}","apiFormat":"chat","stream":false}},"custom:Chosen":{{"service":"custom","name":"Chosen","baseUrl":"{chosen_base}","apiFormat":"chat","stream":false}}}},"defaultModel":"default-model"}},"notify":[]}}"#
        ),
    )
    .unwrap();
    (default_hits, chosen_hits)
}

#[tokio::test]
async fn an_explicit_model_selection_is_used_by_confirmed_actions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (default_hits, chosen_hits) = project_with_two_services(root).await;
    let app = app(root);
    let (status, body) = call(
        app.clone(),
        Method::POST,
        "/api/v1/sessions",
        Some(json!({ "sessionId": "1783099000031-ov01", "sessionKind": "chat" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(
        app,
        Method::POST,
        "/api/v1/agent",
        Some(json!({
            "instruction": "写一个两节点骨架",
            "sessionId": "1783099000031-ov01",
            "actionSource": "button",
            "requestedIntent": "draft_structure",
            "actionPayload": { "draftStructure": { "projectId": "film-override" } },
            "service": "custom:Chosen",
            "model": "chosen-model",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        chosen_hits.load(Ordering::SeqCst),
        1,
        "确认动作必须打在界面选定的服务上（body: {body}）"
    );
    assert_eq!(
        default_hits.load(Ordering::SeqCst),
        0,
        "界面已显式选定别的服务时，项目默认服务不得被调用"
    );
}

#[tokio::test]
async fn without_an_explicit_selection_the_project_default_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (default_hits, chosen_hits) = project_with_two_services(root).await;
    let app = app(root);
    let (status, body) = call(
        app.clone(),
        Method::POST,
        "/api/v1/sessions",
        Some(json!({ "sessionId": "1783099000032-ov02", "sessionKind": "chat" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(
        app,
        Method::POST,
        "/api/v1/agent",
        Some(json!({
            "instruction": "写一个两节点骨架",
            "sessionId": "1783099000032-ov02",
            "actionSource": "button",
            "requestedIntent": "draft_structure",
            "actionPayload": { "draftStructure": { "projectId": "film-default" } },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(default_hits.load(Ordering::SeqCst), 1, "body: {body}");
    assert_eq!(chosen_hits.load(Ordering::SeqCst), 0, "body: {body}");
}

#[tokio::test]
async fn a_selected_service_without_a_key_is_rejected_before_running() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // 显式选定一个非回环、无 key 的服务——覆盖解析层 1 拒绝（TS 同码）。
    std::fs::write(
        root.join("inkos.json"),
        r#"{"version":"0.1.0","name":"覆盖项目","language":"zh","llm":{"service":"custom:Keyless","services":{"custom:Keyless":{"service":"custom","name":"Keyless","baseUrl":"https://example.invalid/v1","apiFormat":"chat"}},"defaultModel":"m"},"notify":[]}"#,
    )
    .unwrap();
    let _ = test_key();
    let app = app(root);
    let (status, body) = call(
        app.clone(),
        Method::POST,
        "/api/v1/sessions",
        Some(json!({ "sessionId": "1783099000033-ov03" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = call(
        app,
        Method::POST,
        "/api/v1/agent",
        Some(json!({
            "instruction": "你好",
            "sessionId": "1783099000033-ov03",
            "service": "custom:Keyless",
            "model": "m",
        })),
    )
    .await;
    // 覆盖解析层 1 的拒绝面：扁平 { error, response } 双语文案（TS 同形）。
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body,
        json!({
            "error": "请先为 custom:Keyless 配置 API Key",
            "response": "请先在模型配置中为 custom:Keyless 填写 API Key，然后再试。",
        })
    );
}
