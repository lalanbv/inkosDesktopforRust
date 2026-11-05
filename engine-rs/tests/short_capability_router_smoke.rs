//! 896 号验收轮：生产路由表面冒烟（模拟真人测试）——`router_books` 全路由装配
//! （strangler_duel `spawn_rust_engine` 同款）+ 生产形态 HTTP 流。
//!
//! 进程内 e2e（short78 app78）只注册 3 条路由；本测试走**完整路由表**（含
//! capability 路由在 server/mod.rs 的真实注册 + 中间件/状态装配），覆盖"真实
//! 客户端打全量路由表"面；bin 进程装配层由 strangler_duel 既有 bin 测试覆盖。
//!
//! 覆盖面：capability 五动作全流 + 错误三面 + agent 路由 short_run + L1 读面
//! + mock 19 轮按序耗尽核验。
//!
//! 运行：`INKOS_DUEL=1 cargo test --test short_capability_router_smoke -- --nocapture`
//! （缺 env 跳过——证据轮面，不进基线；strangler_duel 同纪律）。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
use inkos_engine::server::books_routes::BooksRuntime;
use inkos_engine::server::router_books;
use inkos_engine::server::AppState;
use inkos_engine::state::manager::StateManager;

fn duel_enabled() -> bool {
    std::env::var("INKOS_DUEL").is_ok()
}

/// 测试键惯用形：env 注入优先，回退非凭据形态特征占位。
fn smoke_api_key() -> String {
    std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key".to_string())
}

// ── mock LLM（result-tool 顺序脚本——mismatch 弹即失，顺序即契约）────────

enum Turn {
    Tool(&'static str, Value),
}

fn script_full_flow() -> Vec<(&'static str, Turn)> {
    vec![
        ("submit_short_outline", Turn::Tool("submit_short_outline", json!({
            "storyTitle": "夜雨冒烟",
            "planMarkdown": "## 总纲\n\n雨夜开局，双线收束。",
            "chapter_1_plan": "开场",
            "chapter_2_plan": "收束",
        }))),
        ("submit_short_draft_batch", Turn::Tool("submit_short_draft_batch", json!({
            "storyTitle": "夜雨冒烟",
            "chapter_1_title": "开场",
            "chapter_1_content": "雨夜凶案发生，追查开始，压力逐层升级。",
            "chapter_2_title": "收束",
            "chapter_2_content": "真相收网，雨停天明，双线在此刻交汇。",
        }))),
        ("submit_short_fiction_review", Turn::Tool("record_review_observation", json!({
            "code": "R-1", "summary": "有依据的观察。", "category": "quality",
            "assessment": "observation",
            "sourceRefs": [{ "sourceId": "outline", "startLine": 1, "endLine": 1 }],
        }))),
        ("submit_short_fiction_review", Turn::Tool("submit_short_fiction_review",
            json!({ "summary": "一处观察。", "observationCodes": ["R-1"] }))),
        ("submit_short_package", Turn::Tool("submit_short_package", json!({
            "title": "夜雨冒烟", "intro": "雨夜凶案，双线追凶。",
            "sellingPoints": ["节奏快"], "coverPrompt": "雨夜街口，灯下人影。",
        }))),
        ("submit_short_package_review", Turn::Tool("record_review_observation", json!({
            "code": "P-1", "summary": "包装承诺核对。", "category": "quality",
            "assessment": "observation",
            "sourceRefs": [{ "sourceId": "package", "startLine": 1, "endLine": 1 }],
        }))),
        ("submit_short_package_review", Turn::Tool("submit_short_package_review",
            json!({ "summary": "无硬伤。", "observationCodes": ["P-1"] }))),
    ]
}

fn script_revise() -> Vec<(&'static str, Turn)> {
    vec![
        ("submit_short_author_scope", Turn::Tool("submit_short_author_scope", json!({
            "wholeManuscript": false, "chapterNumbers": [1],
            "opening": false, "outline": false,
        }))),
        ("submit_edit_location", Turn::Tool("submit_edit_location", json!({
            "kind": "paragraphs", "boundary": "first", "count": 1, "targets": [],
        }))),
        ("submit_chapter_range_replacements", Turn::Tool("submit_chapter_range_replacements", json!({
            "selection_0_text": "雨夜里少年提灯出门，第一层雾漫过河面。",
        }))),
        ("submit_short_fiction_review", Turn::Tool("record_review_observation", json!({
            "code": "R-2", "summary": "修订后观察。", "category": "quality",
            "assessment": "observation",
            "sourceRefs": [{ "sourceId": "outline", "startLine": 1, "endLine": 1 }],
        }))),
        ("submit_short_fiction_review", Turn::Tool("submit_short_fiction_review",
            json!({ "summary": "修订无硬伤。", "observationCodes": ["R-2"] }))),
    ]
}

#[derive(Clone)]
struct RouterSmokeMock {
    queue: Arc<Mutex<VecDeque<(&'static str, Turn)>>>,
    calls: Arc<AtomicUsize>,
}

impl RouterSmokeMock {
    async fn spawn(script: Vec<(&'static str, Turn)>) -> (Self, String) {
        let mock = Self {
            queue: Arc::new(Mutex::new(VecDeque::from(script))),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let handler = mock.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(
                move |axum::Json(body): axum::Json<Value>| {
                    let handler = handler.clone();
                    async move { handler.handle(body).await }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (mock, format!("http://{addr}"))
    }

    async fn handle(&self, body: Value) -> axum::response::Response {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let tools = body["tools"].as_array().cloned().unwrap_or_default();
        let result_tool = tools
            .first()
            .and_then(|tool| tool["function"]["name"].as_str())
            .unwrap_or("")
            .to_string();
        let turn = self.queue.lock().unwrap().pop_front();
        let (status, payload) = match turn {
            Some((expected, Turn::Tool(name, arguments))) if expected == result_tool => {
                let response = json!({
                    "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
                        { "id": "call-1", "type": "function", "function": { "name": name, "arguments": arguments.to_string() } }
                    ] } }],
                    "usage": { "prompt_tokens": 7, "completion_tokens": 7, "total_tokens": 14 },
                });
                (axum::http::StatusCode::OK, serde_json::to_string(&response).unwrap())
            }
            _ => {
                let front = self.queue.lock().unwrap().front().map(|(name, _)| *name);
                eprintln!(
                    "[mock mismatch] got={result_tool:?} expected_front={front:?} calls={}",
                    self.calls.load(Ordering::SeqCst)
                );
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": { "message": "script mismatch or exhausted" } }).to_string(),
                )
            }
        };
        axum::response::Response::builder()
            .status(status)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(payload))
            .unwrap()
    }
}

// ── 全路由装配（spawn_rust_engine 同款）─────────────────────────

fn router_runtime(root: &std::path::Path, llm: &str) -> axum::Router {
    let hub = Arc::new(inkos_engine::server::sse::BroadcastHub::new());
    let state = Arc::new(StateManager::new(root.to_path_buf()));
    let router = Arc::new(AgentRouter::new(
        LlmEndpointConfig {
            context_window_tokens: 128_000,
            base_url: llm.into(),
            api_key: smoke_api_key(),
            model: "smoke-model".into(),
            max_tokens: 8192,
            extra_headers: HashMap::new(),
        },
        HashMap::new(),
    ));
    let runner: inkos_engine::server::write_next_route::WriteNextRunner = Arc::new(
        |_state: Arc<StateManager>,
         _book_id: String,
         _word_count: Option<u32>,
         _temperature: Option<f64>,
         _abort: inkos_engine::interaction::agent_loop::AbortHandle,
         _context: Option<String>| {
            Box::pin(async move {
                Err::<inkos_engine::pipeline::write_next::ChapterPipelineResult, String>(
                    "router smoke: write face not exercised".to_string(),
                )
            })
        },
    );
    let write_next = inkos_engine::server::write_next_route::WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: root.to_path_buf(),
    };
    let audit = inkos_engine::server::audit_route::AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: router.clone(),
        builtin_genres_dir: root.join("assets").join("genres"),
    };
    let books = BooksRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router,
        builtin_genres_dir: root.join("assets").join("genres"),
        revision_gate: Default::default(),
    };
    router_books(
        AppState {
            version: "router-smoke".to_string(),
        },
        hub,
        write_next,
        audit,
        books,
    )
}

async fn post_json(base: &str, path: &str, body: Value) -> (u16, Value) {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}{path}"))
        .json(&body)
        .send()
        .await
        .expect("请求失败");
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn get_json(base: &str, path: &str) -> (u16, Value) {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{base}{path}"))
        .send()
        .await
        .expect("请求失败");
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

// ── 冒烟本体 ─────────────────────────────────────────────────────

fn work_seed(id: &str) -> Value {
    json!({
        "version": 2, "id": id, "title": "夜雨冒烟",
        "profileId": "short-fiction", "language": "zh", "status": "active",
        "lineage": [], "artifacts": [], "metadata": {},
        "createdAt": "2026-11-06T00:00:00.000Z", "updatedAt": "2026-11-06T00:00:00.000Z",
    })
}

fn seed_work(root: &std::path::Path, id: &str) {
    let dir = root.join("works").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("work.json"),
        format!("{}\n", serde_json::to_string_pretty(&work_seed(id)).unwrap()),
    )
    .unwrap();
}

#[tokio::test]
async fn router_capability_smoke() {
    if !duel_enabled() {
        eprintln!("INKOS_DUEL 未设——跳过（路由冒烟属证据轮，不进基线）");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();

    let mut script = Vec::new();
    script.extend(script_full_flow()); // capability 四动作（smoke-a）
    script.extend(script_revise()); // revise（smoke-a）
    script.extend(script_full_flow()); // agent short_run 全链（smoke-b）
    let (mock, llm) = RouterSmokeMock::spawn(script).await;

    let app = router_runtime(&root, &llm);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let base = format!("http://{addr}");

    seed_work(&root, "smoke-a");
    seed_work(&root, "smoke-guard");

    // ═══ ① capability 四动作全流（全路由表）═══
    let (status, parsed) = post_json(
        &base,
        "/api/v1/works/smoke-a/capabilities/short-fiction/plan_short_fiction",
        json!({ "direction": "雨夜追凶，双线收束", "chapters": 2, "charsPerChapter": 900, "coverRequired": false }),
    )
    .await;
    assert_eq!(status, 200, "plan: {parsed}");
    assert_eq!(parsed["details"]["kind"], "short_stage_completed", "plan: {parsed}");
    let (status, parsed) = post_json(
        &base,
        "/api/v1/works/smoke-a/capabilities/short-fiction/draft_short_fiction",
        json!({ "chapters": 2, "charsPerChapter": 900 }),
    )
    .await;
    assert_eq!(status, 200, "draft: {parsed}");
    let (status, parsed) = post_json(
        &base,
        "/api/v1/works/smoke-a/capabilities/short-fiction/review_short_fiction",
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "review: {parsed}");
    let (status, parsed) = post_json(
        &base,
        "/api/v1/works/smoke-a/capabilities/short-fiction/package_short_fiction",
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "package: {parsed}");

    // 产物面（work 实体 + 四阶段 + production-state + coverRequired 优先级）。
    let source = root.join("works/smoke-a/source");
    for path in [
        "outline/v001.md",
        "drafts/v001/draft.json",
        "final/full.md",
        "final/sales-package.json",
        "reviews/draft-v001.md",
        "production-state.json",
    ] {
        assert!(source.join(path).is_file(), "缺少产物 {path}");
    }
    let state: Value = serde_json::from_str(
        &std::fs::read_to_string(source.join("production-state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["target"]["coverRequired"], false, "coverRequired 优先级: {state}");
    assert_eq!(state["delivery"]["status"], "checks_passed", "state: {state}");
    let full = std::fs::read_to_string(source.join("final/full.md")).unwrap();
    assert!(full.starts_with("# 夜雨冒烟"), "final 头: {full}");
    assert!(full.contains("## 第1章 开场"), "{full}");
    assert!(full.contains("## 第2章 收束"), "{full}");

    // ═══ ② revise（scoped 授权链）═══
    let (status, parsed) = post_json(
        &base,
        "/api/v1/works/smoke-a/capabilities/short-fiction/revise_short_fiction",
        json!({ "instruction": "重写第一章", "chapterNumbers": [1] }),
    )
    .await;
    assert_eq!(status, 200, "revise: {parsed}");
    assert_eq!(parsed["details"]["kind"], "short_fiction_revised", "revise: {parsed}");
    let final_json = std::fs::read_to_string(source.join("final/short-story.json")).unwrap();
    assert!(final_json.contains("雨夜里少年提灯出门"), "revise 替换落稿: {final_json}");

    // ═══ ③ 错误三面（真实用户误操作形态；零 LLM 轮）═══
    let (status, parsed) = post_json(
        &base,
        "/api/v1/works/smoke-guard/capabilities/short-fiction/plan_short_fiction",
        json!({}),
    )
    .await;
    assert_eq!(status, 502, "空方向守卫: {parsed}");
    assert_eq!(parsed["error"]["message"], "An outline needs a creative direction");
    let (status, parsed) = post_json(
        &base,
        "/api/v1/works/smoke-a/capabilities/short-fiction/draft_short_fiction",
        json!({ "chapters": 0 }),
    )
    .await;
    assert_eq!(status, 502, "参数违约: {parsed}");
    let message = parsed["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("params decode failed") && message.contains("/chapters: Expected integer >= 1"),
        "{message}"
    );
    let (status, parsed) = post_json(
        &base,
        "/api/v1/works/smoke-a/capabilities/short-fiction/totally_unknown",
        json!({}),
    )
    .await;
    assert_eq!(status, 502, "未知动作: {parsed}");
    assert!(parsed["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("Unknown short-fiction capability action"));

    // ═══ ④ 桌面确认流（agent 路由 short_run 全链）═══
    let (status, _) = post_json(&base, "/api/v1/sessions", json!({ "sessionId": "1782994000096-smoke" })).await;
    assert_eq!(status, 200, "session 创建");
    let (status, parsed) = post_json(
        &base,
        "/api/v1/agent",
        json!({
            "instruction": "写一个短篇",
            "sessionId": "1782994000096-smoke",
            "actionSource": "button",
            "requestedIntent": "short_run",
            "actionPayload": { "shortRun": {
                "direction": "雨夜追凶，双线收束", "storyId": "smoke-b",
                "chapters": 2, "charsPerChapter": 900, "cover": false } },
        }),
    )
    .await;
    assert_eq!(status, 200, "agent short_run: {parsed}");
    let exec = &parsed["details"]["toolExecutions"][0];
    assert_eq!(exec["tool"], "short_fiction_run", "body: {parsed}");
    assert_eq!(exec["status"], "completed", "body: {parsed}");
    assert!(parsed["response"]
        .as_str()
        .unwrap_or_default()
        .starts_with("Short fiction manuscript \"smoke-b\" saved with"), "response: {parsed}");

    // ═══ ⑤ L1 读面 + mock 按序耗尽核验 ═══
    let (status, parsed) = get_json(&base, "/api/v1/works/smoke-a").await;
    assert_eq!(status, 200, "L1 work 读面");
    assert_eq!(parsed["work"]["id"], "smoke-a");
    let (status, parsed) = get_json(&base, "/api/v1/works").await;
    assert_eq!(status, 200);
    let ids: Vec<&str> = parsed["works"]
        .as_array()
        .map(|items| items.iter().filter_map(|w| w["id"].as_str()).collect())
        .unwrap_or_default();
    assert!(ids.contains(&"smoke-b"), "works 列表缺 smoke-b: {parsed}");

    let remaining = mock.queue.lock().unwrap().len();
    assert_eq!(remaining, 0, "mock 19 轮必须全按序消费，剩余 {remaining}");
    assert_eq!(mock.calls.load(Ordering::SeqCst), 19, "总调用数须恰 19");

    server.abort();
    eprintln!("router 冒烟绿：五动作+错误三面+桌面确认流+产物面+L1 读面，19 轮全按序。");
}
