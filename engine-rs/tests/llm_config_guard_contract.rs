//! 1669 号：模型调用面的 LLM 配置前置守卫——经 `router_books` 的入口契约。
//!
//! 产品路径 = 桌壳直启引擎、不注入 `INKOS_LLM_*`：启动路由器恒为未配置占位
//! （discard 端口 + 空 key），项目 LLM 配置不可用时此前只在实际请求里以连接
//! 拒绝的传输错误暴露（后台任务 → SSE write:error；同步面 → 500）。现在在模型
//! 调用**之前**同步回 `400 {error:{code:"LLM_CONFIG_ERROR",message}}`，与 TS
//! 腿同码同形（studio `llm-config-error.test.ts`）。
//!
//! env 配置的引擎（开发/冒烟脚本经 `INKOS_LLM_*` 起引擎）不拦——项目配置缺失
//! 照常回退启动路由器，行为不变。
//!
//! 运行：`cargo test --test llm_config_guard_contract`。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

/// 与 TS `LLMConfigurationError("MISSING_API_KEY")` 逐字同句（UI 文案表锚）。
const MISSING_KEY: &str =
    "Studio LLM API key not set. Open Studio services and save an API key for the selected service.";
const NO_SERVICE: &str =
    "Studio LLM service not configured. Open Studio services and select a model.";

/// 守卫覆盖的模型调用面（组合层 + audit 处理器内）。
const MODEL_FACES: &[(&str, &str)] = &[
    ("write-next", "/api/v1/books/b1/write-next"),
    ("audit", "/api/v1/books/b1/audit/1"),
    ("plan", "/api/v1/books/b1/plan"),
    ("settle", "/api/v1/books/b1/settle"),
    ("draft", "/api/v1/books/b1/draft"),
    ("revise", "/api/v1/books/b1/revise/1"),
    ("rewrite", "/api/v1/books/b1/rewrite/1"),
    ("resync", "/api/v1/books/b1/resync/1"),
    ("compose", "/api/v1/books/b1/compose"),
    ("consolidate", "/api/v1/books/b1/consolidate"),
    ("repair-state", "/api/v1/books/b1/repair-state/1"),
];

struct App {
    router: axum::Router,
    hub: Arc<BroadcastHub>,
    runner_calls: Arc<AtomicUsize>,
}

fn startup_router(base_url: &str, api_key: &str) -> Arc<AgentRouter> {
    Arc::new(AgentRouter::new(
        LlmEndpointConfig {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: "default".into(),
            max_tokens: 8192,
            extra_headers: HashMap::new(),
            context_window_tokens: 128_000,
        },
        HashMap::new(),
    ))
}

async fn build(root: &Path, startup: Arc<AgentRouter>) -> App {
    let hub = Arc::new(BroadcastHub::new());
    let state = Arc::new(StateManager::new(root.to_path_buf()));
    let runner_calls = Arc::new(AtomicUsize::new(0));
    let calls = runner_calls.clone();
    let runner: inkos_engine::server::WriteNextRunner = Arc::new(move |_, _, _, _, _, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err("contract: the runner must not run while unconfigured".to_string()) })
    });
    let write_next = WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: root.to_path_buf(),
    };
    let audit = AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: startup.clone(),
        builtin_genres_dir: root.join("assets").join("genres"),
    };
    let books = BooksRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: startup,
        builtin_genres_dir: root.join("assets").join("genres"),
        revision_gate: Default::default(),
    };
    // `/api/v1/agent` 由 router_books 自身挂载——无需另加。
    let router = router_books(
        AppState {
            version: "contract".to_string(),
        },
        hub.clone(),
        write_next,
        audit,
        books,
    );
    App {
        router,
        hub,
        runner_calls,
    }
}

async fn call(app: &App, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(
            body.map(|value| Body::from(value.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

fn unconfigured() -> Arc<AgentRouter> {
    startup_router(LLM_UNCONFIGURED_BASE_URL, "")
}

/// 写一个缺 key 的项目配置（非回环端点——回环端点免 key，测不到缺 key）。
fn write_keyless_config(root: &Path) {
    std::fs::write(
        root.join("inkos.json"),
        r#"{"version":"0.1.0","name":"契约项目","language":"zh","llm":{"services":{"custom:None":{"service":"custom","name":"None","baseUrl":"https://example.invalid/v1","apiFormat":"chat"}},"defaultModel":"none-model"},"notify":[]}"#,
    )
    .unwrap();
}

/// agent 面的检查在会话查找之后（顺序同改动前）——先建一个真实会话。
async fn create_session(app: &App, id: &str) {
    let (status, body) = call(
        app,
        Method::POST,
        "/api/v1/sessions",
        Some(json!({ "sessionId": id, "sessionKind": "chat" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "建会话失败：{body}");
}

#[tokio::test]
async fn an_unconfigured_engine_answers_400_llm_config_error_on_every_model_face() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path(), unconfigured()).await;

    for (name, path) in MODEL_FACES {
        let (status, body) = call(&app, Method::POST, path, Some(json!({}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
        assert_eq!(
            body,
            json!({ "error": { "code": "LLM_CONFIG_ERROR", "message": NO_SERVICE } }),
            "{name}"
        );
    }
    // 顺序不变：未知会话仍先报 404（会话查找先于配置检查）。
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/agent",
        Some(json!({ "instruction": "你好", "sessionId": "ghost" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "agent: {body}");
    assert_eq!(body["error"]["code"], "SESSION_NOT_FOUND", "agent: {body}");

    create_session(&app, "1783099000017-d169").await;
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/agent",
        Some(json!({ "instruction": "你好", "sessionId": "1783099000017-d169" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "agent: {body}");
    assert_eq!(body["error"]["code"], "LLM_CONFIG_ERROR", "agent: {body}");
    assert_eq!(body["error"]["message"], NO_SERVICE, "agent: {body}");

    // 检查发生在启动之前：后台 runner 零调用，audit 没有 start/error 广播。
    assert_eq!(app.runner_calls.load(Ordering::SeqCst), 0);
    assert!(
        app.hub.subscribe().try_recv().is_err(),
        "audit 不应留下广播"
    );
}

#[tokio::test]
async fn a_keyless_project_config_reports_the_ui_anchor_text() {
    let dir = tempfile::tempdir().unwrap();
    write_keyless_config(dir.path());
    let app = build(dir.path(), unconfigured()).await;

    for (name, path) in [
        ("write-next", MODEL_FACES[0].1),
        ("audit", MODEL_FACES[1].1),
    ] {
        let (status, body) = call(&app, Method::POST, path, Some(json!({}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
        assert_eq!(body["error"]["message"], MISSING_KEY, "{name}: {body}");
    }
    create_session(&app, "1783099000018-d169").await;
    let (_, body) = call(
        &app,
        Method::POST,
        "/api/v1/agent",
        Some(json!({ "instruction": "你好", "sessionId": "1783099000018-d169" })),
    )
    .await;
    assert_eq!(body["error"]["message"], MISSING_KEY, "agent: {body}");
}

/// 只守聊天轮：确认式生产动作里有不走文本模型的（generate_cover 走封面服务），
/// 不得被聊天模型的缺 key 误拦——以自身的载荷校验错误收场。
#[tokio::test]
async fn confirmed_production_actions_are_not_blocked_by_the_chat_model_check() {
    let dir = tempfile::tempdir().unwrap();
    write_keyless_config(dir.path());
    let app = build(dir.path(), unconfigured()).await;
    create_session(&app, "1783099000019-d169").await;
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/agent",
        Some(json!({
            "instruction": "生成封面",
            "sessionId": "1783099000019-d169",
            "actionSource": "button",
            "requestedIntent": "generate_cover",
        })),
    )
    .await;
    // 以自身的载荷校验错误收场（缺标题），不是 LLM_CONFIG_ERROR。
    assert_eq!(
        body["error"]["code"], "AGENT_ACTION_FAILED",
        "{status} {body}"
    );
    assert_eq!(
        body["error"]["message"], "确认生成封面缺少标题，请重新生成确认卡。",
        "{status} {body}"
    );
}

#[tokio::test]
async fn an_env_configured_engine_falls_back_and_is_not_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path(), startup_router("http://127.0.0.1:9/v1", "k")).await;

    // write-next 照常起后台任务（runner 被调用）——守卫不拦 env 配置的引擎。
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/books/b1/write-next",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "writing", "{body}");
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.runner_calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("runner 未被调用");

    // audit 走进自身流程（此例因书不存在失败），而不是配置守卫。
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/books/b1/audit/1",
        Some(json!({})),
    )
    .await;
    assert_ne!(
        body["error"]["code"], "LLM_CONFIG_ERROR",
        "audit: {status} {body}"
    );

    // agent 越过配置检查，落到会话查找。
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/agent",
        Some(json!({ "instruction": "你好", "sessionId": "ghost" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], "SESSION_NOT_FOUND", "{body}");
}
