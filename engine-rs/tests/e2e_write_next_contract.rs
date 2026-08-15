//! E2E 契约测试：真实 HTTP 栈 + mock LLM 服务驱动 write-next 全链。
//!
//! 契约面对齐 Node sidecar（server.ts L3512-3530）：
//! - `POST /api/v1/books/:id/write-next` → 200 `{status:"writing", bookId}`
//! - SSE `/api/v1/events`：`write:start` → `write:complete`（或 `write:error`）
//!   负载字段 {bookId, chapterNumber, status, title, wordCount}
//! - task-store 会话快照落盘（.inkos/tasks/{sessionId}.json）
//!
//! mock LLM：axum 起本地 `/chat/completions`——按 system prompt 首行分发
//! planner/writer/settler 脚本响应（OpenAI SSE chunks 形态），驱动
//! AgentRouter → StreamingChatClient 的真实网络面。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use inkos_engine::llm::agent_router::{AgentOverride, AgentRouter, LlmEndpointConfig, RoutedAgent, RoutedSettler};
use inkos_engine::pipeline::write_next::{write_next_chapter, WriteNextAgents, WriteNextConfig, WriteNextCtx};
use inkos_engine::server::sse::BroadcastHub;
use inkos_engine::server::write_next_route::{write_next, WriteNextRuntime};
use inkos_engine::state::manager::StateManager;
use inkos_engine::state::store::FsStateStore;

// ---- mock LLM 服务 ----

fn sse_body(content: &str) -> String {
    // OpenAI chat.completion.chunk 形态（sse_parser 消费面）。
    let chunk = serde_json::json!({
        "choices": [{ "delta": { "content": content } }],
    });
    let usage = serde_json::json!({
        "choices": [],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 },
    });
    format!(
        "data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n",
        chunk = chunk,
        usage = usage
    )
}

const PLANNER_RESPONSE: &str = "# 第 1 章 memo\n\n## 本章目标\n主角初次交锋夺得玉符\n\n## 关联线索\n- H01\n\n## 当前任务\n林动在坊市与人对峙，夺回被夺的玉符。\n\n## 读者此刻在等什么\n期待玉符来历揭开。\n本章部分兑现。\n\n## 该兑现的 / 暂不掀的\n- 该兑现：玉符第一步。\n\n## 日常/过渡承担什么任务\n不适用 - 本章无日常过渡。\n\n## 关键抉择过三连问\n- 主角：为什么？利益？人设？\n\n## 章尾必须发生的改变\n信息改变：玉符一角真相。\n\n## 本章 hook 账\nadvance:\n- H01 \"祖符\" → 推进（planted → pressured）\n\n## 不要做\n- 不要降智。\n\n";

const WRITER_RESPONSE: &str = "=== CHAPTER_TITLE ===\n风起\n\n=== CHAPTER_CONTENT ===\n林动睁开双眼，灵气顺着经脉游走。他握紧拳头，多年屈辱自今日起一笔一笔讨回来。远处钟声响起，少年迈步而出，踏入坊市的喧嚣之中。\n\n=== POST_SETTLEMENT ===\n结算完成。\n\n=== RUNTIME_STATE_DELTA ===\n```json\n{\"chapter\": 1, \"chapterSummary\": {\"chapter\": 1, \"title\": \"风起\", \"characters\": \"林动\", \"events\": \"醒来\", \"stateChanges\": \"无\", \"hookActivity\": \"H01 推进\", \"mood\": \"紧张\", \"chapterType\": \"推进章\"}}\n```\n";

async fn mock_llm_chat(
    axum::extract::State(state): axum::extract::State<Arc<Mutex<Vec<String>>>>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> axum::response::Response {
    let system = body["messages"][0]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    state.lock().unwrap().push(system.chars().take(24).collect());

    let content = if system.contains("创作总编") {
        PLANNER_RESPONSE.to_string()
    } else if system.contains("作家") || system.contains("写手") {
        WRITER_RESPONSE.to_string()
    } else if system.contains("审稿") {
        "PASS\n95".to_string()
    } else {
        // settler / 大纲选段 / 压缩 / 分析 / 校验等次要调用给最小合法输出。
        "PASS".to_string()
    };

    axum::response::IntoResponse::into_response((
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        sse_body(&content),
    ))
}

async fn spawn_mock_llm() -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let app = axum::Router::new()
        .route("/chat/completions", axum::routing::post(mock_llm_chat))
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), calls, handle)
}

fn fixture_project(root: &std::path::Path) {
    let book = root.join("books").join("b1");
    std::fs::create_dir_all(book.join("chapters")).unwrap();
    std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
    std::fs::write(
        root.join("assets").join("genres").join("xianxia.md"),
        "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\",\"高潮章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
    )
    .unwrap();
    std::fs::write(
        book.join("book.json"),
        r#"{"id":"b1","title":"测试书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
    )
    .unwrap();
}

/// 九路端口经 AgentRouter 装配（真实 StreamingChatClient 网络面）。
fn build_agents(base_url: &str) -> WriteNextAgents<'static> {
    let router = AgentRouter::new(
        LlmEndpointConfig {
            base_url: base_url.to_string(),
            api_key: "test-key".to_string(),
            model: "test-model".to_string(),
            max_tokens: 8192,
            extra_headers: HashMap::new(),
        },
        HashMap::from([(
            "planner".to_string(),
            AgentOverride { model: Some("plan-model".into()), ..Default::default() },
        )]),
    );
    let leak = |agent: &'static str| -> &'static RoutedAgent {
        Box::leak(Box::new(RoutedAgent { router: router.clone(), agent }))
    };
    let settler: &'static RoutedSettler = Box::leak(Box::new(RoutedSettler {
        router: router.clone(),
        ctx: inkos_engine::agents::writer::WriterCtx {
            project_root: Box::leak(std::path::PathBuf::from("/nonexistent").into_boxed_path()),
            builtin_genres_dir: Box::leak(
                std::path::PathBuf::from("/nonexistent").into_boxed_path(),
            ),
            prompt_store: Box::leak(Box::new(FsStateStore)),
            state_store: Box::leak(Box::new(FsStateStore)),
        },
        chapter_number: 0,
    }));
    WriteNextAgents {
        writer: leak("writer"),
        planner: leak("planner"),
        composer: leak("composer"),
        reviser: leak("reviser"),
        auditor: leak("auditor"),
        full_auditor: None,
        normalizer: leak("length-normalizer"),
        analyzer: leak("chapter-analyzer"),
        state_validator: leak("state-validator"),
        settler,
    }
}

type E2eRunner = Arc<
    dyn Fn(
            Arc<StateManager>,
            String,
            Option<u32>,
            Option<f64>,
        ) -> futures_util::future::BoxFuture<
            'static,
            Result<inkos_engine::pipeline::write_next::ChapterPipelineResult, String>,
        > + Send
        + Sync,
>;

#[tokio::test]
async fn e2e_write_next_contract_matches_node_shape() {
    let dir = tempfile::tempdir().unwrap();
    let (root, project) = (dir.path().to_path_buf(), dir.path().to_path_buf());
    fixture_project(&project);

    let (llm_url, llm_calls, _llm) = spawn_mock_llm().await;
    let state = Arc::new(StateManager::new(project.clone()));
    let hub = Arc::new(BroadcastHub::new());

    // runner 闭包：真实 AgentRouter + write-next。
    let runner_project = project.clone();
    let runner_state = state.clone();
    let runner_url = llm_url.clone();
    let runner: E2eRunner = Arc::new(move |state, book_id, word_count, temperature| {
        let project = runner_project.clone();
        let llm_url = runner_url.clone();
        let _ = &runner_state;
        Box::pin(async move {
            let agents = build_agents(&llm_url);
            let prompt_store: &'static FsStateStore = Box::leak(Box::new(FsStateStore));
            let project_ref: &'static std::path::Path =
                Box::leak(project.into_boxed_path());
            let builtin: &'static std::path::Path =
                Box::leak(project_ref.join("assets").join("genres").into_boxed_path());
            let ctx = WriteNextCtx {
                project_root: project_ref,
                builtin_genres_dir: builtin,
                prompt_store,
                state_store: prompt_store,
                context_budget: None,
                notify: None,
            };
            write_next_chapter(
                &state,
                &agents,
                &ctx,
                &WriteNextConfig::default(),
                &book_id,
                word_count,
                temperature,
                None,
            )
            .await
            .map_err(|e| e.to_string())
        })
    });

    let runtime = WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: root.clone(),
    };
    let app = axum::Router::new()
        .route("/api/v1/books/:id/write-next", axum::routing::post(write_next))
        .with_state(runtime);

    // 请求（Node 契约：body {wordCount?}，响应 {status:"writing", bookId}）。
    let mut subscriber = hub.subscribe();
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/v1/books/b1/write-next")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"wordCount":3000,"sessionId":"sess-e2e"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["status"], "writing");
    assert_eq!(parsed["bookId"], "b1");

    // SSE 事件序列：write:start → write:complete（负载字段对齐 Node）。
    let mut events: Vec<(String, String)> = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while events.len() < 2 && std::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_secs(5), subscriber.recv()).await {
            Ok(Ok(payload)) => events.push((payload.event, payload.data)),
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Err(_) | Ok(Err(_)) => break,
        }
    }
    assert_eq!(events[0].0, "write:start", "事件序列: {events:?}");
    assert!(events[0].1.contains("\"bookId\":\"b1\""));
    assert!(
        events.len() >= 2,
        "应收到 write:complete/error，实际: {events:?}"
    );
    assert!(
        events[1].0 == "write:complete" || events[1].0 == "write:error",
        "第二事件: {events:?}"
    );

    eprintln!("EVENTS: {events:?}");
    // 任务快照落盘（sessionId；complete 广播后异步落盘——轮询等待）。
    let snapshot_path = root.join(".inkos").join("tasks").join("sess-e2e.json");
    let mut snapshot_exists = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if snapshot_path.exists() {
            snapshot_exists = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        snapshot_exists,
        "task-store 快照应落盘: {}",
        snapshot_path.display()
    );
    let snapshot: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&snapshot_path).unwrap()).unwrap();
    assert_eq!(snapshot["version"], 1);
    assert_eq!(snapshot["requestedIntent"], "write_next");

    // mock LLM 收到了真实请求（planner 优先路由 plan-model）。
    let calls = llm_calls.lock().unwrap();
    assert!(
        calls.iter().any(|c| c.contains("创作总编")),
        "planner 应被调用，实际: {calls:?}"
    );
}

#[tokio::test]
async fn e2e_llm_unreachable_pushes_write_error() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().to_path_buf();
    fixture_project(&project);
    let state = Arc::new(StateManager::new(project.clone()));
    let hub = Arc::new(BroadcastHub::new());

    let runner_project = project.clone();
    let runner: E2eRunner = Arc::new(move |state, book_id, word_count, temperature| {
        let project = runner_project.clone();
        Box::pin(async move {
            // 端口 9（discard）——不可达端点。
            let agents = build_agents("http://127.0.0.1:9");
            let prompt_store: &'static FsStateStore = Box::leak(Box::new(FsStateStore));
            let project_ref: &'static std::path::Path =
                Box::leak(project.into_boxed_path());
            let ctx = WriteNextCtx {
                project_root: project_ref,
                builtin_genres_dir: project_ref,
                prompt_store,
                state_store: prompt_store,
                context_budget: None,
                notify: None,
            };
            write_next_chapter(
                &state,
                &agents,
                &ctx,
                &WriteNextConfig::default(),
                &book_id,
                word_count,
                temperature,
                None,
            )
            .await
            .map_err(|e| e.to_string())
        })
    });

    let runtime = WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: dir.path().to_path_buf(),
    };
    let app = axum::Router::new()
        .route("/api/v1/books/:id/write-next", axum::routing::post(write_next))
        .with_state(runtime);

    let mut subscriber = hub.subscribe();
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/v1/books/b1/write-next")
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let mut events: Vec<(String, String)> = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while events.len() < 2 && std::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_secs(5), subscriber.recv()).await {
            Ok(Ok(payload)) => events.push((payload.event, payload.data)),
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Err(_) | Ok(Err(_)) => break,
        }
    }
    assert_eq!(events[0].0, "write:start");
    assert_eq!(events[1].0, "write:error", "事件序列: {events:?}");
    assert!(events[1].1.contains("b1"));
}

use tower::util::ServiceExt;

// ---- 44 号：审计端点 E2E（mock LLM 驱动完整 audit_chapter 编排） ----

mod audit_e2e {
    use super::*;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::audit_route::{audit_chapter, AuditRuntime};

    /// mock 审计 LLM：返回维度审计 JSON（audit_chapter 的四策略解析面）。
    async fn mock_audit_llm(
        _state: axum::extract::State<()>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::response::Response {
        let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
        let content = if system.contains("审") || system.contains("audit") || system.contains("连续") {
            // 维度审计 JSON（parse_audit_result 的 JSON 策略）。
            r#"{"passed": true, "overallScore": 88, "summary": "整体连贯，无硬矛盾。", "issues": [{"severity": "warning", "category": "节奏", "description": "中段推进略缓。", "suggestion": "压缩过渡。"}]}"#.to_string()
        } else {
            "PASS".to_string()
        };
        axum::response::IntoResponse::into_response((
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            sse_body(&content),
        ))
    }

    #[tokio::test]
    async fn audit_endpoint_returns_full_result_and_events() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        // 审计对象章节（fixture 不自带——write-next E2E 靠管线生成）。
        std::fs::write(
            root.join("books").join("b1").join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。他握紧拳头，多年屈辱自今日起一笔一笔讨回来。",
        )
        .unwrap();

        // mock LLM 服务。
        let app = axum::Router::new()
            .route("/chat/completions", axum::routing::post(mock_audit_llm))
            .with_state(());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

        let hub = Arc::new(BroadcastHub::new());
        let runtime = AuditRuntime {
            hub: hub.clone(),
            state: Arc::new(StateManager::new(root.clone())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: format!("http://{addr}"),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 4096,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
        };
        let route_app = axum::Router::new()
            .route("/api/v1/books/:id/audit/:chapter", axum::routing::post(audit_chapter))
            .with_state(runtime);

        let mut subscriber = hub.subscribe();
        let response = route_app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/audit/1")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // 同步契约：200 + AuditResult JSON。
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
        eprintln!("AUDIT-DEBUG: {status} {}", String::from_utf8_lossy(&body));
        assert_eq!(status, 200);
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["passed"], true, "body: {parsed}");
        assert_eq!(parsed["overallScore"], 88);
        assert_eq!(parsed["summary"], "整体连贯，无硬矛盾。");
        assert_eq!(parsed["issues"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["issues"][0]["severity"], "warning");

        // SSE：start → complete {passed}。
        let first = subscriber.recv().await.unwrap();
        assert_eq!(first.event, "audit:start");
        let second = subscriber.recv().await.unwrap();
        assert_eq!(second.event, "audit:complete");
        assert!(second.data.contains("\"passed\":true"));
        assert!(second.data.contains("\"chapter\":1"));
    }
}

// ---- 45 号：books 域端点 E2E（mock LLM 主链） ----

mod books_e2e {
    use super::*;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::{plan, revise, BooksRuntime};

    /// mock：planner（memo 脚本）/ reviser（修稿 TAG 输出）分发。
    async fn mock_books_llm(
        _state: axum::extract::State<()>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::response::Response {
        let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
        let temperature = body["temperature"].as_f64();
        let content = if system.contains("创作总编") {
            PLANNER_RESPONSE.to_string()
        } else if system.contains("修稿编辑") {
            // 修稿提示含"审稿意见"——须先于 audit 分派。
            "=== FIXED_ISSUES ===\n修正了措辞\n\n=== REVISED_CONTENT ===\n林动睁开双眼，灵气顺着经脉游走。他攥紧拳头——多年屈辱，今日起一笔一笔讨回来。\n\n=== UPDATED_STATE ===\n| 字段 | 值 |\n|---|---|\n| 当前章节 | 1 |\n\n=== UPDATED_HOOKS ===\n| hook_id | 状态 |\n|---|---|\n| H01 | progressing |\n".to_string()
        } else if system.contains("审") || system.contains("连续") {
            if temperature == Some(0.0) {
                r#"{"passed": true, "overallScore": 91, "summary": "修订后连贯。", "issues": []}"#.to_string()
            } else {
                r#"{"passed": false, "overallScore": 70, "summary": "有一处问题。", "issues": [{"severity": "warning", "category": "节奏", "description": "略缓。", "suggestion": "压缩。"}]}"#.to_string()
            }
        } else {
            "PASS".to_string()
        };
        axum::response::IntoResponse::into_response((
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            sse_body(&content),
        ))
    }

    async fn spawn_mock() -> String {
        let app = axum::Router::new()
            .route("/chat/completions", axum::routing::post(mock_books_llm))
            .with_state(());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn books_runtime(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.to_string(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 4096,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    #[tokio::test]
    async fn plan_endpoint_returns_plan_chapter_result_shape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let llm = spawn_mock().await;
        let runtime = books_runtime(&root, &llm);
        let app = axum::Router::new()
            .route("/api/v1/books/:id/plan", axum::routing::post(plan))
            .with_state(runtime);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/plan")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // TS PlanChapterResult 形状。
        assert_eq!(parsed["bookId"], "b1");
        assert_eq!(parsed["chapterNumber"], 1);
        assert!(parsed["intentPath"].as_str().unwrap().contains("chapter-0001.intent.md"));
        assert_eq!(parsed["conflicts"], serde_json::json!([]));
        assert!(!parsed["goal"].as_str().unwrap().is_empty());
        // plan.md 持久化落盘。
        assert!(root
            .join("books")
            .join("b1")
            .join("story")
            .join("runtime")
            .join("chapter-0001.plan.md")
            .exists());
    }

    #[tokio::test]
    async fn revise_endpoint_returns_revised_output_and_events() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        std::fs::write(
            root.join("books").join("b1").join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
        )
        .unwrap();
        let llm = spawn_mock().await;
        let runtime = books_runtime(&root, &llm);
        let hub = runtime.hub.clone();
        let mut subscriber = hub.subscribe();
        let app = axum::Router::new()
            .route("/api/v1/books/:id/revise/:chapter", axum::routing::post(revise))
            .with_state(runtime);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/revise/1")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"mode":"polish","brief":"收紧措辞"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // 46 号起为审核环结果（ReviseChainResult 形状）；47 号起 status 对齐
        // TS（post 审计 passed → ready-for-review）。
        assert_eq!(parsed["applied"], true, "body: {parsed}");
        assert_eq!(parsed["status"], "ready-for-review");
        assert!(parsed["revisedContent"].as_str().unwrap().contains("一笔一笔讨回来"));
        assert_eq!(parsed["fixedIssues"].as_array().unwrap().len(), 1);

        // SSE：start → complete。
        assert_eq!(subscriber.recv().await.unwrap().event, "revise:start");
        assert_eq!(subscriber.recv().await.unwrap().event, "revise:complete");
    }
}

// ---- 46 号：compose / consolidate / repair-state / revise 审核环 E2E ----

mod books46_e2e {
    use super::*;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::{
        compose, consolidate_endpoint, repair_state, revise, BooksRuntime,
    };
    use inkos_engine::state::manager::StateManager;

    async fn mock46_llm(
        _state: axum::extract::State<()>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::response::Response {
        let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
        let temperature = body["temperature"].as_f64();
        let content = if system.contains("创作总编") {
            PLANNER_RESPONSE.to_string()
        } else if system.contains("修稿编辑") {
            // 修稿提示含"审稿意见"——须先于 audit 分派。
            "=== FIXED_ISSUES ===\n压缩了中段\n\n=== REVISED_CONTENT ===\n林动睁开双眼，灵气顺经脉游走。他攥紧拳头——屈辱自今日起讨回。\n\n=== UPDATED_STATE ===\n| 字段 | 值 |\n|---|---|\n| 当前章节 | 1 |\n\n=== UPDATED_HOOKS ===\n| hook_id | 状态 |\n|---|---|\n| H01 | progressing |\n".to_string()
        } else if system.contains("审") || system.contains("连续") {
            // 审计：pre（默认温）1 个 warning；post（temp 0）PASS。
            if temperature == Some(0.0) {
                r#"{"passed": true, "overallScore": 90, "summary": "修订后连贯。", "issues": []}"#.to_string()
            } else {
                r#"{"passed": false, "overallScore": 70, "summary": "有一处节奏问题。", "issues": [{"severity": "warning", "category": "节奏", "description": "中段推进略缓。", "suggestion": "压缩。"}]}"#.to_string()
            }
        } else if system.contains("summarizer") {
            "第一卷概括：主角觉醒，进入宗门修行。".to_string()
        } else if system.contains("校验") || system.contains("validator") || system.contains("continuity validator") {
            "PASS".to_string()
        } else if system.contains("结算") || system.contains("settler") {
            WRITER_RESPONSE.to_string()
        } else {
            "PASS".to_string()
        };
        axum::response::IntoResponse::into_response((
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            sse_body(&content),
        ))
    }

    async fn spawn_mock46() -> String {
        let app = axum::Router::new()
            .route("/chat/completions", axum::routing::post(mock46_llm))
            .with_state(());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn rt(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.to_string(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 4096,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    #[tokio::test]
    async fn compose_endpoint_returns_compose_result_shape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let llm = spawn_mock46().await;
        let runtime = rt(&root, &llm);
        let app = axum::Router::new()
            .route("/api/v1/books/:id/compose", axum::routing::post(compose))
            .with_state(runtime);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/compose")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // ComposeChapterResult 形状。
        assert_eq!(parsed["bookId"], "b1");
        assert!(parsed["intentPath"].as_str().unwrap().contains("chapter-0001.intent.md"));
        assert!(parsed["contextPath"].as_str().unwrap().contains("chapter-0001.context.json"));
        assert!(parsed["ruleStackPath"].as_str().unwrap().contains("chapter-0001.rule-stack.yaml"));
        assert!(parsed["tracePath"].as_str().unwrap().contains("chapter-0001.trace.json"));
        // 工件落盘。
        let runtime_dir = root.join("books").join("b1").join("story").join("runtime");
        assert!(runtime_dir.join("chapter-0001.context.json").exists());
        assert!(runtime_dir.join("chapter-0001.rule-stack.yaml").exists());
        assert!(runtime_dir.join("chapter-0001.trace.json").exists());
    }

    #[tokio::test]
    async fn consolidate_endpoint_archives_volume() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let story = root.join("books").join("b1").join("story");
        let outline = story.join("outline");
        std::fs::create_dir_all(&outline).unwrap();
        std::fs::write(
            outline.join("volume_map.md"),
            "## 第一卷（第1-2章）觉醒\n\n- 第 1 章：开端\n- 第 2 章：推进\n",
        )
        .unwrap();
        std::fs::write(
            story.join("chapter_summaries.md"),
            "| 章节 | 标题 |\n| --- | --- |\n| 1 | a |\n| 2 | b |\n| 3 | c |\n",
        )
        .unwrap();
        let llm = spawn_mock46().await;
        let runtime = rt(&root, &llm);
        let hub = runtime.hub.clone();
        let mut subscriber = hub.subscribe();
        let app = axum::Router::new()
            .route("/api/v1/books/:id/consolidate", axum::routing::post(consolidate_endpoint))
            .with_state(runtime);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/consolidate")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["archivedVolumes"], 1);
        assert_eq!(parsed["retainedChapters"], 1);
        assert!(parsed["volumeSummaries"].as_str().unwrap().contains("第一卷"));
        assert_eq!(subscriber.recv().await.unwrap().event, "consolidate:complete");
    }

    #[tokio::test]
    async fn repair_state_requires_degraded_chapter() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        std::fs::write(
            root.join("books").join("b1").join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼。",
        )
        .unwrap();
        let llm = spawn_mock46().await;
        let runtime = rt(&root, &llm);
        let app = axum::Router::new()
            .route("/api/v1/books/:id/repair-state/:chapter", axum::routing::post(repair_state))
            .with_state(runtime);

        // 索引里的 1 号章非降级 → Node 文案错误。
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/repair-state/1")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"], "Chapter 1 is not state-degraded.");
    }

    #[tokio::test]
    async fn revise_chain_runs_audit_fix_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let book = root.join("books").join("b1");
        std::fs::write(
            book.join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
        )
        .unwrap();
        let llm = spawn_mock46().await;
        let runtime = rt(&root, &llm);
        let hub = runtime.hub.clone();
        let mut subscriber = hub.subscribe();
        let app = axum::Router::new()
            .route("/api/v1/books/:id/revise/:chapter", axum::routing::post(revise))
            .with_state(runtime);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/revise/1")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"mode":"polish"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // 审核环：pre 有 warning → 修稿 → applied（47 号 status 对齐 TS）。
        assert_eq!(parsed["applied"], true, "body: {parsed}");
        assert_eq!(parsed["status"], "ready-for-review");
        eprintln!("REV46: {parsed}");
        assert!(parsed["revisedContent"].as_str().unwrap().contains("屈辱自今日起讨回"));
        assert_eq!(parsed["fixedIssues"].as_array().unwrap().len(), 1);

        // 章节文件被改写（标题保留）。
        let saved = std::fs::read_to_string(book.join("chapters").join("0001_风起.md")).unwrap();
        assert!(saved.starts_with("# 第1章 风起"));
        assert!(saved.contains("屈辱自今日起讨回"));

        // SSE。
        assert_eq!(subscriber.recv().await.unwrap().event, "revise:start");
        assert_eq!(subscriber.recv().await.unwrap().event, "revise:complete");
    }
}

// ---- 47 号：merged audit 门控 + /analytics /eval /export E2E ----

mod books47_e2e {
    use super::*;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::{analytics, eval, export, revise, BooksRuntime};
    use inkos_engine::state::manager::StateManager;

    /// mock：pre 审计 1 warning；post（temp 0）审计 3 critical（变差 → strict 拒绝）。
    async fn mock47_llm(
        _state: axum::extract::State<()>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::response::Response {
        let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
        let temperature = body["temperature"].as_f64();
        let content = if system.contains("修稿编辑") {
            "=== FIXED_ISSUES ===\n修正了措辞\n\n=== REVISED_CONTENT ===\n林动睁开双眼，灵气顺经脉游走。他攥紧拳头——多年屈辱，今日起讨回。\n\n=== UPDATED_STATE ===\n| 字段 | 值 |\n|---|---|\n| 当前章节 | 1 |\n\n=== UPDATED_HOOKS ===\n| hook_id | 状态 |\n|---|---|\n| H01 | progressing |\n".to_string()
        } else if system.contains("审") || system.contains("连续") {
            if temperature == Some(0.0) {
                r#"{"passed": false, "overallScore": 40, "summary": "修订后恶化。", "issues": [{"severity": "critical", "category": "设定", "description": "矛盾甲", "suggestion": "查证。"}, {"severity": "critical", "category": "设定", "description": "矛盾乙", "suggestion": "查证。"}, {"severity": "critical", "category": "节奏", "description": "矛盾丙", "suggestion": "压缩。"}]}"#.to_string()
            } else {
                r#"{"passed": false, "overallScore": 70, "summary": "有一处问题。", "issues": [{"severity": "warning", "category": "节奏", "description": "略缓。", "suggestion": "压缩。"}]}"#.to_string()
            }
        } else {
            "PASS".to_string()
        };
        axum::response::IntoResponse::into_response((
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            sse_body(&content),
        ))
    }

    async fn spawn_mock47() -> String {
        let app = axum::Router::new()
            .route("/chat/completions", axum::routing::post(mock47_llm))
            .with_state(());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn rt47(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.to_string(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 4096,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    /// 带 index/章节/伏笔表的完整 fixture。
    fn fixture47(root: &std::path::Path) {
        fixture_project(root);
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("story")).unwrap();
        std::fs::write(
            book.join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n他推开门，发现灯还亮着。",
        )
        .unwrap();
        std::fs::write(
            book.join("chapters").join("0002_云涌.md"),
            "# 第2章 云涌\n\n她沉默。\n\n然后转身。",
        )
        .unwrap();
        std::fs::write(
            book.join("chapters").join("index.json"),
            r#"[
                {"number":1,"title":"风起","status":"approved","wordCount":100,"auditIssues":[],"lengthWarnings":[],"createdAt":"","updatedAt":""},
                {"number":2,"title":"云涌","status":"ready-for-review","wordCount":200,"auditIssues":["[critical] 节奏: 太慢"],"lengthWarnings":[],"createdAt":"","updatedAt":""}
            ]"#,
        )
        .unwrap();
        std::fs::write(
            book.join("story").join("pending_hooks.md"),
            "| 伏笔 | 状态 |\n| --- | --- |\n| 旧信 | 已回收 |\n| 新谜 | 待定 |\n",
        )
        .unwrap();
    }

    async fn get(app: axum::Router, uri: &str) -> axum::http::Response<axum::body::Body> {
        use tower::ServiceExt;
        app.oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn analytics_endpoint_returns_aggregates() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture47(&root);
        let runtime = rt47(&root, "http://127.0.0.1:9");
        let app = axum::Router::new()
            .route("/api/v1/books/:id/analytics", axum::routing::get(analytics))
            .with_state(runtime);
        let response = get(app, "/api/v1/books/b1/analytics").await;
        assert_eq!(response.status(), 200);
        let content_type = response.headers().get("content-type").unwrap().to_str().unwrap().to_string();
        let body = axum::body::to_bytes(response.into_body(), 65536).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(content_type.starts_with("application/json"));
        assert_eq!(parsed["bookId"], "b1");
        assert_eq!(parsed["totalChapters"], 2);
        assert_eq!(parsed["totalWords"], 300);
        assert_eq!(parsed["avgWordsPerChapter"], 150);
        // audited=2，passed(approved+ready-for-review)=2 → 100%。
        assert_eq!(parsed["auditPassRate"], 100);
        assert_eq!(parsed["statusDistribution"]["approved"], 1);
        assert_eq!(parsed["statusDistribution"]["ready-for-review"], 1);
        assert_eq!(parsed["topIssueCategories"][0]["category"], "节奏");
        assert_eq!(parsed["chaptersWithMostIssues"][0]["chapter"], 2);
        assert!(parsed.get("tokenStats").is_none());
    }

    #[tokio::test]
    async fn eval_endpoint_returns_book_eval_shape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture47(&root);
        let runtime = rt47(&root, "http://127.0.0.1:9");
        let app = axum::Router::new()
            .route("/api/v1/books/:id/eval", axum::routing::get(eval))
            .with_state(runtime);
        let response = get(app, "/api/v1/books/b1/eval").await;
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["bookId"], "b1");
        assert_eq!(parsed["totalChapters"], 2);
        assert_eq!(parsed["totalWords"], 300);
        assert_eq!(parsed["auditPassRate"], 100);
        assert_eq!(parsed["hookResolveRate"], 50);
        assert_eq!(parsed["chapters"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["chapters"][1]["auditIssueCount"], 1);
        assert_eq!(parsed["chapters"][1]["status"], "ready-for-review");
        assert_eq!(parsed["qualityTrend"].as_array().unwrap().len(), 2);
        assert!(parsed["qualityScore"].as_u64().unwrap() > 0);

        // 区间过滤。
        let runtime = rt47(&root, "http://127.0.0.1:9");
        let app = axum::Router::new()
            .route("/api/v1/books/:id/eval", axum::routing::get(eval))
            .with_state(runtime);
        let response = get(app, "/api/v1/books/b1/eval?chapters=1").await;
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["totalChapters"], 1);
        assert_eq!(parsed["totalWords"], 100);
        // duplicateTitles 按全量 index（两章标题不同 → 0）。
        assert_eq!(parsed["duplicateTitles"], 0);
    }

    #[tokio::test]
    async fn export_txt_md_epub_and_approved_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture47(&root);
        let runtime = rt47(&root, "http://127.0.0.1:9");

        // txt。
        let app = axum::Router::new()
            .route("/api/v1/books/:id/export", axum::routing::get(export))
            .with_state(runtime.clone());
        let response = get(app, "/api/v1/books/b1/export?format=txt").await;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            response.headers().get("content-disposition").unwrap(),
            "attachment; filename=\"b1.txt\""
        );
        let body = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.starts_with("测试书\n\n"));
        assert!(text.contains("# 第1章 风起"));
        assert!(text.contains("# 第2章 云涌"));

        // md。
        let app = axum::Router::new()
            .route("/api/v1/books/:id/export", axum::routing::get(export))
            .with_state(runtime.clone());
        let response = get(app, "/api/v1/books/b1/export?format=md").await;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/markdown; charset=utf-8"
        );
        let body = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.starts_with("# 测试书\n\n---\n"));

        // epub：zip 魔数 + epub content-type。
        let app = axum::Router::new()
            .route("/api/v1/books/:id/export", axum::routing::get(export))
            .with_state(runtime.clone());
        let response = get(app, "/api/v1/books/b1/export?format=epub").await;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/epub+zip"
        );
        assert_eq!(
            response.headers().get("content-disposition").unwrap(),
            "attachment; filename=\"b1.epub\""
        );
        let body = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        assert_eq!(&body[..4], &[0x50, 0x4B, 0x03, 0x04]);

        // approvedOnly=true：仅 approved 的第 1 章。
        let app = axum::Router::new()
            .route("/api/v1/books/:id/export", axum::routing::get(export))
            .with_state(runtime);
        let response = get(app, "/api/v1/books/b1/export?approvedOnly=true").await;
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("# 第1章 风起"));
        assert!(!text.contains("# 第2章 云涌"));
    }

    #[tokio::test]
    async fn revise_gate_refusal_returns_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture47(&root);
        let llm = spawn_mock47().await;
        let runtime = rt47(&root, &llm);
        let app = axum::Router::new()
            .route("/api/v1/books/:id/revise/:chapter", axum::routing::post(revise))
            .with_state(runtime);

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/revise/1")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"mode":"polish"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // strict 门控：post 3 critical 变差 → 拒绝。
        assert_eq!(parsed["applied"], false, "body: {parsed}");
        assert_eq!(parsed["status"], "unchanged");
        let reason = parsed["skippedReason"].as_str().unwrap();
        assert!(reason.starts_with("Manual revision kept original chapter: before blocking="), "{reason}");
        assert!(reason.contains("critical="));
        assert!(reason.contains("aiTell="));
        let diagnostics = &parsed["revisionDiagnostics"];
        assert!(diagnostics["standard"]
            .as_str()
            .unwrap()
            .starts_with("A revision is applied only when blocking, critical, and AI-tell counts"));
        assert_eq!(diagnostics["before"]["blockingCount"].as_u64().unwrap(), 1);
        assert_eq!(diagnostics["after"]["blockingCount"].as_u64().unwrap(), 3);
        assert_eq!(diagnostics["after"]["criticalCount"].as_u64().unwrap(), 3);
        let remaining = diagnostics["remainingIssues"].as_array().unwrap();
        assert_eq!(remaining.len(), 3);
        assert_eq!(remaining[0]["severity"], "critical");
        assert_eq!(remaining[0]["category"], "设定");
        assert_eq!(remaining[0]["suggestion"], "查证。");

        // 拒绝时章节文件保持原文。
        let saved = std::fs::read_to_string(root.join("books").join("b1").join("chapters").join("0001_风起.md"))
            .unwrap();
        assert!(saved.contains("他推开门，发现灯还亮着。"));
        assert!(!saved.contains("今日起讨回"));
    }
}

// ---- 48 号：books 状态端点 E2E（列表/章节/approve/reject/truth/review-mode） ----

mod books48_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::books_state_routes::{
        approve_chapter, book_detail, delete_book, get_review_mode, list_books, put_review_mode,
        read_chapter, reject_chapter, truth_file, truth_list,
    };
    use inkos_engine::state::manager::StateManager;

    fn rt48(root: &std::path::Path) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: "http://127.0.0.1:9".into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 4096,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    fn fixture48(root: &std::path::Path) {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story").join("outline")).unwrap();
        std::fs::create_dir_all(book.join("story").join("roles").join("主要角色")).unwrap();
        std::fs::create_dir_all(book.join("story").join("runtime")).unwrap();
        std::fs::write(root.join("inkos.json"), r#"{ "writing": { "reviewMode": "auto" } }"#).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"测试书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(book.join("chapters").join("0001_风起.md"), "# 第1章 风起\n\n林动睁开双眼。").unwrap();
        std::fs::write(book.join("chapters").join("0002_云涌.md"), "# 第2章 云涌\n\n正文二。").unwrap();
        std::fs::write(book.join("story").join("current_state.md"), "状态v1").unwrap();
        std::fs::write(book.join("story").join("pending_hooks.md"), "| 伏笔 | 状态 |").unwrap();
        std::fs::write(book.join("story").join("outline").join("volume_map.md"), "# 卷册地图").unwrap();
        std::fs::write(book.join("story").join("roles").join("主要角色").join("林动.md"), "# 林动").unwrap();
        std::fs::write(
            book.join("story").join("runtime").join("chapter-0001.plan.md"),
            "# plan",
        )
        .unwrap();
        let now = "2026-01-01T00:00:00.000Z";
        let meta = |number: u32, status: &str| {
            serde_json::json!({
                "number": number, "title": format!("第{number}章"), "status": status,
                "wordCount": 10, "auditIssues": [], "lengthWarnings": [],
                "createdAt": now, "updatedAt": now,
            })
        };
        std::fs::write(
            book.join("chapters").join("index.json"),
            serde_json::to_string(&vec![meta(1, "ready-for-review"), meta(2, "ready-for-review")]).unwrap(),
        )
        .unwrap();
    }

    fn app48(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/books", axum::routing::get(list_books))
            .route("/api/v1/books/:id", axum::routing::get(book_detail).delete(delete_book))
            .route("/api/v1/books/:id/chapters/:num", axum::routing::get(read_chapter))
            .route("/api/v1/books/:id/chapters/:num/approve", axum::routing::post(approve_chapter))
            .route("/api/v1/books/:id/chapters/:num/reject", axum::routing::post(reject_chapter))
            .route("/api/v1/books/:id/truth", axum::routing::get(truth_list))
            .route("/api/v1/books/:id/truth/*file", axum::routing::get(truth_file))
            .route(
                "/api/v1/books/:id/chapter-review-mode",
                axum::routing::get(get_review_mode).put(put_review_mode),
            )
            .with_state(runtime)
    }

    async fn call(app: axum::Router, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request = builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) };
        (status, parsed)
    }

    #[tokio::test]
    async fn books_list_and_detail_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture48(&root);
        let (status, parsed) = call(app48(rt48(&root)), "GET", "/api/v1/books/b1", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["book"]["id"], "b1");
        assert_eq!(parsed["chapters"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["nextChapter"], 3);
        let (status, parsed) = call(app48(rt48(&root)), "GET", "/api/v1/books", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["books"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["books"][0]["chaptersWritten"], 2);
        let (status, parsed) = call(app48(rt48(&root)), "GET", "/api/v1/books/ghost", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"], "Book \"ghost\" not found");
    }

    #[tokio::test]
    async fn chapter_read_and_approve() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture48(&root);
        let (status, parsed) = call(app48(rt48(&root)), "GET", "/api/v1/books/b1/chapters/2", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["filename"], "0002_云涌.md");
        assert!(parsed["content"].as_str().unwrap().contains("正文二"));
        let (status, _) = call(app48(rt48(&root)), "GET", "/api/v1/books/b1/chapters/9", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, parsed) = call(app48(rt48(&root)), "POST", "/api/v1/books/b1/chapters/2/approve", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["status"], "approved");
        let index = std::fs::read_to_string(root.join("books").join("b1").join("chapters").join("index.json")).unwrap();
        assert!(index.contains("\"approved\""));
    }

    #[tokio::test]
    async fn reject_rolls_back_discarding_later_chapters() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture48(&root);
        let state = StateManager::new(&root);
        state.snapshot_state("b1", 1).await.unwrap();
        // 状态前进到 v2 → reject 第 2 章应回滚到快照。
        std::fs::write(root.join("books").join("b1").join("story").join("current_state.md"), "状态v2").unwrap();

        let (status, parsed) = call(app48(rt48(&root)), "POST", "/api/v1/books/b1/chapters/2/reject", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["rolledBackTo"], 1);
        assert_eq!(parsed["discarded"], serde_json::json!([2]));
        assert!(!root.join("books").join("b1").join("chapters").join("0002_云涌.md").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("books").join("b1").join("story").join("current_state.md")).unwrap(),
            "状态v1"
        );
        let (status, parsed) = call(app48(rt48(&root)), "POST", "/api/v1/books/b1/chapters/9/reject", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"], "Chapter 9 not found");
    }

    #[tokio::test]
    async fn truth_list_and_nested_wildcard_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture48(&root);
        let (status, parsed) = call(app48(rt48(&root)), "GET", "/api/v1/books/b1/truth", None).await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = parsed["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"current_state.md"));
        assert!(names.contains(&"outline/volume_map.md"));
        assert!(names.contains(&"roles/主要角色/林动.md"));
        // runtime 诊断文件列出且标 readonly。
        let runtime_entry = parsed["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "runtime/chapter-0001.plan.md")
            .expect("runtime 诊断文件应列出");
        assert_eq!(runtime_entry["readonly"], true);
        assert_eq!(runtime_entry["readonlyReason"], "runtime-diagnostic");

        // 嵌套 wildcard：roles 与 runtime 均可达。
        let (status, parsed) = call(
            app48(rt48(&root)),
            "GET",
            "/api/v1/books/b1/truth/roles/%E4%B8%BB%E8%A6%81%E8%A7%92%E8%89%B2/%E6%9E%97%E5%8A%A8.md",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["content"], "# 林动");
        let (status, parsed) =
            call(app48(rt48(&root)), "GET", "/api/v1/books/b1/truth/runtime/chapter-0001.plan.md", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["readonly"], true);
        // 白名单外 → 400。
        let (status, parsed) =
            call(app48(rt48(&root)), "GET", "/api/v1/books/b1/truth/secret/evil.md", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "Invalid truth file");
    }

    #[tokio::test]
    async fn review_mode_roundtrip_and_invalid_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture48(&root);
        // project=auto，book 未设 → mode auto。
        let (status, parsed) =
            call(app48(rt48(&root)), "GET", "/api/v1/books/b1/chapter-review-mode", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["mode"], "auto");
        assert!(parsed["bookMode"].is_null());

        // manual → bookMode 覆盖 project。
        let (status, parsed) = call(
            app48(rt48(&root)),
            "PUT",
            "/api/v1/books/b1/chapter-review-mode",
            Some(r#"{"mode":"manual"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["mode"], "manual");
        assert_eq!(parsed["bookMode"], "manual");

        // 不安全 id → 400。
        let (status, parsed) = call(
            app48(rt48(&root)),
            "GET",
            "/api/v1/books/.%2E%2Fescape/chapter-review-mode",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
    }

    #[tokio::test]
    async fn delete_book_removes_directory_and_broadcasts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture48(&root);
        let runtime = rt48(&root);
        let mut subscriber = runtime.hub.subscribe();
        let (status, parsed) = call(app48(runtime), "DELETE", "/api/v1/books/b1", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["ok"], true);
        assert!(!root.join("books").join("b1").exists());
        assert_eq!(subscriber.recv().await.unwrap().event, "book:deleted");
    }
}

mod books49_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::books_state_routes::{
        chapter_workspace, delete_chapter, get_chapter_version, put_chapter, put_workspace_brief,
        read_chapter, restore_chapter_version,
    };
    use inkos_engine::state::manager::StateManager;

    fn rt49(root: &std::path::Path) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: "http://127.0.0.1:9".into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 4096,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    const VERSION_ID: &str = "1782864000000_manual_11111111-1111-4111-8111-111111111111";

    /// fixture48 同款 + 快照（snapshots/1）+ 第 2 章 runtime 工件 + 归档版本。
    fn fixture49(root: &std::path::Path) {
        let book = root.join("books").join("b1");
        let story = book.join("story");
        std::fs::create_dir_all(book.join("chapters").join(".versions").join("0002")).unwrap();
        std::fs::create_dir_all(story.join("runtime")).unwrap();
        std::fs::create_dir_all(story.join("snapshots").join("1")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"测试书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(book.join("chapters").join("0001_风起.md"), "# 第1章 风起\n\n林动睁开双眼。").unwrap();
        std::fs::write(book.join("chapters").join("0002_云涌.md"), "# 第2章 云涌\n\n正文二。").unwrap();
        std::fs::write(story.join("current_state.md"), "状态v2").unwrap();
        std::fs::write(story.join("pending_hooks.md"), "| 伏笔 | 状态 |").unwrap();
        std::fs::write(story.join("snapshots").join("1").join("current_state.md"), "状态v1").unwrap();
        std::fs::write(story.join("snapshots").join("1").join("pending_hooks.md"), "钩子v1").unwrap();
        std::fs::write(story.join("runtime").join("chapter-0002.plan.md"), "# plan").unwrap();
        std::fs::write(story.join("runtime").join("chapter-0002.user-brief.md"), "  保留证人的原话。  \n").unwrap();
        std::fs::write(
            book.join("chapters").join(".versions").join("0002").join(format!("{VERSION_ID}.md")),
            "# 第2章 旧稿\n\n旧正文。",
        )
        .unwrap();
        let now = "2026-01-01T00:00:00.000Z";
        let meta = |number: u32| {
            serde_json::json!({
                "number": number, "title": format!("第{number}章"), "status": "ready-for-review",
                "wordCount": 10, "auditIssues": [], "lengthWarnings": [],
                "createdAt": now, "updatedAt": now,
            })
        };
        std::fs::write(
            book.join("chapters").join("index.json"),
            serde_json::to_string(&vec![meta(1), meta(2)]).unwrap(),
        )
        .unwrap();
    }

    fn app49(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/books/:id/chapters/:num",
                axum::routing::get(read_chapter).put(put_chapter).delete(delete_chapter),
            )
            .route("/api/v1/books/:id/chapters/:num/workspace", axum::routing::get(chapter_workspace))
            .route(
                "/api/v1/books/:id/chapters/:num/workspace/brief",
                axum::routing::put(put_workspace_brief),
            )
            .route(
                "/api/v1/books/:id/chapters/:num/versions/:versionId",
                axum::routing::get(get_chapter_version),
            )
            .route(
                "/api/v1/books/:id/chapters/:num/versions/:versionId/restore",
                axum::routing::post(restore_chapter_version),
            )
            .with_state(runtime)
    }

    async fn call(app: axum::Router, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request = builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) };
        (status, parsed)
    }

    #[tokio::test]
    async fn workspace_exposes_brief_plan_versions_and_can_delete() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture49(&root);

        let (status, parsed) =
            call(app49(rt49(&root)), "GET", "/api/v1/books/b1/chapters/2/workspace", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["chapterNumber"], 2);
        assert_eq!(parsed["brief"], "保留证人的原话。");
        assert!(parsed["plan"].as_str().unwrap().contains("# plan"));
        assert_eq!(parsed["canDelete"], true);
        let versions = parsed["versions"].as_array().unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0]["id"], VERSION_ID);
        assert_eq!(versions[0]["source"], "manual");
        assert_eq!(versions[0]["chapterNumber"], 2);

        // 非最新章 canDelete=false；非法章节号 400。
        let (_, parsed) =
            call(app49(rt49(&root)), "GET", "/api/v1/books/b1/chapters/1/workspace", None).await;
        assert_eq!(parsed["canDelete"], false);
        let (status, parsed) =
            call(app49(rt49(&root)), "GET", "/api/v1/books/b1/chapters/abc/workspace", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "Invalid chapter number");

        // 版本读：命中 200 / 非法 id 404 / 缺失文件 404。
        let (status, parsed) = call(
            app49(rt49(&root)),
            "GET",
            &format!("/api/v1/books/b1/chapters/2/versions/{VERSION_ID}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(parsed["content"].as_str().unwrap().contains("旧正文"));
        let (status, _) =
            call(app49(rt49(&root)), "GET", "/api/v1/books/b1/chapters/2/versions/bad-id", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(
            app49(rt49(&root)),
            "GET",
            "/api/v1/books/b1/chapters/2/versions/1782864000000_restore_11111111-1111-4111-8111-111111111111",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn brief_put_persists_trims_and_deletes_on_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture49(&root);
        let brief_path = root.join("books").join("b1").join("story").join("runtime").join("chapter-0002.user-brief.md");

        let (status, parsed) = call(
            app49(rt49(&root)),
            "PUT",
            "/api/v1/books/b1/chapters/2/workspace/brief",
            Some(r#"{ "brief": "  让证人先撒谎。  " }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["brief"], "让证人先撒谎。");
        assert_eq!(std::fs::read_to_string(&brief_path).unwrap(), "让证人先撒谎。\n");

        // 非 string / 无效 JSON / 非法章节号 → 400。
        let (status, parsed) = call(
            app49(rt49(&root)),
            "PUT",
            "/api/v1/books/b1/chapters/2/workspace/brief",
            Some(r#"{ "brief": 123 }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "A valid chapter number and brief string are required");
        let (status, _) = call(
            app49(rt49(&root)),
            "PUT",
            "/api/v1/books/b1/chapters/2/workspace/brief",
            Some("{oops"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // 空白 brief → 文件删除。
        let (status, _) = call(
            app49(rt49(&root)),
            "PUT",
            "/api/v1/books/b1/chapters/2/workspace/brief",
            Some(r#"{ "brief": "   " }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!brief_path.exists());
    }

    #[tokio::test]
    async fn put_chapter_replaces_archives_and_marks_review() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture49(&root);
        let book = root.join("books").join("b1");

        let (status, parsed) = call(
            app49(rt49(&root)),
            "PUT",
            "/api/v1/books/b1/chapters/2",
            Some(r##"{ "content": "# 第2章 新稿\n\n人工修改后的正文。" }"##),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["chapterNumber"], 2);
        assert_eq!(parsed["result"]["transactionType"], "chapter-replace");
        assert_eq!(parsed["result"]["reviewRequired"], true);
        assert_eq!(
            parsed["result"]["summary"],
            "Replaced chapter 2 and marked it for review."
        );
        let touched = parsed["result"]["touchedFiles"].as_array().unwrap();
        assert!(touched.iter().any(|f| f == "chapters/0002_云涌.md"));
        assert!(touched.iter().any(|f| f == "story/runtime/chapter-0002.plan.md"));
        assert!(touched.iter().any(|f| f == "chapters/index.json"));

        // 正文覆写 + 尾换行；旧稿归档 _manual_。
        assert_eq!(
            std::fs::read_to_string(book.join("chapters").join("0002_云涌.md")).unwrap(),
            "# 第2章 新稿\n\n人工修改后的正文。\n"
        );
        let versions_dir = book.join("chapters").join(".versions").join("0002");
        let names: Vec<String> = std::fs::read_dir(&versions_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2);
        let manual = names.iter().find(|n| n.contains("_manual_")).unwrap();
        assert_eq!(
            std::fs::read_to_string(versions_dir.join(manual)).unwrap(),
            "# 第2章 云涌\n\n正文二。"
        );
        // runtime：plan 清除、user-brief 保留。
        assert!(!book.join("story").join("runtime").join("chapter-0002.plan.md").exists());
        assert!(book.join("story").join("runtime").join("chapter-0002.user-brief.md").exists());
        // 索引：audit-failed + [warning]。
        let index: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(book.join("chapters").join("index.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(index[1]["status"], "audit-failed");
        assert_eq!(
            index[1]["auditIssues"][0],
            "[warning] Manual chapter replacement requires review before continuation."
        );

        // 空正文 → 500 逐字文案；非法章节号 → 500 Chapter NaN not found.
        let (status, parsed) = call(
            app49(rt49(&root)),
            "PUT",
            "/api/v1/books/b1/chapters/2",
            Some(r#"{ "content": "  " }"#),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"], "Chapter replacement requires fullText.");
        let (status, parsed) = call(
            app49(rt49(&root)),
            "PUT",
            "/api/v1/books/b1/chapters/abc",
            Some(r#"{ "content": "x" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"], "Chapter NaN not found.");
    }

    #[tokio::test]
    async fn restore_version_replaces_content_and_broadcasts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture49(&root);
        let book = root.join("books").join("b1");
        let runtime = rt49(&root);
        let mut subscriber = runtime.hub.subscribe();

        let (status, parsed) = call(
            app49(runtime),
            "POST",
            &format!("/api/v1/books/b1/chapters/2/versions/{VERSION_ID}/restore"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["versionId"], VERSION_ID);
        assert_eq!(parsed["result"]["transactionType"], "chapter-replace");
        assert_eq!(
            std::fs::read_to_string(book.join("chapters").join("0002_云涌.md")).unwrap(),
            "# 第2章 旧稿\n\n旧正文。\n"
        );
        assert_eq!(subscriber.recv().await.unwrap().event, "chapter:restored");

        // 缺失版本 → 500（restore 走 catch，非 404）。
        let (status, _) = call(
            app49(rt49(&root)),
            "POST",
            "/api/v1/books/b1/chapters/2/versions/1782864000000_restore_11111111-1111-4111-8111-111111111111/restore",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn delete_latest_trashes_rolls_back_and_broadcasts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture49(&root);
        let book = root.join("books").join("b1");
        let runtime = rt49(&root);
        let mut subscriber = runtime.hub.subscribe();

        let (status, parsed) =
            call(app49(runtime), "DELETE", "/api/v1/books/b1/chapters/2", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["bookId"], "b1");
        assert_eq!(parsed["deletedChapter"], 2);
        assert_eq!(parsed["title"], "第2章");
        assert_eq!(parsed["rolledBackTo"], 1);
        assert_eq!(parsed["discarded"], serde_json::json!([2]));
        assert_eq!(parsed["trashedFiles"], serde_json::json!(["chapters/.trash/0002_云涌.md"]));
        assert!(book.join("chapters").join(".trash").join("0002_云涌.md").exists());
        assert!(!book.join("chapters").join("0002_云涌.md").exists());
        // 快照恢复 + 索引回写 + runtime 工件清除。
        assert_eq!(
            std::fs::read_to_string(book.join("story").join("current_state.md")).unwrap(),
            "状态v1"
        );
        assert!(!book.join("story").join("runtime").join("chapter-0002.plan.md").exists());
        let index: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(book.join("chapters").join("index.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(index.as_array().unwrap().len(), 1);
        assert_eq!(index[0]["number"], 1);
        assert_eq!(subscriber.recv().await.unwrap().event, "chapter:deleted");

        // 此后删第 1 章：rollbackTarget=0 无快照 → 400 且文件不动。
        let (status, parsed) =
            call(app49(rt49(&root)), "DELETE", "/api/v1/books/b1/chapters/1", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            parsed["error"],
            "Cannot delete chapter 1: the state snapshot for chapter 0 is missing \
(story/snapshots/0/current_state.md). Nothing was changed."
        );
        assert!(book.join("chapters").join("0001_风起.md").exists());

        // 非最新章 → 400（重装 fixture 后请求中间章）。
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture49(&root);
        let (status, parsed) =
            call(app49(rt49(&root)), "DELETE", "/api/v1/books/b1/chapters/1", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            parsed["error"],
            "Only the latest chapter (2) can be deleted, but chapter 1 was requested. \
Deleting a middle chapter would require renumbering later chapters and replaying state."
        );
    }
}

mod books50_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::books_state_routes::{export_save, post_workspace_inspiration};
    use inkos_engine::state::manager::StateManager;

    fn rt50(root: &std::path::Path, llm_url: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm_url.to_string(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 4096,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    fn fixture50(root: &std::path::Path) {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story").join("runtime")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"测试书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(book.join("chapters").join("0001_风起.md"), "# 第1章 风起\n\n林动睁开双眼。").unwrap();
        std::fs::write(book.join("chapters").join("0002_云涌.md"), "# 第2章 云涌\n\n正文二。").unwrap();
        std::fs::write(book.join("story").join("runtime").join("chapter-0002.plan.md"), "# plan").unwrap();
        std::fs::write(book.join("story").join("runtime").join("chapter-0002.user-brief.md"), "保留证人原话。\n").unwrap();
        let now = "2026-01-01T00:00:00.000Z";
        let meta = |number: u32, status: &str| {
            serde_json::json!({
                "number": number, "title": format!("第{number}章"), "status": status,
                "wordCount": 10, "auditIssues": [], "lengthWarnings": [],
                "createdAt": now, "updatedAt": now,
            })
        };
        std::fs::write(
            book.join("chapters").join("index.json"),
            serde_json::to_string(&vec![meta(1, "approved"), meta(2, "ready-for-review")]).unwrap(),
        )
        .unwrap();
    }

    fn app50(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/books/:id/export-save", axum::routing::post(export_save))
            .route(
                "/api/v1/books/:id/chapters/:num/workspace/inspiration",
                axum::routing::post(post_workspace_inspiration),
            )
            .with_state(runtime)
    }

    async fn call(app: axum::Router, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request = builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) };
        (status, parsed)
    }

    #[tokio::test]
    async fn export_save_writes_book_dir_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture50(&root);
        let book = root.join("books").join("b1");

        // md 全量：两章。
        let (status, parsed) = call(
            app50(rt50(&root, "http://127.0.0.1:9")),
            "POST",
            "/api/v1/books/b1/export-save",
            Some(r#"{ "format": "md", "approvedOnly": false }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["format"], "md");
        assert_eq!(parsed["chapters"], 2);
        assert!(parsed["path"].as_str().unwrap().ends_with("b1/b1.md"));
        let md = std::fs::read_to_string(book.join("b1.md")).unwrap();
        assert!(md.contains("# 第1章 风起"));

        // approvedOnly：仅 1 章 approved。
        let (status, parsed) = call(
            app50(rt50(&root, "http://127.0.0.1:9")),
            "POST",
            "/api/v1/books/b1/export-save",
            Some(r#"{ "format": "txt", "approvedOnly": true }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["chapters"], 1);
        assert!(std::fs::read_to_string(book.join("b1.txt")).unwrap().contains("林动睁开双眼"));

        // 无效 JSON → 默认 txt/全量（TS .catch 语义）。
        let (status, parsed) = call(
            app50(rt50(&root, "http://127.0.0.1:9")),
            "POST",
            "/api/v1/books/b1/export-save",
            Some("{oops"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["format"], "txt");
        assert_eq!(parsed["chapters"], 2);
    }

    #[tokio::test]
    async fn export_save_empty_selection_is_500() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture50(&root);
        // 全部章未 approved → approvedOnly 空 → TS "No chapters to export."。
        let index_path = root.join("books").join("b1").join("chapters").join("index.json");
        let raw = std::fs::read_to_string(&index_path).unwrap().replace("approved", "drafting");
        std::fs::write(&index_path, raw).unwrap();
        let (status, parsed) = call(
            app50(rt50(&root, "http://127.0.0.1:9")),
            "POST",
            "/api/v1/books/b1/export-save",
            Some(r#"{ "format": "md", "approvedOnly": true }"#),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"], "No chapters to export.");
    }

    #[tokio::test]
    async fn inspiration_calls_llm_and_returns_card() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture50(&root);
        let (llm_url, _calls, _llm) = spawn_mock_llm().await;
        let chapter_path = root.join("books").join("b1").join("chapters").join("0002_云涌.md");
        let before = std::fs::read_to_string(&chapter_path).unwrap();

        let (status, parsed) = call(
            app50(rt50(&root, &llm_url)),
            "POST",
            "/api/v1/books/b1/chapters/2/workspace/inspiration",
            Some(r#"{ "brief": "不要增加新角色。" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["chapterNumber"], 2);
        // mock 未匹配到创作/审稿关键词 → 通用 "PASS" 卡片（非空即合法）。
        assert!(!parsed["card"].as_str().unwrap().is_empty());
        // 非变更性：章节文件原样。
        assert_eq!(std::fs::read_to_string(&chapter_path).unwrap(), before);

        // brief 非 string → 400；缺章 → 404；非法章节号 → 400。
        let (status, parsed) = call(
            app50(rt50(&root, &llm_url)),
            "POST",
            "/api/v1/books/b1/chapters/2/workspace/inspiration",
            Some(r#"{ "brief": 123 }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "A valid chapter number and optional brief string are required");
        let (status, _) = call(
            app50(rt50(&root, &llm_url)),
            "POST",
            "/api/v1/books/b1/chapters/9/workspace/inspiration",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(
            app50(rt50(&root, &llm_url)),
            "POST",
            "/api/v1/books/b1/chapters/abc/workspace/inspiration",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
