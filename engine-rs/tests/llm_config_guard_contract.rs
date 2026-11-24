//! 1669/1670 号：模型调用面的 LLM 配置前置守卫——经 `router_books` 的入口契约。
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
//! 三份清单：
//! - `GUARDED_ROUTES`：逐处理器取证确认会调文本模型的路由（1669 号首批 11 条 +
//!   1670 号第二批 20 条），未配置引擎上一律 400。新增模型面必须登记在此。
//! - `UNGUARDED_ROUTES`：**不该守**的面——诊断、启发式分析、嵌入可选的检索、
//!   封面服务。错守会让未配置项目上这些功能被误拦。
//! - 产品路径回归：启动路由器是占位、项目配置可用时，模型调用必须走项目配置
//!   （1670 号修掉两处绕过项目配置直调启动路由器的处理器）。
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

/// 守卫覆盖的模型调用面全清单（POST；audit 的守卫在处理器内，其余在组合层）。
const GUARDED_ROUTES: &[(&str, &str)] = &[
    // ── 1669 号首批 ──
    ("write-next", "/api/v1/books/b1/write-next"),
    ("audit", "/api/v1/books/b1/audit/1"),
    ("books plan", "/api/v1/books/b1/plan"),
    ("books settle", "/api/v1/books/b1/settle"),
    ("books draft", "/api/v1/books/b1/draft"),
    ("books revise", "/api/v1/books/b1/revise/1"),
    ("books rewrite", "/api/v1/books/b1/rewrite/1"),
    ("books resync", "/api/v1/books/b1/resync/1"),
    ("books compose", "/api/v1/books/b1/compose"),
    ("books consolidate", "/api/v1/books/b1/consolidate"),
    ("books repair-state", "/api/v1/books/b1/repair-state/1"),
    // ── 1670 号第二批 ──
    (
        "workspace inspiration",
        "/api/v1/books/b1/chapters/1/workspace/inspiration",
    ),
    ("style analyze", "/api/v1/style/analyze"),
    ("style import", "/api/v1/books/b1/style/import"),
    ("import canon", "/api/v1/books/b1/import/canon"),
    ("import canon-file", "/api/v1/books/b1/import/canon-file"),
    ("fanfic init", "/api/v1/fanfic/init"),
    ("fanfic refresh", "/api/v1/books/b1/fanfic/refresh"),
    ("spinoff init", "/api/v1/spinoff/init"),
    ("imitation init", "/api/v1/imitation/init"),
    ("books create", "/api/v1/books/create"),
    ("import chapters", "/api/v1/books/b1/import/chapters"),
    ("foundation revise", "/api/v1/books/b1/foundation/revise"),
    (
        "short-fiction capability",
        "/api/v1/works/w1/capabilities/short-fiction/revise_short_fiction",
    ),
    ("translation run", "/api/v1/translations/t1/run"),
    ("translation revise", "/api/v1/translations/t1/revise"),
    (
        "series-backfill extract",
        "/api/v1/books/b1/series-backfill/extract",
    ),
    ("radar scan", "/api/v1/radar/scan"),
    ("radar analyze", "/api/v1/radar/analyze"),
    ("director directions", "/api/v1/director/directions"),
    ("daemon start", "/api/v1/daemon/start"),
];

/// 不该守的面（方法, 路径, 取证结论）：未配置引擎上必须以**自身**的结果应答，
/// 不得是 `LLM_CONFIG_ERROR`。
const UNGUARDED_ROUTES: &[(Method, &str, &str)] = &[
    (Method::GET, "/api/v1/books", "书目列表：纯读"),
    (
        Method::GET,
        "/api/v1/doctor",
        "诊断面：未配置正是它要报告的事",
    ),
    (Method::POST, "/api/v1/daemon/stop", "停止：不调模型"),
    (
        Method::POST,
        "/api/v1/books/b1/deconstruct",
        "拆书：纯启发式，零 LLM 引用",
    ),
    (
        Method::POST,
        "/api/v1/books/b1/hybrid-search",
        "混合检索：嵌入可选，FTS 回退",
    ),
    (
        Method::POST,
        "/api/v1/style-profiles",
        "文风档案：analyze_style 是纯函数",
    ),
    (Method::GET, "/api/v1/translations", "翻译列表：纯读"),
    (
        Method::POST,
        "/api/v1/books/b1/series-backfill/apply",
        "回填应用：只落盘（抽取才调模型）",
    ),
    (
        Method::POST,
        "/api/v1/play/runs/w1/r1/generate-image",
        "互动叙事生图：走封面服务，不是文本模型",
    ),
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

fn is_llm_config_error(status: StatusCode, body: &Value) -> bool {
    status == StatusCode::BAD_REQUEST && body["error"]["code"] == "LLM_CONFIG_ERROR"
}

/// 测试键：env 注入 + 非凭据回退（源码不落可用凭据字面量）。
fn test_key() -> String {
    std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key-placeholder".to_string())
}

/// 项目配置：服务项指向 `base_url`（`stream:false` → 非流式请求，mock 应答简单）。
fn write_config(root: &Path, base_url: &str) {
    std::fs::write(
        root.join("inkos.json"),
        format!(
            r#"{{"version":"0.1.0","name":"契约项目","language":"zh","llm":{{"services":{{"custom:Mock":{{"service":"custom","name":"Mock","baseUrl":"{base_url}","apiFormat":"chat","stream":false}}}},"defaultModel":"mock-model"}},"notify":[]}}"#
        ),
    )
    .unwrap();
}

/// 写一个缺 key 的项目配置（非回环端点——回环端点免 key，测不到缺 key）。
fn write_keyless_config(root: &Path) {
    write_config(root, "https://example.invalid/v1");
}

fn write_secrets(root: &Path) {
    std::fs::create_dir_all(root.join(".inkos")).unwrap();
    let service_key = test_key();
    std::fs::write(
        root.join(".inkos").join("secrets.json"),
        serde_json::json!({ "services": { "custom:Mock": { "apiKey": service_key } } }).to_string(),
    )
    .unwrap();
}

/// 引擎布局的书夹具（`books/<id>`）：book.json + 第 1 章。
fn seed_engine_book(root: &Path) {
    let book = root.join("books").join("b1");
    std::fs::create_dir_all(book.join("chapters")).unwrap();
    std::fs::write(
        book.join("book.json"),
        r#"{"id":"b1","title":"契约书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"2026-08-17T00:00:00.000Z","updatedAt":"2026-08-17T00:00:00.000Z"}"#,
    )
    .unwrap();
    std::fs::write(
        book.join("chapters").join("0001_风起.md"),
        "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
    )
    .unwrap();
}

/// 进程内 mock LLM：任意 `/chat/completions` 请求都回 `content`（流式请求回 SSE）。
/// 返回 (base_url, 命中计数)。
async fn spawn_mock_llm(content: &'static str) -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(
            move |axum::Json(body): axum::Json<Value>| async move {
                counter.fetch_add(1, Ordering::SeqCst);
                if body["stream"].as_bool().unwrap_or(false) {
                    let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content }, "finish_reason": null }] });
                    let finish = serde_json::json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] });
                    return axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {chunk}\n\ndata: {finish}\n\ndata: [DONE]\n\n"),
                    ));
                }
                axum::response::IntoResponse::into_response(axum::Json(serde_json::json!({
                    "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }],
                    "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
                })))
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), hits)
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

    for (name, path) in GUARDED_ROUTES {
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

/// 负面清单：不该守的面在未配置引擎上照常以自身结果应答——错守等于把诊断、
/// 拆书、检索、封面这些不需要文本模型的功能在未配置项目上一并拦死。
#[tokio::test]
async fn faces_that_do_not_need_the_text_model_are_not_guarded() {
    let dir = tempfile::tempdir().unwrap();
    seed_engine_book(dir.path());
    let app = build(dir.path(), unconfigured()).await;

    for (method, path, why) in UNGUARDED_ROUTES {
        let (status, body) = call(&app, method.clone(), path, Some(json!({}))).await;
        assert!(
            !is_llm_config_error(status, &body),
            "{path}（{why}）不该被 LLM 配置守卫拦截：{status} {body}"
        );
    }
    // 几个可钉死的自身结果。
    let (status, body) = call(&app, Method::GET, "/api/v1/books", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = call(&app, Method::GET, "/api/v1/doctor", None).await;
    assert_eq!(status, StatusCode::OK, "诊断面必须应答：{body}");
    let (status, body) = call(&app, Method::POST, "/api/v1/daemon/stop", Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Daemon not running", "{body}");
}

#[tokio::test]
async fn a_keyless_project_config_reports_the_ui_anchor_text() {
    let dir = tempfile::tempdir().unwrap();
    write_keyless_config(dir.path());
    let app = build(dir.path(), unconfigured()).await;

    for (name, path) in [
        ("write-next", "/api/v1/books/b1/write-next"),
        ("audit", "/api/v1/books/b1/audit/1"),
        ("style analyze", "/api/v1/style/analyze"),
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

/// 确认式生产动作：按意图是否需要文本模型分流（1670 号逐意图取证）。
/// - 需要模型的（建书等）：未配置 → 400 LLM_CONFIG_ERROR（此前要等请求发到
///   discard 端点才以 500「error sending request」失败）。
/// - 不走文本模型的（生成封面走封面服务等）：不检查，以自身的结果收场。
/// - 次序同 TS：坏载荷先报 CONFIRMED_ACTION_PAYLOAD_INCOMPLETE。
#[tokio::test]
async fn confirmed_actions_check_the_model_only_for_model_needing_intents() {
    let dir = tempfile::tempdir().unwrap();
    write_keyless_config(dir.path());
    let app = build(dir.path(), unconfigured()).await;
    create_session(&app, "1783099000019-d169").await;
    let confirmed = |intent: &str, payload: Value| {
        json!({
            "instruction": "确认动作",
            "sessionId": "1783099000019-d169",
            "actionSource": "button",
            "requestedIntent": intent,
            "actionPayload": payload,
        })
    };

    // ① 不走文本模型 + 合法载荷（生成封面，封面服务未配置）：不是 LLM_CONFIG_ERROR。
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/agent",
        Some(confirmed(
            "generate_cover",
            json!({ "generateCover": { "title": "契约书" } }),
        )),
    )
    .await;
    assert!(
        !is_llm_config_error(status, &body),
        "generate_cover 走封面服务，不该被文本模型的配置检查拦截：{status} {body}"
    );

    // ② 需要文本模型 + 坏载荷（缺标题）：载荷校验先于配置检查（TS 次序）。
    // 1670 号对齐上游：载荷缺失 = 400 CONFIRMED_ACTION_PAYLOAD_INCOMPLETE（此前
    // Rust 落 502 AGENT_ACTION_FAILED）。
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/agent",
        Some(confirmed("create_book", json!({ "createBook": {} }))),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["error"]["code"], "CONFIRMED_ACTION_PAYLOAD_INCOMPLETE",
        "{body}"
    );
    assert_eq!(
        body["error"]["message"], "确认建书缺少书名，请重新生成确认卡。",
        "{body}"
    );

    // ③ 需要文本模型 + 合法载荷（建书）：400 LLM_CONFIG_ERROR。
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/agent",
        Some(confirmed(
            "create_book",
            json!({ "createBook": { "title": "契约新书" } }),
        )),
    )
    .await;
    assert!(
        is_llm_config_error(status, &body),
        "create_book 需要文本模型，未配置应回 400 LLM_CONFIG_ERROR：{status} {body}"
    );
    assert_eq!(body["error"]["message"], MISSING_KEY, "{body}");
    assert_eq!(body["response"], body["error"]["message"], "{body}");

    // 建书未起跑：没有留下「创建中」的书目录。
    let book_dirs = match std::fs::read_dir(dir.path().join("books")) {
        Ok(entries) => entries.count(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(book_dirs, 0, "未配置时建书不得起跑");
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

    // 全清单放行（各面以自身结果收场，绝不是 LLM_CONFIG_ERROR）。
    for (name, path) in GUARDED_ROUTES.iter().skip(1) {
        let (status, body) = call(&app, Method::POST, path, Some(json!({}))).await;
        assert!(
            !is_llm_config_error(status, &body),
            "{name}: env 配置的引擎不应被拦：{status} {body}"
        );
    }

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

/// 产品路径回归（1670 号）：桌壳直启 → 启动路由器是未配置占位；项目配置可用时
/// 模型调用必须走项目配置。此前灵感卡直调 `runtime.router`（启动路由器），项目
/// 配好了模型也连不上 discard 端点。
#[tokio::test]
async fn model_faces_use_the_project_config_not_the_startup_router() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (mock_base, hits) = spawn_mock_llm("一张可选的修订灵感卡。").await;
    write_config(root, &mock_base);
    write_secrets(root);
    seed_engine_book(root);
    let app = build(root, unconfigured()).await;

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/books/b1/chapters/1/workspace/inspiration",
        Some(json!({ "brief": "收紧节奏" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["chapterNumber"], 1, "{body}");
    assert_eq!(body["card"], "一张可选的修订灵感卡。", "{body}");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "模型调用必须打在项目配置的端点上"
    );
}

/// env 配置的引擎：需要模型的确认动作照常进入执行（以自身的结果收场——这里是
/// mock 端点不可达的执行失败），不被配置检查拦。
#[tokio::test]
async fn an_env_configured_engine_runs_model_needing_confirmed_actions() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path(), startup_router("http://127.0.0.1:9/v1", "k")).await;
    create_session(&app, "1783099000022-d169").await;
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/v1/agent",
        Some(json!({
            "instruction": "建书",
            "sessionId": "1783099000022-d169",
            "actionSource": "button",
            "requestedIntent": "create_book",
            "actionPayload": { "createBook": { "title": "环境配置新书" } },
        })),
    )
    .await;
    assert!(
        !is_llm_config_error(status, &body),
        "env 配置的引擎不应被拦：{status} {body}"
    );
}
