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
        // 预置归档（VERSION_ID）与新归档都含 _manual_：按排除预置精确定位
        // （read_dir 顺序在 APFS 上不稳定，裸 find 偶发拿错文件）。
        let manual = names
            .iter()
            .find(|n| n.contains("_manual_") && n.as_str() != format!("{VERSION_ID}.md"))
            .unwrap();
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

mod books51_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::{resync, rewrite, BooksRuntime};
    use inkos_engine::state::manager::StateManager;

    /// mock：修稿「修稿编辑」；pre 审计 1 warning、post（temp 0）3 critical
    /// （变差——strict 会拒，rewrite 的 Always 门应放行）；settler「结算」；
    /// validator「校验」PASS。
    async fn mock51_llm(
        _state: axum::extract::State<()>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::response::Response {
        let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
        let temperature = body["temperature"].as_f64();
        let content = if system.contains("修稿编辑") {
            "=== FIXED_ISSUES ===\n重排了冲突顺序\n\n=== REVISED_CONTENT ===\n林动睁开双眼，灵气顺经脉游走。他攥紧拳头——多年屈辱，今日起一笔一笔讨回。\n\n=== UPDATED_STATE ===\n| 字段 | 值 |\n|---|---|\n| 当前章节 | 1 |\n\n=== UPDATED_HOOKS ===\n| hook_id | 状态 |\n|---|---|\n| H01 | progressing |\n".to_string()
        } else if system.contains("审") || system.contains("连续") {
            if temperature == Some(0.0) {
                r#"{"passed": false, "overallScore": 40, "summary": "重写后仍恶化。", "issues": [{"severity": "critical", "category": "设定", "description": "矛盾甲", "suggestion": "查证。"}, {"severity": "critical", "category": "设定", "description": "矛盾乙", "suggestion": "查证。"}, {"severity": "critical", "category": "节奏", "description": "矛盾丙", "suggestion": "压缩。"}]}"#.to_string()
            } else {
                r#"{"passed": false, "overallScore": 70, "summary": "有一处问题。", "issues": [{"severity": "warning", "category": "节奏", "description": "略缓。", "suggestion": "压缩。"}]}"#.to_string()
            }
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

    async fn spawn_mock51() -> String {
        let app = axum::Router::new()
            .route("/chat/completions", axum::routing::post(mock51_llm))
            .with_state(());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn rt51(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    /// fixture_project + 第 1 章正文与索引。
    fn fixture51(root: &std::path::Path) {
        fixture_project(root);
        let book = root.join("books").join("b1");
        std::fs::write(
            book.join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
        )
        .unwrap();
        let now = "2026-01-01T00:00:00.000Z";
        std::fs::write(
            book.join("chapters").join("index.json"),
            serde_json::to_string(&vec![serde_json::json!({
                "number": 1, "title": "风起", "status": "ready-for-review",
                "wordCount": 16, "auditIssues": [], "lengthWarnings": [],
                "createdAt": now, "updatedAt": now,
            })])
            .unwrap(),
        )
        .unwrap();
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
    async fn rewrite_applies_under_always_gate_and_saves_brief() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture51(&root);
        let book = root.join("books").join("b1");
        let llm = spawn_mock51().await;
        let runtime = rt51(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();
        let app = axum::Router::new()
            .route("/api/v1/books/:id/rewrite/:chapter", axum::routing::post(rewrite))
            .with_state(runtime);

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/books/b1/rewrite/1",
            Some(r#"{ "brief": "保留事实，重做冲突顺序。" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["status"], "complete");
        assert_eq!(parsed["bookId"], "b1");
        assert_eq!(parsed["chapter"], 1);
        // Always 门：post 审计 3 critical（变差）仍 applied（strict 下会拒绝）。
        assert_eq!(parsed["result"]["applied"], true);
        assert_eq!(parsed["result"]["status"], "audit-failed");
        assert_eq!(parsed["result"]["revisionDiagnostics"], serde_json::Value::Null);

        // brief 落盘 user-brief；章节改写。
        assert_eq!(
            std::fs::read_to_string(book.join("story").join("runtime").join("chapter-0001.user-brief.md")).unwrap(),
            "保留事实，重做冲突顺序。\n"
        );
        let saved = std::fs::read_to_string(book.join("chapters").join("0001_风起.md")).unwrap();
        assert!(saved.starts_with("# 第1章 风起"));
        assert!(saved.contains("一笔一笔讨回"));

        // SSE：rewrite:start → rewrite:complete。
        assert_eq!(subscriber.recv().await.unwrap().event, "rewrite:start");
        assert_eq!(subscriber.recv().await.unwrap().event, "rewrite:complete");

        // 非法章节号 → 500 NaN 文案 + rewrite:error。
        let runtime2 = rt51(&root, &llm);
        let mut subscriber2 = runtime2.hub.subscribe();
        let (status, parsed) = call(
            axum::Router::new()
                .route("/api/v1/books/:id/rewrite/:chapter", axum::routing::post(rewrite))
                .with_state(runtime2),
            "POST",
            "/api/v1/books/b1/rewrite/abc",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"], "Chapter NaN not found in index");
        assert_eq!(subscriber2.recv().await.unwrap().event, "rewrite:error");
    }

    #[tokio::test]
    async fn resync_rebuilds_truth_and_flips_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture51(&root);
        let book = root.join("books").join("b1");
        // 人工编辑后的正文（无真相同步）。
        std::fs::write(
            book.join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动夺得玉符，踏入坊市。",
        )
        .unwrap();
        let llm = spawn_mock51().await;
        let runtime = rt51(&root, &llm);
        let app = axum::Router::new()
            .route("/api/v1/books/:id/resync/:chapter", axum::routing::post(resync))
            .with_state(runtime);

        let (status, parsed) = call(app, "POST", "/api/v1/books/b1/resync/1", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        // ChapterPipelineResult 的 resync 形状。
        assert_eq!(parsed["chapterNumber"], 1);
        assert_eq!(parsed["title"], "风起");
        assert_eq!(parsed["wordCount"], 16);
        assert_eq!(parsed["auditResult"]["passed"], true);
        assert_eq!(parsed["auditResult"]["issues"], serde_json::json!([]));
        assert_eq!(parsed["auditResult"]["summary"], "chapter truth/state resynced from edited body");
        assert_eq!(parsed["revised"], false);
        assert_eq!(parsed["status"], "ready-for-review");
        // 真相被结算输出更新（WRITER_RESPONSE 的 RUNTIME_STATE_DELTA 驱动）。
        assert!(book.join("story").join("current_state.md").exists());
        // 快照落盘。
        assert!(book.join("story").join("snapshots").join("1").join("current_state.md").exists());

        // 错误分支：NaN / 非最新 / 空索引（逐字文案）。
        let (status, parsed) = call(
            axum::Router::new()
                .route("/api/v1/books/:id/resync/:chapter", axum::routing::post(resync))
                .with_state(rt51(&root, &llm)),
            "POST",
            "/api/v1/books/b1/resync/abc",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"], "Chapter NaN not found in \"b1\".");
        let (status, parsed) = call(
            axum::Router::new()
                .route("/api/v1/books/:id/resync/:chapter", axum::routing::post(resync))
                .with_state(rt51(&root, &llm)),
            "POST",
            "/api/v1/books/b1/resync/9",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"], "Chapter 9 not found in \"b1\".");
    }

    #[tokio::test]
    async fn resync_rejects_non_latest_chapter() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture51(&root);
        let book = root.join("books").join("b1");
        std::fs::write(book.join("chapters").join("0002_云涌.md"), "# 第2章 云涌\n\n正文二。").unwrap();
        let index_path = book.join("chapters").join("index.json");
        let mut index: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
        index.push(serde_json::json!({
            "number": 2, "title": "云涌", "status": "ready-for-review",
            "wordCount": 4, "auditIssues": [], "lengthWarnings": [],
            "createdAt": "2026-01-01T00:00:00.000Z", "updatedAt": "2026-01-01T00:00:00.000Z",
        }));
        std::fs::write(&index_path, serde_json::to_string(&index).unwrap()).unwrap();
        let llm = spawn_mock51().await;

        let (status, parsed) = call(
            axum::Router::new()
                .route("/api/v1/books/:id/resync/:chapter", axum::routing::post(resync))
                .with_state(rt51(&root, &llm)),
            "POST",
            "/api/v1/books/b1/resync/1",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            parsed["error"],
            "Only the latest persisted chapter can be synced safely (latest is 2)."
        );

        // 空索引 → sync 版文案。
        std::fs::remove_file(&index_path).unwrap();
        std::fs::remove_file(book.join("chapters").join("0001_风起.md")).unwrap();
        std::fs::remove_file(book.join("chapters").join("0002_云涌.md")).unwrap();
        let (status, parsed) = call(
            axum::Router::new()
                .route("/api/v1/books/:id/resync/:chapter", axum::routing::post(resync))
                .with_state(rt51(&root, &llm)),
            "POST",
            "/api/v1/books/b1/resync/1",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"], "Book \"b1\" has no persisted chapters to sync.");
    }
}

mod books52_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::books_state_routes::{detect_all, detect_chapter, detect_stats};
    use inkos_engine::state::manager::StateManager;

    fn rt52(root: &std::path::Path) -> BooksRuntime {
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

    fn fixture52(root: &std::path::Path) {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story")).unwrap();
        // 三段等长文本触发段落等长 AI-tell（47 号同款手法）。
        let para = "林动睁开双眼，灵气顺着经脉游走，多年屈辱涌上心头。";
        let uniform = format!("{para}\n\n{para}\n\n{para}");
        std::fs::write(book.join("chapters").join("0001_风起.md"), format!("# 第1章\n\n{uniform}")).unwrap();
        std::fs::write(book.join("chapters").join("0002_云涌.md"), "# 第2章\n\n正文二。").unwrap();
        std::fs::write(book.join("chapters").join("0010_远行.md"), "# 第10章\n\n正文十。").unwrap();
        // 非 .md / 非 4 位前缀 → 排除。
        std::fs::write(book.join("chapters").join("index.json"), "[]").unwrap();
        std::fs::write(book.join("chapters").join("notes.md"), "无前缀").unwrap();
        std::fs::write(book.join("chapters").join("12_bad.md"), "两位前缀").unwrap();
    }

    fn app52(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/books/:id/detect-all", axum::routing::post(detect_all))
            .route("/api/v1/books/:id/detect/stats", axum::routing::get(detect_stats))
            .route("/api/v1/books/:id/detect/:chapter", axum::routing::post(detect_chapter))
            .with_state(runtime)
    }

    async fn call(app: axum::Router, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let request = axum::http::Request::builder().method(method).uri(uri).body(axum::body::Body::empty()).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) };
        (status, parsed)
    }

    #[tokio::test]
    async fn detect_all_scans_chapters_in_sorted_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture52(&root);

        let (status, parsed) = call(app52(rt52(&root)), "POST", "/api/v1/books/b1/detect-all").await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["bookId"], "b1");
        let results = parsed["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        // 字典序：0001 / 0002 / 0010（chapterNumber parseInt(前 4 位)）。
        let numbers: Vec<u32> = results.iter().map(|r| r["chapterNumber"].as_u64().unwrap() as u32).collect();
        assert_eq!(numbers, vec![1, 2, 10]);
        assert_eq!(results[0]["filename"], "0001_风起.md");
        // 等长三段 → 段落等长维度触发（issues 非空，severity warning/info）。
        let issues = results[0]["issues"].as_array().unwrap();
        assert!(!issues.is_empty(), "body: {parsed}");
        assert!(issues.iter().all(|i| i["severity"] == "warning" || i["severity"] == "info"));
        assert!(issues[0]["category"].as_str().is_some_and(|c| !c.is_empty()));
        // 常规短文本 → 无 issue。
        assert_eq!(results[1]["issues"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn detect_chapter_single_and_404() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture52(&root);

        let (status, parsed) = call(app52(rt52(&root)), "POST", "/api/v1/books/b1/detect/1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["chapterNumber"], 1);
        assert!(!parsed["issues"].as_array().unwrap().is_empty());

        let (status, parsed) = call(app52(rt52(&root)), "POST", "/api/v1/books/b1/detect/9").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"], "Chapter not found");
        // NaN → padded "NaN" 无匹配 → 404。
        let (status, _) = call(app52(rt52(&root)), "POST", "/api/v1/books/b1/detect/abc").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn detect_stats_aggregates_history_and_defaults_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture52(&root);
        let story = root.join("books").join("b1").join("story");

        // 缺失 → 空统计。
        let (status, parsed) = call(app52(rt52(&root)), "GET", "/api/v1/books/b1/detect/stats").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["totalDetections"], 0);
        assert_eq!(parsed["chapterBreakdown"].as_array().unwrap().len(), 0);

        // 两章五条：第 2 章首现 → breakdown 顺序 [2, 1]。
        std::fs::write(
            story.join("detection_history.json"),
            r#"[
              {"chapterNumber":2,"timestamp":"t1","provider":"gpt","score":0.9,"action":"detect","attempt":0},
              {"chapterNumber":1,"timestamp":"t2","provider":"gpt","score":0.8,"action":"detect","attempt":0},
              {"chapterNumber":2,"timestamp":"t3","provider":"gpt","score":0.5,"action":"rewrite","attempt":1},
              {"chapterNumber":2,"timestamp":"t4","provider":"gpt","score":0.4,"action":"rewrite","attempt":2},
              {"chapterNumber":1,"timestamp":"t5","provider":"gpt","score":0.75,"action":"rewrite","attempt":1}
            ]"#,
        )
        .unwrap();
        let (status, parsed) = call(app52(rt52(&root)), "GET", "/api/v1/books/b1/detect/stats").await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["totalDetections"], 2);
        assert_eq!(parsed["totalRewrites"], 3);
        assert_eq!(parsed["avgOriginalScore"], 0.85);
        assert_eq!(parsed["avgFinalScore"], 0.575);
        assert_eq!(parsed["avgScoreReduction"], 0.275);
        assert_eq!(parsed["passRate"], 1.0);
        let breakdown = parsed["chapterBreakdown"].as_array().unwrap();
        assert_eq!(breakdown.len(), 2);
        assert_eq!(breakdown[0]["chapterNumber"], 2);
        assert_eq!(breakdown[0]["originalScore"], 0.9);
        assert_eq!(breakdown[0]["finalScore"], 0.4);
        assert_eq!(breakdown[0]["rewriteAttempts"], 2);
        assert_eq!(breakdown[1]["chapterNumber"], 1);

        // 损坏 JSON → 空统计（TS catch 语义）。
        std::fs::write(story.join("detection_history.json"), "{oops").unwrap();
        let (status, parsed) = call(app52(rt52(&root)), "GET", "/api/v1/books/b1/detect/stats").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["totalDetections"], 0);
    }
}

mod books53_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::books_state_routes::{create_status, truth_file, write_truth_file};
    use inkos_engine::server::genre_routes::{
        copy_genre, create_genre, delete_genre, genre_detail, list_genres, update_genre,
    };
    use inkos_engine::state::manager::StateManager;

    fn rt53(root: &std::path::Path) -> BooksRuntime {
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

    fn fixture53(root: &std::path::Path) {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story").join("outline")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"测试书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(book.join("story").join("outline").join("story_frame.md"), "# 故事框架").unwrap();
        std::fs::write(book.join("story").join("outline").join("volume_map.md"), "# 卷册地图").unwrap();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\",\"高潮章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
    }

    fn app53(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/books/:id/truth/*file",
                axum::routing::get(truth_file).put(write_truth_file),
            )
            .route("/api/v1/books/:id/create-status", axum::routing::get(create_status))
            .route("/api/v1/genres", axum::routing::get(list_genres))
            .route("/api/v1/genres/create", axum::routing::post(create_genre))
            .route(
                "/api/v1/genres/:id",
                axum::routing::get(genre_detail).put(update_genre).delete(delete_genre),
            )
            .route("/api/v1/genres/:id/copy", axum::routing::post(copy_genre))
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
    async fn truth_write_roundtrip_and_guards() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture53(&root);

        // 白名单 outline 文件写入（父目录已存在场景）+ 读回。
        let (status, parsed) = call(
            app53(rt53(&root)),
            "PUT",
            "/api/v1/books/b1/truth/outline/volume_map.md",
            Some(r##"{ "content": "# 新卷册\n\n第一卷布局。" }"##),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        let (_, parsed) = call(
            app53(rt53(&root)),
            "GET",
            "/api/v1/books/b1/truth/outline/volume_map.md",
            None,
        )
        .await;
        assert!(parsed["content"].as_str().unwrap().contains("新卷册"));

        // roles 嵌套路径写入（自动建父目录）。
        let (status, _) = call(
            app53(rt53(&root)),
            "PUT",
            "/api/v1/books/b1/truth/roles/主要角色/林动.md",
            Some(r##"{ "content": "# 林动" }"##),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(root.join("books").join("b1").join("story").join("roles").join("主要角色").join("林动.md").exists());

        // 白名单外 → 400；runtime 诊断 → 400；新布局书 shim → 400。
        let (status, parsed) = call(
            app53(rt53(&root)),
            "PUT",
            "/api/v1/books/b1/truth/secret/evil.md",
            Some(r#"{ "content": "x" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "Invalid truth file");
        let (status, parsed) = call(
            app53(rt53(&root)),
            "PUT",
            "/api/v1/books/b1/truth/runtime/chapter-0001.plan.md",
            Some(r#"{ "content": "x" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "Runtime diagnostic files are read-only");
        let (status, parsed) = call(
            app53(rt53(&root)),
            "PUT",
            "/api/v1/books/b1/truth/story_bible.md",
            Some(r#"{ "content": "x" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "Legacy compat shim; edit outline/story_frame.md instead");

        // 无效 JSON / 缺 content → onError 形状 500。
        let (status, parsed) = call(app53(rt53(&root)), "PUT", "/api/v1/books/b1/truth/current_focus.md", Some("{oops")).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"]["code"], "INTERNAL_ERROR");
        let (status, _) = call(app53(rt53(&root)), "PUT", "/api/v1/books/b1/truth/current_focus.md", Some("{}")).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn create_status_ready_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture53(&root);
        let book = root.join("books").join("b1");

        // 五节缺二（book_rules/pending_hooks/roles）→ missing。
        let (status, parsed) = call(app53(rt53(&root)), "GET", "/api/v1/books/b1/create-status", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["status"], "missing");

        // 补齐五节 + 角色卡 → ready。
        std::fs::write(book.join("story").join("book_rules.md"), "规则").unwrap();
        std::fs::write(book.join("story").join("pending_hooks.md"), "| 伏笔 |").unwrap();
        std::fs::create_dir_all(book.join("story").join("roles").join("主要角色")).unwrap();
        std::fs::write(book.join("story").join("roles").join("主要角色").join("林动.md"), "# 林动").unwrap();
        let (status, parsed) = call(app53(rt53(&root)), "GET", "/api/v1/books/b1/create-status", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["status"], "ready");
    }

    #[tokio::test]
    async fn genres_crud_full_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture53(&root);

        // 列表：内置 xianxia，language 附着（frontmatter 无 language → 默认 zh）。
        let (status, parsed) = call(app53(rt53(&root)), "GET", "/api/v1/genres", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let genres = parsed["genres"].as_array().unwrap();
        assert_eq!(genres.len(), 1);
        assert_eq!(genres[0]["id"], "xianxia");
        assert_eq!(genres[0]["source"], "builtin");
        assert_eq!(genres[0]["language"], "zh");

        // 详情。
        let (status, parsed) = call(app53(rt53(&root)), "GET", "/api/v1/genres/xianxia", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["profile"]["name"], "仙侠");
        assert!(parsed["body"].as_str().unwrap().contains("正文指导"));

        // 创建：frontmatter 逐字断言。
        let (status, parsed) = call(
            app53(rt53(&root)),
            "POST",
            "/api/v1/genres/create",
            Some(r#"{ "id": "wuxia", "name": "武侠", "chapterTypes": ["成长章"], "numericalSystem": true, "auditDimensions": [1, 6], "body": "侠义指导" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["id"], "wuxia");
        let saved = std::fs::read_to_string(root.join("genres").join("wuxia.md")).unwrap();
        assert_eq!(
            saved,
            "---\nname: \"武侠\"\nid: \"wuxia\"\nlanguage: \"zh\"\nchapterTypes: [\"成长章\"]\nfatigueWords: []\nnumericalSystem: true\npowerScaling: false\neraResearch: false\npacingRule: \"\"\nsatisfactionTypes: []\nauditDimensions: [1,6]\n---\n\n侠义指导"
        );
        // 项目级覆盖同 id：列表 source=project。
        let (_, parsed) = call(app53(rt53(&root)), "GET", "/api/v1/genres", None).await;
        let wuxia = parsed["genres"].as_array().unwrap().iter().find(|g| g["id"] == "wuxia").unwrap();
        assert_eq!(wuxia["source"], "project");

        // 编辑：缺省字段回路径参数。
        let (status, _) = call(
            app53(rt53(&root)),
            "PUT",
            "/api/v1/genres/wuxia",
            Some(r#"{ "profile": { "name": "新武侠" }, "body": "改写指导" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let saved = std::fs::read_to_string(root.join("genres").join("wuxia.md")).unwrap();
        assert!(saved.contains("name: \"新武侠\""));
        assert!(saved.contains("id: \"wuxia\""));
        assert!(saved.ends_with("改写指导"));

        // 复制内置 → 项目。
        let (status, parsed) = call(app53(rt53(&root)), "POST", "/api/v1/genres/xianxia/copy", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["path"], "genres/xianxia.md");
        assert!(root.join("genres").join("xianxia.md").exists());

        // 删除 → 二次 404。
        let (status, _) = call(app53(rt53(&root)), "DELETE", "/api/v1/genres/wuxia", None).await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(app53(rt53(&root)), "DELETE", "/api/v1/genres/wuxia", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"], "Genre \"wuxia\" not found in project");

        // 校验分支：缺 name 400；unsafe id ApiError 形状。
        let (status, parsed) = call(
            app53(rt53(&root)),
            "POST",
            "/api/v1/genres/create",
            Some(r#"{ "id": "only-id" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "id and name are required");
        let (status, parsed) = call(
            app53(rt53(&root)),
            "POST",
            "/api/v1/genres/create",
            Some(r#"{ "id": "../evil", "name": "x" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_GENRE_ID");
        assert_eq!(parsed["error"]["message"], "Invalid genre ID: \"../evil\"");
    }
}

mod config54_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::project_config_routes::*;
    use inkos_engine::state::manager::StateManager;

    fn rt54(root: &std::path::Path) -> BooksRuntime {
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

    fn fixture54(root: &std::path::Path) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("inkos.json"), r#"{ "name": "demo" }"#).unwrap();
    }

    fn app54(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/project/input-governance-mode",
                axum::routing::get(get_input_governance_mode).put(put_input_governance_mode),
            )
            .route(
                "/api/v1/project/detection",
                axum::routing::get(get_detection).put(put_detection),
            )
            .route(
                "/api/v1/project/model-overrides",
                axum::routing::get(get_model_overrides).put(put_model_overrides),
            )
            .route(
                "/api/v1/project/default-model",
                axum::routing::get(get_default_model).put(put_default_model),
            )
            .route(
                "/api/v1/project/research-search",
                axum::routing::get(get_research_search).put(put_research_search),
            )
            .route(
                "/api/v1/project/chapter-review-mode",
                axum::routing::get(get_chapter_review_mode).put(put_chapter_review_mode),
            )
            .route("/api/v1/project/notify", axum::routing::get(get_notify).put(put_notify))
            .route("/api/v1/project/language", axum::routing::post(post_language))
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

    fn read_config(root: &std::path::Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(root.join("inkos.json")).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn governance_mode_and_review_mode_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture54(&root);

        // governance：无键 → v2；legacy 落盘；非法 400。
        let (status, parsed) = call(app54(rt54(&root)), "GET", "/api/v1/project/input-governance-mode", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["mode"], "v2");
        let (status, parsed) = call(app54(rt54(&root)), "PUT", "/api/v1/project/input-governance-mode", Some(r#"{ "mode": "legacy" }"#)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed, serde_json::json!({ "ok": true, "mode": "legacy" }));
        assert_eq!(read_config(&root)["inputGovernanceMode"], "legacy");
        let (status, parsed) = call(app54(rt54(&root)), "PUT", "/api/v1/project/input-governance-mode", Some(r#"{ "mode": "bad" }"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "mode must be legacy or v2");

        // review-mode：无键 auto；manual 落盘 writing.reviewMode；非法值归 auto。
        let (_, parsed) = call(app54(rt54(&root)), "GET", "/api/v1/project/chapter-review-mode", None).await;
        assert_eq!(parsed["mode"], "auto");
        let (status, parsed) = call(app54(rt54(&root)), "PUT", "/api/v1/project/chapter-review-mode", Some(r#"{ "mode": "manual" }"#)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed, serde_json::json!({ "ok": true, "mode": "manual" }));
        assert_eq!(read_config(&root)["writing"]["reviewMode"], "manual");
        let (_, parsed) = call(app54(rt54(&root)), "PUT", "/api/v1/project/chapter-review-mode", Some(r#"{ "mode": "garbage" }"#)).await;
        assert_eq!(parsed["mode"], "auto");
    }

    #[tokio::test]
    async fn detection_config_validation_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture54(&root);

        let (status, parsed) = call(app54(rt54(&root)), "GET", "/api/v1/project/detection", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["detection"], serde_json::Value::Null);

        // 合法：zod default 填充 7 字段标准化。
        let (status, parsed) = call(
            app54(rt54(&root)),
            "PUT",
            "/api/v1/project/detection",
            Some(r#"{ "detection": { "apiUrl": "https://api.gptzero.me", "apiKeyEnv": "GPTZERO_KEY" } }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["detection"]["provider"], "custom");
        assert_eq!(parsed["detection"]["threshold"], 0.5);
        assert_eq!(parsed["detection"]["enabled"], false);
        assert_eq!(parsed["detection"]["autoRewrite"], false);
        assert_eq!(parsed["detection"]["maxRetries"], 3);
        let saved = read_config(&root)["detection"].clone();
        assert_eq!(saved["apiKeyEnv"], "GPTZERO_KEY");

        // 非法：threshold 越界 400（错误拼接形态）。
        let (status, parsed) = call(
            app54(rt54(&root)),
            "PUT",
            "/api/v1/project/detection",
            Some(r#"{ "detection": { "apiUrl": "https://x.io", "apiKeyEnv": "K", "threshold": 2 } }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(parsed["error"].as_str().is_some_and(|e| e.contains("less than or equal to 1")), "{parsed}");

        // null 删键。
        let (status, parsed) = call(app54(rt54(&root)), "PUT", "/api/v1/project/detection", Some(r#"{ "detection": null }"#)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["detection"], serde_json::Value::Null);
        assert!(read_config(&root).get("detection").is_none());
    }

    #[tokio::test]
    async fn model_overrides_default_model_and_notify() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture54(&root);

        // model-overrides：缺省 {}；写入对象落盘。
        let (_, parsed) = call(app54(rt54(&root)), "GET", "/api/v1/project/model-overrides", None).await;
        assert_eq!(parsed["overrides"], serde_json::json!({}));
        let (status, _) = call(
            app54(rt54(&root)),
            "PUT",
            "/api/v1/project/model-overrides",
            Some(r#"{ "overrides": { "planner": { "model": "glm-5" } } }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(read_config(&root)["modelOverrides"]["planner"]["model"], "glm-5");

        // default-model：GET null/null；PUT 写入（sync 镜像暂缓——不写 llm.model）。
        let (status, parsed) = call(app54(rt54(&root)), "GET", "/api/v1/project/default-model", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["service"], serde_json::Value::Null);
        assert_eq!(parsed["defaultModel"], serde_json::Value::Null);
        let (status, parsed) = call(
            app54(rt54(&root)),
            "PUT",
            "/api/v1/project/default-model",
            Some(r#"{ "defaultModel": "glm-5-air", "service": "zhipu" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["defaultModel"], "glm-5-air");
        assert_eq!(parsed["service"], "zhipu");
        let llm = read_config(&root)["llm"].clone();
        assert_eq!(llm["defaultModel"], "glm-5-air");
        assert_eq!(llm["service"], "zhipu");
        // GET 回读：defaultModel 命中。
        let (_, parsed) = call(app54(rt54(&root)), "GET", "/api/v1/project/default-model", None).await;
        assert_eq!(parsed["defaultModel"], "glm-5-air");
        // 空 defaultModel → 400。
        let (status, parsed) = call(app54(rt54(&root)), "PUT", "/api/v1/project/default-model", Some(r#"{ "defaultModel": "  " }"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "defaultModel is required");

        // notify：缺省 []；写入数组。
        let (_, parsed) = call(app54(rt54(&root)), "GET", "/api/v1/project/notify", None).await;
        assert_eq!(parsed["channels"], serde_json::json!([]));
        let (status, _) = call(app54(rt54(&root)), "PUT", "/api/v1/project/notify", Some(r#"{ "channels": ["bark"] }"#)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(read_config(&root)["notify"], serde_json::json!(["bark"]));

        // language：透传落盘。
        let (status, parsed) = call(app54(rt54(&root)), "POST", "/api/v1/project/language", Some(r#"{ "language": "en" }"#)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed, serde_json::json!({ "ok": true, "language": "en" }));
        assert_eq!(read_config(&root)["language"], "en");
    }

    #[tokio::test]
    async fn research_search_defaults_and_validation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture54(&root);

        // GET 无键 → 整体默认。
        let (status, parsed) = call(app54(rt54(&root)), "GET", "/api/v1/project/research-search", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["researchSearch"], serde_json::json!({ "enabled": false, "provider": "tavily" }));

        // PUT 合法：填充 + 可选字段保留。
        let (status, parsed) = call(
            app54(rt54(&root)),
            "PUT",
            "/api/v1/project/research-search",
            Some(r#"{ "researchSearch": { "enabled": true, "provider": "custom", "baseUrl": "https://s.example.com", "apiKeyEnv": "TAVILY_KEY" } }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["researchSearch"]["enabled"], true);
        assert_eq!(parsed["researchSearch"]["provider"], "custom");
        assert_eq!(parsed["researchSearch"]["baseUrl"], "https://s.example.com");
        assert_eq!(parsed["researchSearch"]["apiKeyEnv"], "TAVILY_KEY");
        assert_eq!(read_config(&root)["researchSearch"]["provider"], "custom");

        // PUT 非法 provider → 500（zod 抛 → onError）。
        let (status, parsed) = call(
            app54(rt54(&root)),
            "PUT",
            "/api/v1/project/research-search",
            Some(r#"{ "researchSearch": { "provider": "google" } }"#),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"]["code"], "INTERNAL_ERROR");

        // inkos.json 缺失 → GET 500 onError 形状。
        std::fs::remove_file(root.join("inkos.json")).unwrap();
        let (status, parsed) = call(app54(rt54(&root)), "GET", "/api/v1/project/research-search", None).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"]["code"], "INTERNAL_ERROR");
    }
}

mod style55_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::style_routes::{fanfic_show, import_canon_endpoint, style_analyze, style_import};
    use inkos_engine::state::manager::StateManager;

    async fn mock55_llm(
        _state: axum::extract::State<()>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::response::Response {
        let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
        let content = if system.contains("网络小说架构师") {
            "# 正传正典\n\n## 世界规则\n斗气大陆。".to_string()
        } else {
            "## 叙事声音与语气\n冷峻克制，例句：少年握紧了拳。".to_string()
        };
        axum::response::IntoResponse::into_response((
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            sse_body(&content),
        ))
    }

    async fn spawn_mock55() -> String {
        let app = axum::Router::new()
            .route("/chat/completions", axum::routing::post(mock55_llm))
            .with_state(());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn rt55(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn fixture55(root: &std::path::Path) {
        for id in ["target", "parent"] {
            let book = root.join("books").join(id);
            std::fs::create_dir_all(book.join("chapters")).unwrap();
            std::fs::create_dir_all(book.join("story")).unwrap();
            std::fs::write(
                book.join("book.json"),
                format!(r#"{{"id":"{id}","title":"书{id}","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}}"#),
            )
            .unwrap();
        }
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
        // 父书真相（Phase 5 新布局）+ 一章正文（>500 字触发风格向导）。
        let parent = root.join("books").join("parent").join("story");
        std::fs::create_dir_all(parent.join("outline")).unwrap();
        std::fs::write(parent.join("outline").join("story_frame.md"), "# 世界框架\n斗气大陆。").unwrap();
        std::fs::write(parent.join("current_state.md"), "状态v1").unwrap();
        std::fs::write(root.join("books").join("parent").join("chapters").join("0001_启.md"), format!("# 第1章\n\n{}", "少年握紧了拳。".repeat(80))).unwrap();
    }

    fn app55(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/style/analyze", axum::routing::post(style_analyze))
            .route("/api/v1/books/:id/style/import", axum::routing::post(style_import))
            .route("/api/v1/books/:id/import/canon", axum::routing::post(import_canon_endpoint))
            .route("/api/v1/books/:id/fanfic", axum::routing::get(fanfic_show))
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
    async fn style_analyze_returns_profile_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture55(&root);

        let (status, parsed) = call(
            app55(rt55(&root, "http://127.0.0.1:9")),
            "POST",
            "/api/v1/style/analyze",
            Some(r#"{ "text": "林动握紧了拳。他抬起头，灵气涌动。多年屈辱涌上心头。", "sourceName": "斗破" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["sourceName"], "斗破");
        assert!(parsed["avgSentenceLength"].as_f64().unwrap() > 0.0);
        assert!(parsed["analyzedAt"].as_str().is_some_and(|s| s.ends_with('Z')));
        assert!(parsed.get("paragraphLengthRange").is_some());

        // text 空/缺 → 400；缺 sourceName → "unknown"。
        let (status, parsed) = call(app55(rt55(&root, "http://127.0.0.1:9")), "POST", "/api/v1/style/analyze", Some(r#"{ "text": "  " }"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "text is required");
        let (_, parsed) = call(app55(rt55(&root, "http://127.0.0.1:9")), "POST", "/api/v1/style/analyze", Some(r#"{ "text": "短句。" }"#)).await;
        assert_eq!(parsed["sourceName"], "unknown");
    }

    #[tokio::test]
    async fn style_import_short_sample_uses_fingerprint_guide() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture55(&root);
        let llm = spawn_mock55().await;
        let runtime = rt55(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();

        // 短样本（<500）→ 确定性指南（不调 LLM）。
        let (status, parsed) = call(
            app55(runtime),
            "POST",
            "/api/v1/books/target/style/import",
            Some(r#"{ "text": "林动握紧了拳。", "sourceName": "斗破" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        let guide = parsed["result"].as_str().unwrap();
        assert!(guide.contains("# 文风指南"));
        assert!(guide.contains("样本文本较短"));
        // 指纹 + 方法论落盘。
        let story = root.join("books").join("target").join("story");
        assert!(story.join("style_profile.json").exists());
        let saved = std::fs::read_to_string(story.join("style_guide.md")).unwrap();
        assert!(saved.contains("## 统计风格指纹"));
        assert_eq!(subscriber.recv().await.unwrap().event, "style:start");
        assert_eq!(subscriber.recv().await.unwrap().event, "style:complete");
    }

    #[tokio::test]
    async fn style_import_long_sample_uses_llm_guide() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture55(&root);
        let llm = spawn_mock55().await;

        let long_text = "少年握紧了拳，抬起头来。".repeat(60);
        let (status, parsed) = call(
            app55(rt55(&root, &llm)),
            "POST",
            "/api/v1/books/target/style/import",
            Some(&format!(r#"{{ "text": "{long_text}" }}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let guide = parsed["result"].as_str().unwrap();
        // LLM 定性输出（mock：叙事声音与语气）+ 方法论拼接。
        assert!(guide.contains("## 叙事声音与语气"));
        assert!(guide.contains("冷峻克制"));
    }

    #[tokio::test]
    async fn import_canon_generates_parent_canon_and_style_guide() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture55(&root);
        let llm = spawn_mock55().await;
        let runtime = rt55(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();

        let (status, parsed) = call(
            app55(runtime),
            "POST",
            "/api/v1/books/target/import/canon",
            Some(r#"{ "fromBookId": "parent" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);

        // parent_canon.md：LLM 输出 + 确定性 meta 块。
        let canon = std::fs::read_to_string(
            root.join("books").join("target").join("story").join("parent_canon.md"),
        )
        .unwrap();
        assert!(canon.contains("# 正传正典"));
        assert!(canon.contains("斗气大陆"));
        assert!(canon.contains("meta:"));
        assert!(canon.contains("parentBookId: \"parent\""));
        assert!(canon.contains("parentTitle: \"书parent\""));
        // 父书章节样本 ≥500 → 目标书也生成风格指纹。
        assert!(root.join("books").join("target").join("story").join("style_guide.md").exists());

        // SSE import:start（type canon）→ import:complete。
        let start = subscriber.recv().await.unwrap();
        assert_eq!(start.event, "import:start");
        assert!(start.data.contains("\"type\":\"canon\""));
        assert_eq!(subscriber.recv().await.unwrap().event, "import:complete");

        // 父书缺失 → 500 逐字文案（Available 列表）。
        let (status, parsed) = call(
            app55(rt55(&root, &llm)),
            "POST",
            "/api/v1/books/target/import/canon",
            Some(r#"{ "fromBookId": "ghost" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        let error = parsed["error"].as_str().unwrap();
        assert!(error.starts_with("Parent book \"ghost\" not found. Available: "), "{error}");
        assert!(error.contains("parent") && error.contains("target"), "{error}");
        // 缺 fromBookId → 400。
        let (status, parsed) = call(app55(rt55(&root, &llm)), "POST", "/api/v1/books/target/import/canon", Some("{}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "fromBookId is required");
    }

    #[tokio::test]
    async fn fanfic_show_reads_canon_or_null() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture55(&root);

        let (status, parsed) = call(app55(rt55(&root, "http://127.0.0.1:9")), "GET", "/api/v1/books/target/fanfic", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["bookId"], "target");
        assert_eq!(parsed["content"], serde_json::Value::Null);

        std::fs::write(
            root.join("books").join("target").join("story").join("fanfic_canon.md"),
            "# 番外正典",
        )
        .unwrap();
        let (_, parsed) = call(app55(rt55(&root, "http://127.0.0.1:9")), "GET", "/api/v1/books/target/fanfic", None).await;
        assert_eq!(parsed["content"], "# 番外正典");
    }
}

mod config56_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::project_config_routes::{get_project, put_project};
    use inkos_engine::state::manager::StateManager;

    fn rt56(root: &std::path::Path) -> BooksRuntime {
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

    fn fixture56(root: &std::path::Path) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(
            root.join("inkos.json"),
            r#"{ "name": "demo", "llm": { "provider": "custom", "baseUrl": "https://api.example.com", "model": "glm-5" } }"#,
        )
        .unwrap();
    }

    fn app56(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/project", axum::routing::get(get_project).put(put_project))
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
    async fn project_get_defaults_and_put_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture56(&root);

        // GET：schema 默认（language zh / temperature 0.7 / stream true）。
        let (status, parsed) = call(app56(rt56(&root)), "GET", "/api/v1/project", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["name"], "demo");
        assert_eq!(parsed["language"], "zh");
        assert_eq!(parsed["languageExplicit"], false);
        assert_eq!(parsed["model"], "glm-5");
        assert_eq!(parsed["provider"], "custom");
        assert_eq!(parsed["baseUrl"], "https://api.example.com");
        assert_eq!(parsed["temperature"], 0.7);
        assert_eq!(parsed["stream"], true);

        // PUT 合并（server.test.ts L792 同款场景）→ GET 回读。
        let (status, parsed) = call(
            app56(rt56(&root)),
            "PUT",
            "/api/v1/project",
            Some(r#"{ "language": "en", "temperature": 0.2, "stream": true }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        let (status, parsed) = call(app56(rt56(&root)), "GET", "/api/v1/project", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["language"], "en");
        assert_eq!(parsed["languageExplicit"], true);
        assert_eq!(parsed["temperature"], 0.2);
        assert_eq!(parsed["stream"], true);
        // name/model 不受影响。
        assert_eq!(parsed["name"], "demo");
        assert_eq!(parsed["model"], "glm-5");

        // 非法 language 不写入（精确 zh/en 命中才生效）。
        let (status, _) = call(app56(rt56(&root)), "PUT", "/api/v1/project", Some(r#"{ "language": "jp" }"#)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, parsed) = call(app56(rt56(&root)), "GET", "/api/v1/project", None).await;
        assert_eq!(parsed["language"], "en");
    }

    #[tokio::test]
    async fn project_get_invalid_config_is_structured_500() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture56(&root);

        // 损坏 JSON → PROJECT_CONFIG_INVALID（message 含 inkos.json）。
        std::fs::write(root.join("inkos.json"), "{ this is not valid json").unwrap();
        let (status, parsed) = call(app56(rt56(&root)), "GET", "/api/v1/project", None).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"]["code"], "PROJECT_CONFIG_INVALID");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("inkos.json"));

        // schema 校验失败（name 缺失）同样 500。
        std::fs::write(root.join("inkos.json"), r#"{ "llm": { "provider": "custom", "baseUrl": "https://x.io", "model": "m" } }"#).unwrap();
        let (status, parsed) = call(app56(rt56(&root)), "GET", "/api/v1/project", None).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"]["code"], "PROJECT_CONFIG_INVALID");
        // model 空串：63 号 env/services 合并层（fillNoopLLMDefaults）对齐 TS
        // 真实语义——resolveEffectiveLLMConfig 填 noop-model 后 schema 通过 → 200
        //（56 号时按 raw 直校验固化的 500 断言随之废弃）。
        std::fs::write(
            root.join("inkos.json"),
            r#"{ "name": "d", "llm": { "provider": "custom", "baseUrl": "https://x.io", "model": "" } }"#,
        )
        .unwrap();
        let (status, parsed) = call(app56(rt56(&root)), "GET", "/api/v1/project", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["model"], "noop-model");
    }

    #[tokio::test]
    async fn project_put_without_llm_object_is_flat_500() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(&root).unwrap();
        // 无 llm 键：temperature 给出 → TS existing.llm 解引用失败 → 平铺 500。
        std::fs::write(root.join("inkos.json"), r#"{ "name": "demo" }"#).unwrap();
        let (status, parsed) = call(
            app56(rt56(&root)),
            "PUT",
            "/api/v1/project",
            Some(r#"{ "temperature": 0.3 }"#),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(parsed["error"].is_string(), "body: {parsed}");

        // 仅 language（无 temperature/stream）→ 不解引用 llm → 200。
        let (status, _) = call(app56(rt56(&root)), "PUT", "/api/v1/project", Some(r#"{ "language": "en" }"#)).await;
        assert_eq!(status, StatusCode::OK);
    }
}

mod books58_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::book_create_routes::{create_book, import_chapters_endpoint};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::books_state_routes::create_status;
    use inkos_engine::state::manager::StateManager;

    pub(crate) const ARCHITECT_OUTPUT: &str = r#"=== SECTION: story_frame ===
## 主题与基调
少年于微末中抬起头。

=== SECTION: volume_map ===
### 第一卷（1-30章）觉醒
主角入宗门。

=== SECTION: roles ===
---ROLE---
tier: major
name: 林动
---CONTENT---
## 核心标签
坚韧、藏拙。

=== SECTION: book_rules ===
## 主角
- 名字：林动

=== SECTION: pending_hooks ===
| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H01 | 0 | 身世 | open | 0 | 第2卷 | 慢烧 | 无 | 第2卷中段 | true |  | 祖符来历 |
"#;

    pub(crate) const REVIEW_PASS: &str = "\
=== DIMENSION: 1 ===
分数：90
意见：冲突清晰。

=== DIMENSION: 2 ===
分数：88
意见：开篇有力。

=== DIMENSION: 3 ===
分数：85
意见：世界观内洽。

=== DIMENSION: 4 ===
分数：86
意见：角色区分明显。

=== DIMENSION: 5 ===
分数：84
意见：节奏可行。

=== OVERALL ===
总分：87
通过：是
总评：整体扎实。";

    const ANALYZER_OUTPUT: &str = "\
=== CHAPTER_TITLE ===
风起

=== CHAPTER_CONTENT ===
林动睁开双眼。

=== PRE_WRITE_CHECK ===

=== POST_SETTLEMENT ===

=== UPDATED_STATE ===
| Field | Value |
| --- | --- |
| Current Chapter | 1 |

=== UPDATED_LEDGER ===

=== UPDATED_HOOKS ===
| hook_id | start_chapter | type | status | last_advanced_chapter | expected_payoff | payoff_timing | notes |
| --- | --- | --- | --- | --- | --- | --- | --- |

=== CHAPTER_SUMMARY ===
| Chapter | Title | Characters | Key Events | State Changes | Hook Activity | Mood | Chapter Type |
| --- | --- | --- | --- | --- | --- | --- | --- |

=== UPDATED_SUBPLOTS ===

=== UPDATED_EMOTIONAL_ARCS ===

=== UPDATED_CHARACTER_MATRIX ===
## 林动
- **Role**: protagonist
";

    async fn mock58_llm(
        _state: axum::extract::State<()>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::response::Response {
        let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
        let content = if system.contains("总架构师") || system.contains("网络小说架构师") {
            ARCHITECT_OUTPUT.to_string()
        } else if system.contains("资深小说编辑") {
            REVIEW_PASS.to_string()
        } else if system.contains("连续性分析") || system.contains("continuity analyst") {
            ANALYZER_OUTPUT.to_string()
        } else {
            "PASS".to_string()
        };
        axum::response::IntoResponse::into_response((
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            sse_body(&content),
        ))
    }

    pub(crate) async fn spawn_mock58() -> String {
        let app = axum::Router::new()
            .route("/chat/completions", axum::routing::post(mock58_llm))
            .with_state(());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn rt58(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.to_string(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 8192,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    fn fixture58(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("books")).unwrap();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
    }

    fn app58(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/books/create", axum::routing::post(create_book))
            .route(
                "/api/v1/books/:id/import/chapters",
                axum::routing::post(import_chapters_endpoint),
            )
            .route("/api/v1/books/:id/create-status", axum::routing::get(create_status))
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

    async fn wait_for_book(root: &std::path::Path, book_id: &str) {
        for _ in 0..100 {
            if root.join("books").join(book_id).join("book.json").exists() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("book creation did not finish in time");
    }

    #[tokio::test]
    async fn create_book_staging_rename_and_sse_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture58(&root);
        let llm = spawn_mock58().await;
        let runtime = rt58(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();

        let (status, parsed) = call(
            app58(runtime),
            "POST",
            "/api/v1/books/create",
            Some(r#"{ "title": "斗破苍穹", "genre": "xianxia", "targetChapters": 50, "blurb": "废柴崛起" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["status"], "creating");
        assert_eq!(parsed["bookId"], "斗破苍穹");

        // SSE：book:creating → book:created。
        assert_eq!(subscriber.recv().await.unwrap().event, "book:creating");
        assert_eq!(subscriber.recv().await.unwrap().event, "book:created");

        wait_for_book(&root, "斗破苍穹").await;
        let book = root.join("books").join("斗破苍穹");
        // staging 原子落盘面：book.json + Phase 5 地基 + 控制文档 + 快照 0。
        assert!(book.join("book.json").exists());
        assert!(book.join("story").join("outline").join("story_frame.md").exists());
        assert!(book.join("story").join("roles").join("主要角色").join("林动.md").exists());
        assert!(book.join("story").join("pending_hooks.md").exists());
        assert!(book.join("story").join("author_intent.md").exists());
        assert!(book.join("story").join("snapshots").join("0").exists() || book.join("story").join("snapshots").exists());
        // brief.md：外部指令（blurb 段）落盘。
        let brief = std::fs::read_to_string(book.join("story").join("brief.md")).unwrap_or_default();
        assert!(brief.contains("废柴崛起"), "brief: {brief}");
        // 无残留 staging 目录。
        for entry in std::fs::read_dir(root.join("books")).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.starts_with(".tmp-book-create-"), "staging 残留: {name}");
        }
        // create-status：地基齐备 → ready（内存分支已清）。
        let (status, parsed) = call(app58(rt58(&root, &llm)), "GET", "/api/v1/books/斗破苍穹/create-status", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["status"], "ready");

        // 完整书已存在 → 409。
        let (status, parsed) = call(
            app58(rt58(&root, &llm)),
            "POST",
            "/api/v1/books/create",
            Some(r#"{ "title": "斗破苍穹", "genre": "xianxia" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(parsed["error"], "Book \"斗破苍穹\" already exists");

        // 空标题 → 400。
        let (status, parsed) = call(app58(rt58(&root, &llm)), "POST", "/api/v1/books/create", Some(r#"{ "title": "", "genre": "x" }"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "Could not derive a valid book id from title");
    }

    #[tokio::test]
    async fn import_chapters_replays_analyzer_and_builds_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture58(&root);
        // 预置目标书（Phase 5 地基由导入链重生成覆盖）。
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story").join("runtime")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"测试书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        // 旧运行时痕迹 → 回放重置。
        std::fs::write(book.join("story").join("chapter_summaries.md"), "旧摘要").unwrap();

        let llm = spawn_mock58().await;
        let runtime = rt58(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();

        let text = "# 第一章 风起\n\n林动睁开双眼，灵气涌动。\n\n# 第二章 云涌\n\n坊市喧闹。";
        let (status, parsed) = call(
            app58(runtime),
            "POST",
            "/api/v1/books/b1/import/chapters",
            Some(&format!(r#"{{ "text": {text:?} }}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["bookId"], "b1");
        assert_eq!(parsed["importedCount"], 2);
        assert_eq!(parsed["nextChapter"], 3);
        assert!(parsed["totalWords"].as_u64().unwrap() > 0);

        // SSE：import:start（type chapters）→ import:complete。
        let start = subscriber.recv().await.unwrap();
        assert_eq!(start.event, "import:start");
        assert!(start.data.contains("\"type\":\"chapters\""));
        assert_eq!(subscriber.recv().await.unwrap().event, "import:complete");

        // 章节文件落盘 + 索引 status=imported。
        assert!(book.join("chapters").join("0001_风起.md").exists());
        // 文件名 title 来自 analyzer 输出（mock 恒为"风起"——TS 同款：persisted
        // title 由 CHAPTER_TITLE 提取，非分章标题）。
        assert!(book.join("chapters").join("0002_风起.md").exists());
        let index: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(book.join("chapters").join("index.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(index.as_array().unwrap().len(), 2);
        assert_eq!(index[0]["status"], "imported");
        assert_eq!(index[0]["title"], "风起");
        // 回放重置：旧摘要清除、快照 0 与逐章快照存在。
        assert!(!book.join("story").join("chapter_summaries.md").exists()
            || std::fs::read_to_string(book.join("story").join("chapter_summaries.md")).unwrap().contains("风起"));
        assert!(book.join("story").join("snapshots").join("0").exists());
        assert!(book.join("story").join("snapshots").join("2").exists());
        // 地基重生成（fromImport 输出）。
        assert!(book.join("story").join("outline").join("story_frame.md").exists());

        // text 空 → 400。
        let (status, parsed) = call(app58(rt58(&root, &llm)), "POST", "/api/v1/books/b1/import/chapters", Some(r#"{ "text": "  " }"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "text is required");
    }
}

mod fanfic59_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::fanfic_routes::{fanfic_init, fanfic_refresh, imitation_init, spinoff_init};
    use inkos_engine::state::manager::StateManager;

    const ARCHITECT_OUTPUT: &str = r#"=== SECTION: story_frame ===
## 分岔点
三年之约后的空白期。

=== SECTION: volume_map ===
### 第一卷（1-20章）新程
独立冲突开启。

=== SECTION: roles ===
---ROLE---
tier: major
name: 萧炎
---CONTENT---
## 核心标签
骄傲、重情。

=== SECTION: book_rules ===
## 同人模式
- canon

=== SECTION: pending_hooks ===
| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H01 | 0 | 新谜 | open | 0 | 第2卷 | 慢烧 | 无 | 第2卷 | true |  | 空白期之谜 |
"#;

    const REVIEW_PASS: &str = "\
=== DIMENSION: 1 ===
分数：90
意见：好。

=== DIMENSION: 2 ===
分数：88
意见：好。

=== DIMENSION: 3 ===
分数：85
意见：好。

=== DIMENSION: 4 ===
分数：86
意见：好。

=== DIMENSION: 5 ===
分数：84
意见：好。

=== OVERALL ===
总分：87
通过：是
总评：通过。";

    const CANON_IMPORT: &str = "\
=== SECTION: world_rules ===
斗气大陆，等级森严。

=== SECTION: character_profiles ===
| 角色 | 身份 |
|---|---|
| 萧炎 | 主角 |

=== SECTION: key_events ===
| 序号 | 事件 |
|---|---|
| 1 | 三年之约 |

=== SECTION: power_system ===
斗气九段。

=== SECTION: writing_style ===
热血紧凑。";

    const PARENT_CANON: &str = "# 正传正典\n\n## 世界规则\n斗气大陆。";

    async fn mock59_llm(
        _state: axum::extract::State<()>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::response::Response {
        let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
        let content = if system.contains("同人创作素材分析师") {
            CANON_IMPORT.to_string()
        } else if system.contains("同人架构师") || system.contains("总架构师") || system.contains("网络小说架构师") {
            ARCHITECT_OUTPUT.to_string()
        } else if system.contains("资深小说编辑") {
            REVIEW_PASS.to_string()
        } else if system.contains("网络小说架构师") || system.contains("parent-canon") || system.contains("正传正典参照") {
            PARENT_CANON.to_string()
        } else if system.contains("文学风格分析专家") || system.contains("literary style analyst") {
            "## 叙事声音\n热血紧凑。".to_string()
        } else {
            "# 正传正典\n\n## 世界规则\n斗气大陆。".to_string()
        };
        axum::response::IntoResponse::into_response((
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            sse_body(&content),
        ))
    }

    async fn spawn_mock59() -> String {
        let app = axum::Router::new()
            .route("/chat/completions", axum::routing::post(mock59_llm))
            .with_state(());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn rt59(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.to_string(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 8192,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    fn fixture59(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("books").join("parent").join("chapters")).unwrap();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("books").join("parent").join("book.json"),
            r#"{"id":"parent","title":"斗破正传","platform":"other","genre":"xianxia","status":"active","targetChapters":80,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
    }

    fn app59(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/fanfic/init", axum::routing::post(fanfic_init))
            .route("/api/v1/books/:id/fanfic/refresh", axum::routing::post(fanfic_refresh))
            .route("/api/v1/spinoff/init", axum::routing::post(spinoff_init))
            .route("/api/v1/imitation/init", axum::routing::post(imitation_init))
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

    async fn wait_for(path: std::path::PathBuf) {
        for _ in 0..150 {
            if path.exists() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("path not ready in time: {path:?}");
    }

    #[tokio::test]
    async fn fanfic_init_builds_canon_and_foundation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture59(&root);
        let llm = spawn_mock59().await;
        let runtime = rt59(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();

        let source_text = "斗气大陆，萧炎三年之约。".repeat(60);
        let (status, parsed) = call(
            app59(runtime),
            "POST",
            "/api/v1/fanfic/init",
            Some(&format!(r#"{{ "title": "斗破新程", "genre": "xianxia", "sourceText": "{source_text}", "mode": "canon", "sourceName": "斗破苍穹" }}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["bookId"], "斗破新程");

        // fanfic_canon.md：五段提取 + meta。
        let book = root.join("books").join("斗破新程");
        let canon = std::fs::read_to_string(book.join("story").join("fanfic_canon.md")).unwrap();
        assert!(canon.contains("# 同人正典（《斗破苍穹》）"));
        assert!(canon.contains("斗气大陆，等级森严。"));
        assert!(canon.contains("fanficMode: \"canon\""));
        // 地基（fanfic 架构师输出）+ 角色卡 + 快照 0 + 空索引。
        assert!(book.join("story").join("outline").join("story_frame.md").exists());
        assert!(book.join("story").join("roles").join("主要角色").join("萧炎.md").exists());
        assert!(book.join("story").join("snapshots").join("0").exists() || book.join("story").join("snapshots").exists());
        assert!(book.join("chapters").join("index.json").exists());
        // 风格向导（sourceText ≥500）。
        assert!(book.join("story").join("style_guide.md").exists());

        assert_eq!(subscriber.recv().await.unwrap().event, "fanfic:start");
        assert_eq!(subscriber.recv().await.unwrap().event, "fanfic:complete");

        // 缺 sourceText → 400。
        let (status, parsed) = call(app59(rt59(&root, &llm)), "POST", "/api/v1/fanfic/init", Some(r#"{ "title": "x" }"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "title and sourceText are required");

        // refresh：重导 canon。
        let (status, parsed) = call(
            app59(rt59(&root, &llm)),
            "POST",
            "/api/v1/books/斗破新程/fanfic/refresh",
            Some(r#"{ "sourceText": "新的原作素材补充。", "sourceName": "斗破苍穹" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        let (status, _) = call(app59(rt59(&root, &llm)), "POST", "/api/v1/books/斗破新程/fanfic/refresh", Some(r#"{ "sourceText": "  " }"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn spinoff_and_imitation_init_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture59(&root);
        let llm = spawn_mock59().await;

        // spinoff：后台创建（creating 响应 + spinoff:* + book:created）。
        let runtime = rt59(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();
        let (status, parsed) = call(
            app59(runtime),
            "POST",
            "/api/v1/spinoff/init",
            Some(r#"{ "title": "药老前传", "parentBookId": "parent", "direction": "药老的早年" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["status"], "creating");
        assert_eq!(parsed["bookId"], "药老前传");

        let book = root.join("books").join("药老前传");
        wait_for(book.join("story").join("parent_canon.md")).await;
        wait_for(book.join("chapters").join("index.json")).await;
        // 正传正典 + 地基（original 模式 + spinoff 上下文）。
        let parent_canon = std::fs::read_to_string(book.join("story").join("parent_canon.md")).unwrap();
        assert!(parent_canon.contains("meta:"));
        assert!(book.join("story").join("outline").join("story_frame.md").exists());
        assert_eq!(subscriber.recv().await.unwrap().event, "spinoff:start");
        // spinoff:complete → book:created 顺序到达。
        let mut events = Vec::new();
        for _ in 0..2 {
            events.push(subscriber.recv().await.unwrap().event);
        }
        assert!(events.contains(&"spinoff:complete".to_string()));
        assert!(events.contains(&"book:created".to_string()));

        // parent 缺失 → 404；缺 parentBookId → 400。
        let (status, parsed) = call(app59(rt59(&root, &llm)), "POST", "/api/v1/spinoff/init", Some(r#"{ "title": "x", "parentBookId": "ghost" }"#)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"], "Parent book \"ghost\" not found");
        let (status, parsed) = call(app59(rt59(&root, &llm)), "POST", "/api/v1/spinoff/init", Some(r#"{ "title": "x" }"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "title and parentBookId are required");

        // imitation：initBook + 强制风格向导。
        let runtime2 = rt59(&root, &llm);
        let mut subscriber2 = runtime2.hub.subscribe();
        let reference = "少年握紧了拳，抬起头来。".repeat(60);
        let (status, parsed) = call(
            app59(runtime2),
            "POST",
            "/api/v1/imitation/init",
            Some(&format!(r#"{{ "title": "仿写书", "genre": "xianxia", "referenceText": "{reference}", "storyIdea": "废柴逆袭，一雪前耻" }}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["bookId"], "仿写书");
        let imbook = root.join("books").join("仿写书");
        wait_for(imbook.join("story").join("style_guide.md")).await;
        wait_for(imbook.join("story").join("outline").join("story_frame.md")).await;
        // storyIdea 作为外部指令 → brief.md。
        let brief = std::fs::read_to_string(imbook.join("story").join("brief.md")).unwrap();
        assert!(brief.contains("废柴逆袭"));
        assert_eq!(subscriber2.recv().await.unwrap().event, "imitation:start");
        // 缺 storyIdea → 400。
        let (status, parsed) = call(app59(rt59(&root, &llm)), "POST", "/api/v1/imitation/init", Some(r#"{ "title": "y", "referenceText": "z" }"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "title, referenceText and storyIdea are required");
    }
}

// ── 60 号：skills / prompt-packs 轻域（server.ts L4209-L4268）─────────

mod skills60_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::skill_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt60(root: &std::path::Path) -> BooksRuntime {
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

    fn app60(root: &std::path::Path) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/skills", axum::routing::get(skill_routes::list_skills))
            .route("/api/v1/skills/import", axum::routing::post(skill_routes::import_skill))
            .route("/api/v1/skills/:skillId", axum::routing::delete(skill_routes::delete_skill))
            .route("/api/v1/prompt-packs", axum::routing::get(skill_routes::list_prompt_packs))
            .route(
                "/api/v1/prompt-packs/:promptId",
                axum::routing::put(skill_routes::put_prompt_pack)
                    .delete(skill_routes::delete_prompt_pack),
            )
            .with_state(rt60(root))
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

    /// 标准 base64 编码（测试 fixture 构造 dataUrl 用）。
    fn b64(input: &str) -> String {
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes = input.as_bytes();
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
            out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
        }
        out
    }

    fn data_url(content: &str) -> String {
        format!("data:text/markdown;base64,{}", b64(content))
    }

    const MANIFEST: &str = "---\nname: Combat Tactics\ndescription: 战斗策略技能\n---\n\n# 战斗策略\n\n正文指导。";

    fn import_body(files: &[(&str, &str)]) -> String {
        let entries: Vec<String> = files
            .iter()
            .map(|(path, content)| {
                serde_json::json!({ "path": path, "dataUrl": data_url(content) }).to_string()
            })
            .collect();
        format!("{{\"files\":[{}]}}", entries.join(","))
    }

    #[tokio::test]
    async fn get_prompt_packs_lists_builtin_packs_and_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        let (status, parsed) = call(app60(&root), "GET", "/api/v1/prompt-packs", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let packs = parsed["packs"].as_array().unwrap();
        assert_eq!(packs.len(), 3);
        assert_eq!(packs[0]["id"], "longform");
        assert_eq!(packs[0]["title"], "Longform Writing");
        assert_eq!(packs[0]["source"], "builtin");
        let prompts = parsed["prompts"].as_array().unwrap();
        assert_eq!(prompts.len(), 12);
        let writer = prompts.iter().find(|p| p["id"] == "longform.writer").unwrap();
        assert_eq!(writer["packId"], "longform");
        assert_eq!(writer["title"], "Longform Writer");
        assert_eq!(writer["source"], "builtin");
        assert_eq!(writer["overridden"], false);
        // builtin 态 content == defaultContent，且无 path 键（undefined 不序列化）
        assert_eq!(writer["content"], writer["defaultContent"]);
        assert!(writer.get("path").is_none());
    }

    #[tokio::test]
    async fn put_prompt_pack_overrides_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // 大小写路径命中（normalizeStudioPromptId trim+lower）
        let (status, parsed) = call(
            app60(&root),
            "PUT",
            "/api/v1/prompt-packs/LONGFORM.Writer",
            Some(r##"{ "content": "# 覆盖后的写作提示\n\n第二段。" }"##),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["prompt"]["id"], "longform.writer");
        assert_eq!(parsed["prompt"]["source"], "project");
        assert_eq!(parsed["prompt"]["overridden"], true);
        assert_eq!(parsed["prompt"]["path"], "prompt/longform/writer.md");
        assert!(parsed["prompt"]["content"].as_str().unwrap().contains("覆盖后的写作提示"));
        // defaultContent 保留 builtin 原文
        assert_ne!(parsed["prompt"]["content"], parsed["prompt"]["defaultContent"]);

        // 覆盖文件落盘（utf-8 原文）
        let on_disk = std::fs::read_to_string(root.join("prompt").join("longform").join("writer.md")).unwrap();
        assert!(on_disk.contains("覆盖后的写作提示"));

        // GET 列表反映覆盖态
        let (_, parsed) = call(app60(&root), "GET", "/api/v1/prompt-packs", None).await;
        let writer = parsed["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "longform.writer")
            .unwrap()
            .clone();
        assert_eq!(writer["source"], "project");
        assert_eq!(writer["overridden"], true);

        // DELETE 清除覆盖 → 回 builtin
        let (status, parsed) = call(
            app60(&root),
            "DELETE",
            "/api/v1/prompt-packs/longform.writer",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["prompt"]["source"], "builtin");
        assert_eq!(parsed["prompt"]["overridden"], false);
        assert!(parsed["prompt"].get("path").is_none());
        assert!(!root.join("prompt").join("longform").join("writer.md").exists());
    }

    #[tokio::test]
    async fn put_prompt_pack_invalid_branches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // 未知 id → 404（消息带原值）
        let (status, parsed) = call(
            app60(&root),
            "PUT",
            "/api/v1/prompt-packs/unknown.prompt",
            Some(r#"{"content":"x"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"]["code"], "PROMPT_PACK_PROMPT_NOT_FOUND");
        assert_eq!(parsed["error"]["message"], "Prompt pack prompt not found: unknown.prompt");

        // 非 JSON body → 400
        let (status, parsed) = call(
            app60(&root),
            "PUT",
            "/api/v1/prompt-packs/longform.writer",
            Some("not-json"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_PROMPT_PACK_PAYLOAD");
        assert_eq!(parsed["error"]["message"], "Prompt pack payload must be JSON");

        // content 非串 → 400
        let (status, parsed) = call(
            app60(&root),
            "PUT",
            "/api/v1/prompt-packs/longform.writer",
            Some(r#"{"content":123}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "content must be a string");

        // JSON 数组（无 content 键）→ 400
        let (status, parsed) = call(
            app60(&root),
            "PUT",
            "/api/v1/prompt-packs/longform.writer",
            Some(r#"[1,2]"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "content must be a string");

        // DELETE 未知 id → 404
        let (status, parsed) = call(
            app60(&root),
            "DELETE",
            "/api/v1/prompt-packs/nope.nope",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"]["code"], "PROMPT_PACK_PROMPT_NOT_FOUND");
    }

    #[tokio::test]
    async fn import_skill_roundtrip_lists_and_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // folder 形式：folder 前缀剥离后落盘 .agents/skills/{id}/
        let body = import_body(&[
            ("my-pack/SKILL.md", MANIFEST),
            ("my-pack/reference.md", "参考资料。"),
        ]);
        let (status, parsed) = call(app60(&root), "POST", "/api/v1/skills/import", Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["skill"]["id"], "combat-tactics");
        assert_eq!(parsed["skill"]["name"], "Combat Tactics");
        assert_eq!(parsed["skill"]["source"], "project");
        assert_eq!(parsed["skill"]["editable"], true);
        assert_eq!(parsed["skill"]["path"], ".agents/skills/combat-tactics/SKILL.md");
        assert!(parsed["skill"]["body"].as_str().unwrap().contains("战斗策略"));

        let skill_dir = root.join(".agents").join("skills").join("combat-tactics");
        assert!(skill_dir.join("SKILL.md").exists());
        assert!(skill_dir.join("reference.md").exists());
        assert!(!root.join(".agents").join("skills").join("my-pack").exists());

        // GET /skills：注册表归并（id 排序）+ project 标记
        let (status, parsed) = call(app60(&root), "GET", "/api/v1/skills", None).await;
        assert_eq!(status, StatusCode::OK);
        let skills = parsed["skills"].as_array().unwrap();
        let combat = skills
            .iter()
            .find(|s| s["id"] == "combat-tactics")
            .unwrap()
            .clone();
        assert_eq!(combat["source"], "project");
        assert_eq!(combat["editable"], true);
        // 按 id 排序（createSkillRegistry 语义）
        let ids: Vec<&str> = skills.iter().map(|s| s["id"].as_str().unwrap()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);

        // 重复导入 → 409 SKILL_EXISTS
        let (status, parsed) = call(app60(&root), "POST", "/api/v1/skills/import", Some(&body)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(parsed["error"]["code"], "SKILL_EXISTS");
        assert_eq!(parsed["error"]["message"], "Project skill already exists: combat-tactics");

        // 删除 → ok:true → 目录消失 → 再删 404
        let (status, parsed) = call(
            app60(&root),
            "DELETE",
            "/api/v1/skills/combat-tactics",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert!(!skill_dir.exists());
        let (status, parsed) = call(
            app60(&root),
            "DELETE",
            "/api/v1/skills/combat-tactics",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"]["code"], "SKILL_NOT_FOUND");
        assert_eq!(parsed["error"]["message"], "Project skill not found: combat-tactics");

        // 删除后可再次导入
        let (status, _) = call(app60(&root), "POST", "/api/v1/skills/import", Some(&body)).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn import_skill_root_manifest_form() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // 根级 SKILL.md 形式（skillPath = root/SKILL.md → id 回退 root basename，但 name 可用）
        let body = import_body(&[("SKILL.md", MANIFEST)]);
        let (status, parsed) = call(app60(&root), "POST", "/api/v1/skills/import", Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["skill"]["id"], "combat-tactics");
        assert!(root.join(".agents").join("skills").join("combat-tactics").join("SKILL.md").exists());
    }

    #[tokio::test]
    async fn import_skill_validation_branches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let post = |body: &str| {
            let root = root.clone();
            let body = body.to_string();
            async move {
                call(app60(&root), "POST", "/api/v1/skills/import", Some(&body)).await
            }
        };

        // 非 JSON → 400
        let (status, parsed) = post("not-json").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_SKILL_IMPORT");
        assert_eq!(parsed["error"]["message"], "Skill import payload must be JSON");

        // payload 非对象 → 400
        let (status, parsed) = post("[1]").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "Skill import payload must be an object");

        // files 缺失 / 空数组 → 400
        let (status, parsed) = post("{}").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "Skill import requires at least one file");
        let (status, parsed) = post("{\"files\":[]}").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "Skill import requires at least one file");

        // 不安全路径（遍历）→ 400 INVALID_SKILL_IMPORT_PATH（消息带原值）
        let body = import_body(&[("../evil.md", "x")]);
        let (status, parsed) = post(&body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_SKILL_IMPORT_PATH");
        assert_eq!(parsed["error"]["message"], "Unsafe skill import path: ../evil.md");

        // 绝对路径 → 400
        let body = import_body(&[("/etc/passwd", "x")]);
        let (status, parsed) = post(&body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_SKILL_IMPORT_PATH");

        // 重复 path（大小写不敏感键）→ 400
        let body = import_body(&[("SKILL.md", MANIFEST), ("skill.md", MANIFEST)]);
        let (status, parsed) = post(&body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "Duplicate skill import path: skill.md");

        // 缺 dataUrl → 400
        let (status, parsed) = post(r#"{"files":[{"path":"SKILL.md"}]}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "Missing dataUrl for SKILL.md");

        // 坏 dataUrl → 400 INVALID_ATTACHMENT_DATA_URL
        let (status, parsed) = post(r#"{"files":[{"path":"SKILL.md","dataUrl":"http://x/y"}]}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_ATTACHMENT_DATA_URL");

        // 无 SKILL.md → 400
        let body = import_body(&[("notes/other.md", MANIFEST)]);
        let (status, parsed) = post(&body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "Skill import must contain exactly one SKILL.md");

        // 两个 SKILL.md → 400
        let body = import_body(&[("a/SKILL.md", MANIFEST), ("b/SKILL.md", MANIFEST)]);
        let (status, parsed) = post(&body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "Skill import must contain exactly one SKILL.md");

        // 越出 manifest folder → 400 INVALID_SKILL_IMPORT_PATH
        let body = import_body(&[("a/SKILL.md", MANIFEST), ("outside.md", "x")]);
        let (status, parsed) = post(&body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_SKILL_IMPORT_PATH");
        assert_eq!(parsed["error"]["message"], "All imported files must be inside the SKILL.md folder");

        // manifest 解析失败 → 400 INVALID_SKILL_MANIFEST
        let body = import_body(&[("SKILL.md", "no frontmatter")]);
        let (status, parsed) = post(&body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_SKILL_MANIFEST");
        assert_eq!(
            parsed["error"]["message"],
            "SKILL.md must start with YAML frontmatter delimiters."
        );

        // 所有失败分支都不留 staging 残留
        let skills_dir = root.join(".agents").join("skills");
        if skills_dir.exists() {
            let leftovers: Vec<_> = std::fs::read_dir(&skills_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with(".import-"))
                .collect();
            assert!(leftovers.is_empty(), "staging 残留: {leftovers:?}");
        }
    }

    #[tokio::test]
    async fn delete_skill_rejects_invalid_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // 下划线不合规（^[a-zA-Z][a-zA-Z0-9-]*$）→ 400 INVALID_SKILL_ID
        let (status, parsed) = call(app60(&root), "DELETE", "/api/v1/skills/bad_id", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_SKILL_ID");
        assert_eq!(
            parsed["error"]["message"],
            "Invalid skillId: Skill id must use letters, numbers, and hyphens."
        );
    }

    #[tokio::test]
    async fn get_skills_reports_diagnostics_for_broken_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 项目 skills 目录下放一个坏 manifest → 进 diagnostics 而非失败
        std::fs::create_dir_all(root.join("skills").join("broken")).unwrap();
        std::fs::write(root.join("skills").join("broken").join("SKILL.md"), "bad").unwrap();

        let (status, parsed) = call(app60(&root), "GET", "/api/v1/skills", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let diagnostics = parsed["diagnostics"].as_array().unwrap();
        assert!(diagnostics
            .iter()
            .any(|d| d["path"].as_str().unwrap().contains(&root.join("skills").join("broken").join("SKILL.md").to_string_lossy().to_string())
                && d["message"].as_str().unwrap().contains("frontmatter")),
            "diagnostics: {diagnostics:?}");
        // 坏 manifest 不进 skills 列表（home 目录可能带本机技能，故不断言总数）
        let skills = parsed["skills"].as_array().unwrap();
        assert!(!skills.iter().any(|s| s["id"] == "broken"));
    }
}

// ── 61 号：project 文件浏览面（server.ts L4270-L4320）──────────────

mod projectfiles61_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::project_files_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt61(root: &std::path::Path) -> BooksRuntime {
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

    fn app61(root: &std::path::Path) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/project/files/*file",
                axum::routing::get(project_files_routes::get_project_file),
            )
            .route(
                "/api/v1/project/artifacts/*file",
                axum::routing::get(project_files_routes::get_project_artifact)
                    .put(project_files_routes::put_project_artifact),
            )
            .with_state(rt61(root))
    }

    async fn call(app: axum::Router, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, Option<Vec<u8>>, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request = builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap().to_vec();
        let parsed = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, Some(bytes), parsed)
    }

    /// PNG 魔数开头的最小 fixture。
    const PNG_BYTES: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

    #[tokio::test]
    async fn get_project_file_serves_image_with_no_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("shorts").join("ep1")).unwrap();
        std::fs::write(root.join("shorts").join("ep1").join("cover.png"), PNG_BYTES).unwrap();

        let (status, bytes, _) = call(
            app61(&root),
            "GET",
            "/api/v1/project/files/shorts/ep1/cover.png",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes.as_deref(), Some(PNG_BYTES));

        // percent-encoded 段（decodeURIComponent 语义）
        std::fs::write(root.join("shorts").join("sp ace.png"), PNG_BYTES).unwrap();
        let (status, bytes, _) = call(
            app61(&root),
            "GET",
            "/api/v1/project/files/shorts/sp%20ace.png",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "percent 解码后命中");
        assert_eq!(bytes.as_deref(), Some(PNG_BYTES));

        // 头部多余 / 剥离
        let (status, _, _) = call(
            app61(&root),
            "GET",
            "/api/v1/project/files//shorts/ep1/cover.png",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn get_project_file_validation_branches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let get = |uri: &str| {
            let root = root.clone();
            let uri = uri.to_string();
            async move { call(app61(&root), "GET", &uri, None).await }
        };

        // 越权前缀（books/ 不在白名单）
        let (status, _, parsed) = get("/api/v1/project/files/books/b1/cover.png").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_PROJECT_FILE_PATH");
        assert_eq!(
            parsed["error"]["message"],
            "Only generated shorts/, covers/, interactive-films/ images can be previewed"
        );

        // .. 穿越（含反斜杠形态）
        let (status, _, parsed) = get("/api/v1/project/files/shorts/..%2F..%2Fetc%2Fx.png").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "Invalid project file path");
        let (status, _, parsed) = get("/api/v1/project/files/shorts/..\\..\\x.png").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "Invalid project file path");

        // 非图片扩展 → 415
        let (status, _, parsed) = get("/api/v1/project/files/shorts/a.txt").await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(parsed["error"]["code"], "UNSUPPORTED_PROJECT_FILE_TYPE");

        // 非法 percent 序列（decodeURIComponent throw）→ 400
        let (status, _, parsed) = get("/api/v1/project/files/shorts/%zz.png").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_PROJECT_FILE_PATH");

        // 文件缺失 → 404
        let (status, _, _) = get("/api/v1/project/files/covers/missing.png").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn artifacts_get_put_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("dramas")).unwrap();
        std::fs::write(root.join("dramas").join("ep1.md"), "# 第一幕\n\n正文。").unwrap();

        // GET md：path/content/contentType/size（UTF-8 字节数）
        let (status, _, parsed) = call(
            app61(&root),
            "GET",
            "/api/v1/project/artifacts/dramas/ep1.md",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["path"], "dramas/ep1.md");
        assert_eq!(parsed["contentType"], "text/markdown; charset=utf-8");
        assert_eq!(parsed["content"], "# 第一幕\n\n正文。");
        assert_eq!(parsed["size"], "# 第一幕\n\n正文。".len());

        // PUT 深层新文件（mkdir -p 父目录）+ 覆写
        let (status, _, parsed) = call(
            app61(&root),
            "PUT",
            "/api/v1/project/artifacts/storyboards/scenes/s1.json",
            Some(r#"{"content":"{ \"shot\": 1 }"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["path"], "storyboards/scenes/s1.json");
        assert_eq!(parsed["contentType"], "application/json; charset=utf-8");
        assert_eq!(parsed["size"], "{ \"shot\": 1 }".len());
        assert_eq!(
            std::fs::read_to_string(root.join("storyboards").join("scenes").join("s1.json")).unwrap(),
            "{ \"shot\": 1 }"
        );

        // GET 回读（json 类型）
        let (status, _, parsed) = call(
            app61(&root),
            "GET",
            "/api/v1/project/artifacts/storyboards/scenes/s1.json",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["content"], "{ \"shot\": 1 }");

        // 缺失 → 404
        let (status, _, _) = call(
            app61(&root),
            "GET",
            "/api/v1/project/artifacts/dramas/nope.md",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn artifacts_validation_branches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // 越权前缀 → 400 INVALID_PROJECT_ARTIFACT_PATH
        let (status, _, parsed) = call(
            app61(&root),
            "GET",
            "/api/v1/project/artifacts/story/pending_hooks.md",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_PROJECT_ARTIFACT_PATH");
        assert_eq!(parsed["error"]["message"], "Only generated writing artifacts can be opened");

        // 非文本扩展 → 415
        let (status, _, parsed) = call(
            app61(&root),
            "GET",
            "/api/v1/project/artifacts/shorts/a.png",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(parsed["error"]["code"], "UNSUPPORTED_PROJECT_ARTIFACT_TYPE");

        // .. 穿越 → 400（消息为 artifact 版）
        let (status, _, parsed) = call(
            app61(&root),
            "GET",
            "/api/v1/project/artifacts/shorts/..%2Fx.md",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_PROJECT_ARTIFACT_PATH");
        assert_eq!(parsed["error"]["message"], "Invalid project artifact path");

        // PUT content 非串 → 400 INVALID_PROJECT_ARTIFACT_BODY
        let (status, _, parsed) = call(
            app61(&root),
            "PUT",
            "/api/v1/project/artifacts/shorts/a.md",
            Some(r#"{"content":5}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_PROJECT_ARTIFACT_BODY");
        assert_eq!(parsed["error"]["message"], "content must be a string");

        // PUT 非 JSON body（json catch → null → content undefined）→ 400
        let (status, _, parsed) = call(
            app61(&root),
            "PUT",
            "/api/v1/project/artifacts/shorts/a.md",
            Some("not-json"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "content must be a string");

        // PUT JSON 数组（无 content 键）→ 400
        let (status, _, parsed) = call(
            app61(&root),
            "PUT",
            "/api/v1/project/artifacts/shorts/a.md",
            Some("[1]"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["message"], "content must be a string");

        // 校验失败不落盘
        assert!(!root.join("shorts").exists());
    }

    #[tokio::test]
    async fn artifacts_txt_and_markdown_ext() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("covers")).unwrap();
        std::fs::write(root.join("covers").join("note.txt"), "plain").unwrap();
        std::fs::write(root.join("covers").join("brief.markdown"), "md").unwrap();

        let (status, _, parsed) = call(
            app61(&root),
            "GET",
            "/api/v1/project/artifacts/covers/note.txt",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["contentType"], "text/plain; charset=utf-8");

        let (status, _, parsed) = call(
            app61(&root),
            "GET",
            "/api/v1/project/artifacts/covers/brief.markdown",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["contentType"], "text/markdown; charset=utf-8");
    }
}

// ── 63 号：services / cover 域（server.ts L3705-L4177 / L3854-L3953）────

mod services63_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::service_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt63(root: &std::path::Path) -> BooksRuntime {
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

    fn app63(root: &std::path::Path) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/services", axum::routing::get(service_routes::list_services))
            .route(
                "/api/v1/services/config",
                axum::routing::get(service_routes::get_services_config)
                    .put(service_routes::put_services_config),
            )
            .route(
                "/api/v1/services/config/import-env",
                axum::routing::post(service_routes::import_env_config),
            )
            .route("/api/v1/services/models", axum::routing::get(service_routes::list_services_models))
            .route(
                "/api/v1/services/models/custom",
                axum::routing::get(service_routes::list_custom_services_models),
            )
            .route(
                "/api/v1/services/:service/models",
                axum::routing::get(service_routes::list_service_models),
            )
            .route(
                "/api/v1/services/:service/secret",
                axum::routing::get(service_routes::get_service_secret)
                    .put(service_routes::put_service_secret),
            )
            .route(
                "/api/v1/services/:service/test",
                axum::routing::post(service_routes::test_service),
            )
            .route(
                "/api/v1/services/:service",
                axum::routing::delete(service_routes::delete_service),
            )
            .route(
                "/api/v1/cover/config",
                axum::routing::get(service_routes::get_cover_config)
                    .put(service_routes::put_cover_config),
            )
            .route(
                "/api/v1/cover/secret/:service",
                axum::routing::get(service_routes::get_cover_secret)
                    .put(service_routes::put_cover_secret),
            )
            .with_state(rt63(root))
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

    /// mock OpenAI 兼容 /models 上游（custom 服务 probe 用）。
    async fn spawn_models_upstream() -> (String, tokio::task::JoinHandle<()>) {
        let app = axum::Router::new().route(
            "/v1/models",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({
                    "data": [
                        { "id": "mock-chat-model" },
                        { "id": "mock-embedding-model" },
                    ]
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn list_services_bank_connected_and_priority() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        let (status, parsed) = call(app63(&root), "GET", "/api/v1/services", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let services = parsed["services"].as_array().unwrap();
        // bank 38 项（custom 排除）；测试机 home 技能不影响本端点，但进程 env 无关
        assert_eq!(services.len(), 38, "39 bank - custom");
        // 优先级排序：kkaiapi 置首
        assert_eq!(services[0]["service"], "kkaiapi");
        assert_eq!(services[1]["service"], "openrouter");
        // 未配置未存 key → 全部未连接
        assert!(services.iter().all(|s| s["connected"] == false));
        // ollama 本地端点免 key
        let ollama = services.iter().find(|s| s["service"] == "ollama").unwrap();
        assert_eq!(ollama["apiKeyOptional"], true);

        // 写 key → connected
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{ "services": { "deepseek": { "apiKey": "sk-x" } } }"#,
        )
        .unwrap();
        let (_, parsed) = call(app63(&root), "GET", "/api/v1/services", None).await;
        let deepseek = parsed["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["service"] == "deepseek")
            .unwrap()
            .clone();
        assert_eq!(deepseek["connected"], true);
    }

    #[tokio::test]
    async fn services_config_read_and_env_status() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(
            root.join("inkos.json"),
            r#"{ "llm": { "service": "deepseek", "defaultModel": "deepseek-v4-flash", "services": [{ "service": "deepseek" }] } }"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".env"),
            "INKOS_LLM_PROVIDER=anthropic\nINKOS_LLM_API_KEY=sk-env\n",
        )
        .unwrap();

        let (status, parsed) = call(app63(&root), "GET", "/api/v1/services/config", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["service"], "deepseek");
        assert_eq!(parsed["defaultModel"], "deepseek-v4-flash");
        assert_eq!(parsed["configSource"], "studio");
        // config 无 configSource 字段 → normalize 回 "env"（TS 同款）
        assert_eq!(parsed["storedConfigSource"], "env");
        assert_eq!(parsed["services"].as_array().unwrap().len(), 1);
        // env 摘要：project 层检测到（provider + key）
        let env = &parsed["envConfig"];
        assert_eq!(env["project"]["detected"], true);
        assert_eq!(env["project"]["provider"], "anthropic");
        assert_eq!(env["project"]["hasApiKey"], true);
        assert_eq!(env["effectiveSource"], "project");
        assert_eq!(env["runtimeUsesEnv"], false);
    }

    #[tokio::test]
    async fn import_env_writes_config_secrets_and_mirror() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(
            root.join(".env"),
            "INKOS_LLM_SERVICE=deepseek\nINKOS_LLM_MODEL=deepseek-v4-pro\nINKOS_LLM_API_KEY=sk-env-123\n",
        )
        .unwrap();

        let (status, parsed) = call(
            app63(&root),
            "POST",
            "/api/v1/services/config/import-env",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["source"], "project");
        assert_eq!(parsed["service"], "deepseek");
        assert_eq!(parsed["defaultModel"], "deepseek-v4-pro");

        // config 落盘：services + service + configSource=studio + 顶层镜像（63 号补齐）
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("inkos.json")).unwrap()).unwrap();
        let llm = &saved["llm"];
        assert_eq!(llm["service"], "deepseek");
        assert_eq!(llm["configSource"], "studio");
        assert_eq!(llm["provider"], "openai", "syncTopLevelLlmMirror");
        assert_eq!(llm["baseUrl"], "https://api.deepseek.com");
        assert_eq!(llm["model"], "deepseek-v4-pro");

        // secrets 落盘
        let secrets: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".inkos").join("secrets.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(secrets["services"]["deepseek"]["apiKey"], "sk-env-123");

        // 无 env → 400 平铺 error
        let dir2 = tempfile::tempdir().unwrap();
        let (status, parsed) = call(
            app63(dir2.path()),
            "POST",
            "/api/v1/services/config/import-env",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(parsed["error"].as_str().unwrap().contains("INKOS_LLM_API_KEY"));
    }

    #[tokio::test]
    async fn put_services_config_merge_env_reject_and_mirror() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("inkos.json"), r#"{ "llm": {} }"#).unwrap();

        // merge services + service + defaultModel → 镜像
        let (status, parsed) = call(
            app63(&root),
            "PUT",
            "/api/v1/services/config",
            Some(r#"{"services":[{"service":"deepseek","temperature":1.2}],"service":"deepseek","defaultModel":"deepseek-v4-flash"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("inkos.json")).unwrap()).unwrap();
        assert_eq!(saved["llm"]["service"], "deepseek");
        assert_eq!(saved["llm"]["baseUrl"], "https://api.deepseek.com");
        assert_eq!(saved["llm"]["provider"], "openai");
        assert_eq!(saved["llm"]["model"], "deepseek-v4-flash");
        assert_eq!(saved["llm"]["temperature"], 1.2);

        // configSource=env → 400 平铺（且不落盘）
        let before = std::fs::read_to_string(root.join("inkos.json")).unwrap();
        let (status, parsed) = call(
            app63(&root),
            "PUT",
            "/api/v1/services/config",
            Some(r#"{"configSource":"env"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(parsed["error"].as_str().unwrap().contains("env"));
        let after = std::fs::read_to_string(root.join("inkos.json")).unwrap();
        assert_eq!(before, after, "拒绝分支不落盘");
    }

    #[tokio::test]
    async fn delete_service_removes_entry_and_secret() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(
            root.join("inkos.json"),
            r#"{ "llm": { "service": "deepseek", "defaultModel": "m", "services": [{ "service": "deepseek" }, { "service": "moonshot" }] } }"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{ "services": { "deepseek": { "apiKey": "sk" } } }"#,
        )
        .unwrap();

        let (status, parsed) = call(app63(&root), "DELETE", "/api/v1/services/deepseek", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["service"], "deepseek");

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("inkos.json")).unwrap()).unwrap();
        let llm = &saved["llm"];
        // 选中服务被删 → service + defaultModel 联动清除；moonshot 保留
        assert_eq!(llm["services"].as_array().unwrap().len(), 1);
        assert!(llm.get("service").is_none() || llm["service"].is_null());
        assert!(llm.get("defaultModel").is_none());
        let secrets: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".inkos").join("secrets.json")).unwrap(),
        )
        .unwrap();
        assert!(secrets["services"].get("deepseek").is_none());
    }

    #[tokio::test]
    async fn service_secret_roundtrip_and_validation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // PUT 合法 key
        let (status, parsed) = call(
            app63(&root),
            "PUT",
            "/api/v1/services/deepseek/secret",
            Some(r#"{"apiKey":"sk-abc-123"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);

        // GET 回读
        let (status, parsed) = call(app63(&root), "GET", "/api/v1/services/deepseek/secret", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["apiKey"], "sk-abc-123");

        // 含空白/非 ASCII → 400 {ok:false, error}
        let (status, parsed) = call(
            app63(&root),
            "PUT",
            "/api/v1/services/deepseek/secret",
            Some(r#"{"apiKey":"bad key"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["ok"], false);
        assert!(parsed["error"].is_string());

        // 空 key → 删除
        let (status, _) = call(
            app63(&root),
            "PUT",
            "/api/v1/services/deepseek/secret",
            Some(r#"{"apiKey":"  "}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, parsed) = call(app63(&root), "GET", "/api/v1/services/deepseek/secret", None).await;
        assert_eq!(parsed["apiKey"], "");
    }

    #[tokio::test]
    async fn services_models_bank_groups_filtered_by_key() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{ "services": { "deepseek": { "apiKey": "sk" } } }"#,
        )
        .unwrap();

        let (status, parsed) = call(app63(&root), "GET", "/api/v1/services/models", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let groups = parsed["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1, "仅已存 key 的服务");
        assert_eq!(groups[0]["service"], "deepseek");
        let models = groups[0]["models"].as_array().unwrap();
        assert!(!models.is_empty());
        // 卡字段：maxOutput + contextWindow
        assert!(models[0].get("maxOutput").is_some());
        assert!(models[0].get("contextWindow").is_some());
    }

    #[tokio::test]
    async fn custom_service_models_probe_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (upstream, _guard) = spawn_models_upstream().await;
        std::fs::write(
            root.join("inkos.json"),
            format!(r#"{{ "llm": {{ "services": [{{ "service": "custom", "name": "Mock", "baseUrl": "{upstream}/v1" }}] }} }}"#),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{ "services": { "custom:Mock": { "apiKey": "sk" } } }"#,
        )
        .unwrap();

        // GET /services/models/custom：probe mock 上游 → 文本模型过滤掉 embedding
        let (status, parsed) = call(app63(&root), "GET", "/api/v1/services/models/custom", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let groups = parsed["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["service"], "custom:Mock");
        let models = groups[0]["models"].as_array().unwrap();
        assert_eq!(models.len(), 1, "embedding 模型被过滤");
        assert_eq!(models[0]["id"], "mock-chat-model");

        // GET /services/custom:Mock/models：query 参数里 custom id 走 live probe
        let (status, parsed) = call(
            app63(&root),
            "GET",
            "/api/v1/services/custom%3AMock/models?apiKey=sk&refresh=1",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let models = parsed["models"].as_array().unwrap();
        assert!(models.iter().any(|m| m["id"] == "mock-chat-model"));
    }

    #[tokio::test]
    async fn test_service_probe_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (upstream, _guard) = spawn_models_upstream().await;

        // 未知服务（无预设 baseUrl）→ 400
        let (status, parsed) = call(
            app63(&root),
            "POST",
            "/api/v1/services/unknown-svc/test",
            Some(r#"{"apiKey":"k"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["ok"], false);
        assert!(parsed["error"].as_str().unwrap().contains("unknown-svc"));

        // 公网服务无 key → 400
        let (status, parsed) = call(
            app63(&root),
            "POST",
            "/api/v1/services/deepseek/test",
            Some(r#"{}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(parsed["error"].as_str().unwrap().contains("API Key"));

        // inline baseUrl 指向 mock → probe 成功（B12 形状）
        let (status, parsed) = call(
            app63(&root),
            "POST",
            "/api/v1/services/custom/test",
            Some(&format!(r#"{{"apiKey":"sk","baseUrl":"{upstream}/v1"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["selectedModel"], "mock-chat-model");
        assert_eq!(parsed["detected"]["modelsSource"], "api");
        assert_eq!(parsed["detected"]["baseUrl"], format!("{upstream}/v1"));
        assert_eq!(parsed["probe"]["ok"], true);
        assert_eq!(parsed["chat"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn cover_config_and_secret_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // 初始：3 providers、未配置
        let (status, parsed) = call(app63(&root), "GET", "/api/v1/cover/config", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["providers"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["configured"], false);
        assert!(parsed["service"].is_null());

        // PUT 非法服务 → 400
        let (status, parsed) = call(
            app63(&root),
            "PUT",
            "/api/v1/cover/config",
            Some(r#"{"service":"bad-service"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "Unsupported cover service");

        // PUT kkaiapi + 非法 model（回 default）+ 非法 baseUrl → 400
        let (status, parsed) = call(
            app63(&root),
            "PUT",
            "/api/v1/cover/config",
            Some(r#"{"service":"kkaiapi","model":"not-in-list","baseUrl":"https://x.com/a?q=1"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(parsed["error"].as_str().unwrap().contains("Base URL"));

        // PUT 合法
        let (status, parsed) = call(
            app63(&root),
            "PUT",
            "/api/v1/cover/config",
            Some(r#"{"service":"kkaiapi"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["service"], "kkaiapi");
        assert_eq!(parsed["model"], "gpt-image-2", "非法 model 回 default");

        // secret roundtrip（cover: 前缀键）
        let (status, _) = call(
            app63(&root),
            "PUT",
            "/api/v1/cover/secret/kkaiapi",
            Some(r#"{"apiKey":"sk-cover"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, parsed) = call(app63(&root), "GET", "/api/v1/cover/secret/kkaiapi", None).await;
        assert_eq!(parsed["apiKey"], "sk-cover");

        // configured 变 true（cover service + key）
        let (_, parsed) = call(app63(&root), "GET", "/api/v1/cover/config", None).await;
        assert_eq!(parsed["configured"], true);
        assert_eq!(parsed["service"], "kkaiapi");

        // 非法 cover key → 400
        let (status, parsed) = call(
            app63(&root),
            "PUT",
            "/api/v1/cover/secret/kkaiapi",
            Some(r#"{"apiKey":"bad key"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(parsed["error"].as_str().unwrap().contains("Authorization"));
    }

    #[tokio::test]
    async fn get_project_returns_effective_llm_after_merge() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // raw llm 顶层字段空/缺——有效值来自 services + secrets 合并（63 号补齐 56 号偏差）
        std::fs::write(
            root.join("inkos.json"),
            r#"{
  "name": "p1",
  "language": "zh",
  "llm": {
    "service": "custom:Mock",
    "defaultModel": "mock-chat-model",
    "services": [{ "service": "custom", "name": "Mock", "baseUrl": "https://mock.example/v1" }]
  }
}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{ "services": { "custom:Mock": { "apiKey": "sk" } } }"#,
        )
        .unwrap();

        let app = app63(&root).route(
            "/api/v1/project",
            axum::routing::get(inkos_engine::server::project_config_routes::get_project)
                .with_state(rt63(&root)),
        );
        let (status, parsed) = call(app, "GET", "/api/v1/project", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        // raw 顶层 model/baseUrl 缺失 → 合并层从选中服务补齐（而非 schema 校验失败）
        assert_eq!(parsed["model"], "mock-chat-model");
        assert_eq!(parsed["baseUrl"], "https://mock.example/v1");
        assert_eq!(parsed["provider"], "custom");
    }

    #[tokio::test]
    async fn put_default_model_mirrors_top_level() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("inkos.json"), r#"{ "name": "p", "llm": { "service": "deepseek" } }"#).unwrap();

        let app = app63(&root).route(
            "/api/v1/project/default-model",
            axum::routing::put(inkos_engine::server::project_config_routes::put_default_model)
                .with_state(rt63(&root)),
        );
        let (status, parsed) = call(
            app,
            "PUT",
            "/api/v1/project/default-model",
            Some(r#"{"defaultModel":"deepseek-v4-pro"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("inkos.json")).unwrap()).unwrap();
        let llm = &saved["llm"];
        assert_eq!(llm["defaultModel"], "deepseek-v4-pro");
        // 63 号补齐：顶层镜像
        assert_eq!(llm["model"], "deepseek-v4-pro");
        assert_eq!(llm["baseUrl"], "https://api.deepseek.com");
        assert_eq!(llm["provider"], "openai");
    }
}

// ── 64 号：sessions / interaction 会话域（server.ts L4536 / L4703-L4803）──

mod sessions64_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt64(root: &std::path::Path) -> BooksRuntime {
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

    fn app64(root: &std::path::Path) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/interaction/session",
                axum::routing::get(session_routes::get_interaction_session),
            )
            .route(
                "/api/v1/sessions",
                axum::routing::get(session_routes::list_sessions)
                    .post(session_routes::create_session),
            )
            .route(
                "/api/v1/sessions/:sessionId/play-mode",
                axum::routing::put(session_routes::put_session_play_mode),
            )
            .route(
                "/api/v1/sessions/:sessionId/abort",
                axum::routing::post(session_routes::abort_session),
            )
            .route(
                "/api/v1/sessions/:sessionId",
                axum::routing::get(session_routes::get_session)
                    .put(session_routes::rename_session)
                    .delete(session_routes::delete_session),
            )
            .with_state(rt64(root))
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

    const SESSION_ID: &str = "1782900000000-abc123";

    /// transcript fixture：session_created + 一轮 committed user/assistant 消息。
    fn transcript_fixture(root: &std::path::Path, book_id: Option<&str>, session_kind: Option<&str>) {
        std::fs::create_dir_all(root.join(".inkos").join("sessions")).unwrap();
        let created = serde_json::json!({
            "type": "session_created", "version": 1, "sessionId": SESSION_ID, "seq": 1,
            "timestamp": 1000, "bookId": book_id, "sessionKind": session_kind, "title": null,
            "createdAt": 1000, "updatedAt": 1000,
        });
        let started = serde_json::json!({
            "type": "request_started", "version": 1, "sessionId": SESSION_ID, "seq": 2,
            "timestamp": 1100, "requestId": "req-1", "input": "",
        });
        let user = serde_json::json!({
            "type": "message", "version": 1, "sessionId": SESSION_ID, "seq": 3,
            "timestamp": 1100, "requestId": "req-1", "uuid": "u1", "parentUuid": null,
            "role": "user", "message": { "role": "user", "content": "帮我构思一个修仙故事：主角是青阳镇走出的一位坚韧少年，背负血仇踏入修行路", "timestamp": 1100 },
        });
        let assistant = serde_json::json!({
            "type": "message", "version": 1, "sessionId": SESSION_ID, "seq": 4,
            "timestamp": 1200, "requestId": "req-1", "uuid": "u2", "parentUuid": "u1",
            "role": "assistant",
            "message": {
                "role": "assistant",
                "content": [{ "type": "text", "text": "主角可以是一个废柴少年，偶得上古传承。" }],
                "timestamp": 1200,
            },
        });
        let committed = serde_json::json!({
            "type": "request_committed", "version": 1, "sessionId": SESSION_ID, "seq": 5,
            "timestamp": 1200, "requestId": "req-1",
        });
        std::fs::write(
            root.join(".inkos").join("sessions").join(format!("{SESSION_ID}.jsonl")),
            [created, started, user, assistant, committed]
                .iter()
                .map(|e| serde_json::to_string(e).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
    }

    #[tokio::test]
    async fn interaction_session_resolves_active_book() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 唯一书目录 → activeBookId 自动解析
        std::fs::create_dir_all(root.join("books").join("my-book")).unwrap();

        let (status, parsed) = call(app64(&root), "GET", "/api/v1/interaction/session", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["activeBookId"], "my-book");
        assert_eq!(parsed["session"]["activeBookId"], "my-book", "session.activeBookId 以解析值覆盖");
        assert_eq!(parsed["session"]["automationMode"], "semi", "zod default 填充");
        assert_eq!(parsed["session"]["messages"], serde_json::json!([]));

        // 多书且无 activeBookId → undefined
        std::fs::create_dir_all(root.join("books").join("other-book")).unwrap();
        let (_, parsed) = call(app64(&root), "GET", "/api/v1/interaction/session", None).await;
        assert!(parsed["activeBookId"].is_null());
    }

    #[tokio::test]
    async fn session_crud_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        transcript_fixture(&root, Some("my-book"), Some("book"));

        // GET 详情：消息重建 + 首条 user 消息标题（≤20 字 + …）
        let (status, parsed) = call(app64(&root), "GET", &format!("/api/v1/sessions/{SESSION_ID}"), None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let session = &parsed["session"];
        assert_eq!(session["sessionId"], SESSION_ID);
        assert_eq!(session["bookId"], "my-book");
        assert_eq!(session["sessionKind"], "book");
        assert_eq!(session["draftRounds"], serde_json::json!([]), "zod default");
        let title = session["title"].as_str().unwrap();
        assert!(title.ends_with('…'), "超 20 字标题截断：{title}");
        assert!(title.chars().count() <= 21, "20 码元 + 省略号：{title}");
        let messages = session["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert!(messages[1]["content"].as_str().unwrap().contains("废柴少年"));

        // 缺失 → 404 平铺
        let (status, parsed) = call(app64(&root), "GET", "/api/v1/sessions/nope-1", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"], "Session not found");

        // PUT 改名
        let (status, parsed) = call(
            app64(&root),
            "PUT",
            &format!("/api/v1/sessions/{SESSION_ID}"),
            Some(r#"{"title":"我的会话"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["session"]["title"], "我的会话");
        // 改名落盘（metadata 事件）
        let (_, parsed) = call(app64(&root), "GET", &format!("/api/v1/sessions/{SESSION_ID}"), None).await;
        assert_eq!(parsed["session"]["title"], "我的会话");

        // PUT 空 title → 400 ApiError
        let (status, parsed) = call(
            app64(&root),
            "PUT",
            &format!("/api/v1/sessions/{SESSION_ID}"),
            Some(r#"{"title":"  "}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_SESSION_TITLE");

        // PUT 改名不存在 → 404
        let (status, _) = call(app64(&root), "PUT", "/api/v1/sessions/nope-1", Some(r#"{"title":"x"}"#)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn session_list_and_create() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        transcript_fixture(&root, Some("my-book"), Some("book"));

        // 列表（TS `session.bookId !== bookId` 严格过滤：无过滤 = 只列未绑定书的会话）
        let (status, parsed) = call(app64(&root), "GET", "/api/v1/sessions", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["sessions"].as_array().unwrap().len(), 0, "已绑定书的会话不在 null 过滤内");
        let (_, parsed) = call(app64(&root), "GET", "/api/v1/sessions?bookId=my-book", None).await;
        let sessions = parsed["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["sessionId"], SESSION_ID);
        assert_eq!(sessions[0]["bookId"], "my-book");
        assert_eq!(sessions[0]["messageCount"], 2);
        let (_, parsed) = call(app64(&root), "GET", "/api/v1/sessions?bookId=other", None).await;
        assert_eq!(parsed["sessions"].as_array().unwrap().len(), 0);
        // 创建未绑定 chat 会话后，无过滤列表可见
        let _ = call(app64(&root), "POST", "/api/v1/sessions", Some(r#"{"sessionId":"1782950000000-chat01"}"#)).await;
        let (_, parsed) = call(app64(&root), "GET", "/api/v1/sessions", None).await;
        let ids: Vec<&str> = parsed["sessions"].as_array().unwrap().iter().map(|s| s["sessionId"].as_str().unwrap()).collect();
        assert!(ids.contains(&"1782950000000-chat01"), "{ids:?}");
        assert!(!ids.contains(&SESSION_ID), "绑定书的会话不出现");

        // POST 创建：无 bookId → chat kind；safeSessionId 命中
        let (status, parsed) = call(
            app64(&root),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782950000000-xyz789"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["session"]["sessionId"], "1782950000000-xyz789");
        assert_eq!(parsed["session"]["sessionKind"], "chat", "无 bookId 回退 chat");
        assert!(parsed["session"]["bookId"].is_null());
        assert!(parsed["session"]["title"].is_null());

        // 幂等：同 id 再建（kind 变更走 metadata 更新）
        let (_, parsed) = call(
            app64(&root),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782950000000-xyz789","sessionKind":"edit"}"#),
        )
        .await;
        assert_eq!(parsed["session"]["sessionKind"], "edit");

        // 非法 sessionKind → 400
        let (status, parsed) = call(
            app64(&root),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionKind":"bad"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_SESSION_KIND");

        // 不安全 sessionId（注入形态）→ 忽略后生成新 id
        let (_, parsed) = call(
            app64(&root),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"../../etc/passwd"}"#),
        )
        .await;
        let generated = parsed["session"]["sessionId"].as_str().unwrap();
        assert_ne!(generated, "../../etc/passwd");
        assert!(!generated.contains('/'));
    }

    #[tokio::test]
    async fn play_mode_update_and_abort() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture64_create(&root).await;

        // play-mode：非法值 → 400 INVALID_PLAY_MODE
        let (status, parsed) = call(
            app64(&root),
            "PUT",
            &format!("/api/v1/sessions/{SESSION_ID}/play-mode"),
            Some(r#"{"playMode":"bad"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_PLAY_MODE");

        // 合法更新（open）
        let (status, parsed) = call(
            app64(&root),
            "PUT",
            &format!("/api/v1/sessions/{SESSION_ID}/play-mode"),
            Some(r#"{"playMode":"open"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["session"]["playMode"], "open");
        // 不存在的会话 → 404
        let (status, _) = call(
            app64(&root),
            "PUT",
            "/api/v1/sessions/nope-1/play-mode",
            Some(r#"{"playMode":"open"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // abort：无运行执行体 → aborted=false + agent:aborted 广播
        let (status, parsed) = call(
            app64(&root),
            "POST",
            &format!("/api/v1/sessions/{SESSION_ID}/abort"),
            Some(r#"{"scope":"all"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["aborted"], false);
    }

    async fn fixture64_create(root: &std::path::Path) {
        let app = app64(root);
        let _ = call(
            app,
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SESSION_ID}"}}"#)),
        )
        .await;
    }

    #[tokio::test]
    async fn session_delete_removes_files_and_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture64_create(&root).await;
        let transcript = root.join(".inkos").join("sessions").join(format!("{SESSION_ID}.jsonl"));
        assert!(transcript.exists());
        // 任务快照落盘（对账改写终态的前提件）
        std::fs::create_dir_all(root.join(".inkos").join("tasks")).unwrap();
        std::fs::write(
            root.join(".inkos").join("tasks").join(format!("{SESSION_ID}.json")),
            r#"{"version":1,"sessionId":"SESSION","requestedIntent":"write_next","execution":{"id":"e1","tool":"write_next","label":"x","status":"completed","startedAt":1,"completedAt":2},"updatedAt":1}"#.replace("SESSION", SESSION_ID),
        )
        .unwrap();

        let (status, parsed) = call(app64(&root), "DELETE", &format!("/api/v1/sessions/{SESSION_ID}"), None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert!(!transcript.exists());
        assert!(!root.join(".inkos").join("tasks").join(format!("{SESSION_ID}.json")).exists());
    }

    #[tokio::test]
    async fn running_task_snapshot_is_reconciled_to_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture64_create(&root).await;
        std::fs::create_dir_all(root.join(".inkos").join("tasks")).unwrap();
        std::fs::write(
            root.join(".inkos").join("tasks").join(format!("{SESSION_ID}.json")),
            format!(r#"{{"version":1,"sessionId":"{SESSION_ID}","requestedIntent":"write_next","execution":{{"id":"e1","tool":"write_next","label":"写作","status":"running","startedAt":1}},"updatedAt":1}}"#),
        )
        .unwrap();

        // GET 详情：running 快照 + 本进程无运行确认 → 对账改写为 error 终态并落盘
        let (status, parsed) = call(app64(&root), "GET", &format!("/api/v1/sessions/{SESSION_ID}"), None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["task"]["execution"]["status"], "error");
        assert!(parsed["task"]["execution"]["error"].as_str().unwrap().contains("任务已中断"));
        let saved: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".inkos").join("tasks").join(format!("{SESSION_ID}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(saved["execution"]["status"], "error", "对账结果已持久化");
    }

    #[tokio::test]
    async fn legacy_json_session_migrates_to_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".inkos").join("sessions")).unwrap();
        // 旧版单文件形态
        std::fs::write(
            root.join(".inkos").join("sessions").join(format!("{SESSION_ID}.json")),
            format!(r#"{{"sessionId":"{SESSION_ID}","bookId":"b1","title":"旧会话","messages":[{{"role":"user","content":"你好","timestamp":1000}},{{"role":"assistant","content":"在的","timestamp":1100}}],"createdAt":1000,"updatedAt":1100}}"#),
        )
        .unwrap();

        let (status, parsed) = call(app64(&root), "GET", &format!("/api/v1/sessions/{SESSION_ID}"), None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["session"]["title"], "旧会话");
        let messages = parsed["session"]["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["content"], "你好");
        // 迁移后 transcript 落盘
        assert!(root.join(".inkos").join("sessions").join(format!("{SESSION_ID}.jsonl")).exists());
    }
}

// ── 65 号：POST /api/v1/agent 直通聊天主路径（server.ts L4805）─────────

mod agent65_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt65(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.into(),
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

    fn app65(root: &std::path::Path, llm: &str) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .route(
                "/api/v1/sessions/:sessionId",
                axum::routing::get(session_routes::get_session),
            )
            .with_state(rt65(root, llm))
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

    async fn mock_llm() -> (String, tokio::task::JoinHandle<()>) {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|| async {
                // 首个 chunk 起始（system 关键词不匹配分发器，直接固定回复）
                let chunk = serde_json::json!({ "choices": [{ "delta": { "content": "好的，我来帮你分析这个修仙故事的节奏问题。" } }] });
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 8, "completion_tokens": 6, "total_tokens": 14 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), handle)
    }

    const SESSION_ID: &str = "1782960000000-agent01";

    async fn create_session(root: &std::path::Path) {
        let _ = call(
            app65(root, "http://127.0.0.1:9"),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SESSION_ID}"}}"#)),
        )
        .await;
    }

    #[tokio::test]
    async fn validation_branches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let app = app65(&root, "http://127.0.0.1:9");

        // 无 instruction → 平铺 400
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/agent", Some(r#"{"sessionId":"s"}"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "No instruction provided");

        // 无 sessionId → ApiError 400 SESSION_ID_REQUIRED
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/agent", Some(r#"{"instruction":"你好"}"#)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "SESSION_ID_REQUIRED");

        // 会话不存在 → 404 SESSION_NOT_FOUND
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"你好","sessionId":"nope-1"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"]["code"], "SESSION_NOT_FOUND");
        assert_eq!(parsed["error"]["message"], "Session not found: nope-1");

        // 非 JSON body → 平铺 400（instruction 缺省）
        let (status, _) = call(app.clone(), "POST", "/api/v1/agent", Some("not-json")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn chat_roundtrip_persists_transcript_and_broadcasts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, _guard) = mock_llm().await;
        create_session(&root).await;

        let app = app65(&root, &llm);
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"帮我看下第二章节奏","sessionId":"{SESSION_ID}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert!(parsed["response"].as_str().unwrap().contains("修仙故事"));
        assert_eq!(parsed["session"]["sessionId"], SESSION_ID);
        assert_eq!(parsed["session"]["sessionKind"], "chat");

        // transcript 持久化：user + assistant 消息可从会话详情 derive
        let (status, parsed) = call(
            app,
            "GET",
            &format!("/api/v1/sessions/{SESSION_ID}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let messages = parsed["session"]["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2, "user + assistant");
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "帮我看下第二章节奏");
        assert_eq!(messages[1]["role"], "assistant");
        assert!(messages[1]["content"].as_str().unwrap().contains("修仙故事"));
        // 无标题会话 → 首条 user 消息成为标题
        let title = parsed["session"]["title"].as_str().unwrap();
        assert!(title.contains("帮我看下"));
    }

    #[tokio::test]
    async fn book_binding_mismatch_and_missing_book() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 已绑定书 b1 的会话
        let _ = call(
            app65(&root, "http://127.0.0.1:9"),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SESSION_ID}","bookId":"b1"}}"#)),
        )
        .await;
        std::fs::create_dir_all(root.join("books").join("b1")).unwrap();
        std::fs::write(
            root.join("books").join("b1").join("book.json"),
            r#"{"id":"b1","title":"t","platform":"other","genre":"xianxia","status":"active","targetChapters":10,"chapterWordCount":3000,"createdAt":0,"updatedAt":0}"#,
        )
        .unwrap();

        // 请求 activeBookId 与绑定不一致 → 409 SESSION_BOOK_MISMATCH
        let (status, parsed) = call(
            app65(&root, "http://127.0.0.1:9"),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"hi","sessionId":"{SESSION_ID}","activeBookId":"b2"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "SESSION_BOOK_MISMATCH");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("b1"));

        // 匹配书但书文件缺失 → 404 BOOK_NOT_FOUND
        let _ = call(
            app65(&root, "http://127.0.0.1:9"),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782960000001-bkmiss","bookId":"ghost"}"#),
        )
        .await;
        let (status, parsed) = call(
            app65(&root, "http://127.0.0.1:9"),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"hi","sessionId":"1782960000001-bkmiss","activeBookId":"ghost"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"]["code"], "BOOK_NOT_FOUND");

        // 绑定书一致 → 直通聊天成功（kind book）
        let (llm, _guard) = mock_llm().await;
        let (status, parsed) = call(
            app65(&root, &llm),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"第二章节奏如何","sessionId":"{SESSION_ID}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["session"]["sessionKind"], "book");
        assert_eq!(parsed["session"]["activeBookId"], "b1");
    }

    #[tokio::test]
    async fn llm_failure_is_agent_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        create_session(&root).await;
        // LLM 不可达 → agent:error 形态 500
        let (status, parsed) = call(
            app65(&root, "http://127.0.0.1:9"),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"hi","sessionId":"{SESSION_ID}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "AGENT_SESSION_FAILED");
        assert!(parsed["response"].is_string());
    }
}

// ── 66 号：POST /agent 工具循环（tool-calls 多轮 + SSE + 工具执行）────

mod agent66_e2e {
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt66(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.into(),
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

    fn app66(root: &std::path::Path, llm: &str) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .route(
                "/api/v1/sessions/:sessionId",
                axum::routing::get(session_routes::get_session),
            )
            .with_state(rt66(root, llm))
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

    /// 两轮 mock：首轮 tool_calls(read)，次轮最终文本。断言请求里带 tools。
    async fn mock_tool_llm() -> (String, tokio::task::JoinHandle<()>) {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let calls_for_server = calls.clone();
        let app = axum::Router::new()
            .route(
                "/chat/completions",
                axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let calls = calls_for_server.clone();
                    async move {
                        let has_tools = body.get("tools").is_some();
                        let msg_count = body["messages"].as_array().map(|m| m.len()).unwrap_or(0);
                        calls.lock().unwrap().push(format!("{has_tools}:{msg_count}"));
                        let chunk = if msg_count <= 2 {
                            // 首轮：发起 read 工具调用
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_read_1", "function": { "name": "read", "arguments": "{\"path\":\"note.md\"}" } },
                            ] } }] })
                        } else {
                            serde_json::json!({ "choices": [{ "delta": { "content": "笔记里写着：这是主角设定草稿。" } }] })
                        };
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 9, "completion_tokens": 7, "total_tokens": 16 } });
                        axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), handle)
    }

    const SESSION_ID: &str = "1782970000000-tool01";

    #[tokio::test]
    async fn tool_loop_roundtrip_with_execution_cards() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("note.md"), "这是主角设定草稿。").unwrap();
        let (llm, _guard) = mock_tool_llm().await;
        let _ = call(
            app66(&root, &llm),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SESSION_ID}"}}"#)),
        )
        .await;

        let app = app66(&root, &llm);
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"看看 note.md 写了什么","sessionId":"{SESSION_ID}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "笔记里写着：这是主角设定草稿。");

        // 工具执行卡
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0]["tool"], "read");
        assert_eq!(execs[0]["status"], "completed");
        assert!(execs[0]["result"].as_str().unwrap().contains("主角设定草稿"));

        // transcript：会话详情可见 assistant 轮文本（工具轮并入 assistant 卡）
        let (status, parsed) = call(app, "GET", &format!("/api/v1/sessions/{SESSION_ID}"), None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let messages = parsed["session"]["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "user");
        let assistant = &messages[1];
        assert_eq!(assistant["role"], "assistant");
        assert!(assistant["content"].as_str().unwrap().contains("主角设定草稿"));
    }

    #[tokio::test]
    async fn direct_chat_still_works_without_tool_calls() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 65 号 mock（无 tool_calls，单轮文本）复用
        let app_chat = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|| async {
                let chunk = serde_json::json!({ "choices": [{ "delta": { "content": "直接回答。" } }] });
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app_chat).await.unwrap(); });
        let llm = format!("http://{addr}");

        let _ = call(
            app66(&root, &llm),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SESSION_ID}"}}"#)),
        )
        .await;
        let (status, parsed) = call(
            app66(&root, &llm),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"你好","sessionId":"{SESSION_ID}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "直接回答。");
        assert_eq!(parsed["details"]["toolExecutions"].as_array().unwrap().len(), 0);
    }
}

mod agent67_e2e {
    //! 67 号：确认式生产任务分支（write_next / create_book 执行器 + 单任务
    //! 闸门 + task 快照 + 建书迁移 + 聊天分支背景任务上下文）。
    use super::books58_e2e::spawn_mock58;
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::interaction::session_transcript::transcript_path;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_production::{
        active_confirmed_tasks, reserved_production_sessions,
    };
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::server::task_store::studio_task_snapshot_path;
    use inkos_engine::state::manager::StateManager;

    fn rt67(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 8192,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    fn app67(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .route(
                "/api/v1/sessions/:sessionId",
                axum::routing::get(session_routes::get_session),
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
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) };
        (status, parsed)
    }

    /// 捕获 system prompt 的聊天 mock（背景任务上下文注入断言用）。
    async fn mock_capture_system() -> (String, Arc<Mutex<Vec<String>>>) {
        let systems = Arc::new(Mutex::new(Vec::<String>::new()));
        let systems_for_server = systems.clone();
        let app = axum::Router::new()
            .route(
                "/chat/completions",
                axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let systems = systems_for_server.clone();
                    async move {
                        let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                        systems.lock().unwrap().push(system);
                        let chunk = serde_json::json!({ "choices": [{ "delta": { "content": "直接回答。" } }] });
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 9, "completion_tokens": 7, "total_tokens": 16 } });
                        axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), systems)
    }

    const SID_WRITE: &str = "1782988000000-prod01";
    const SID_GATE: &str = "1782988000001-gate";
    const SID_BOOK: &str = "1782988000002-book";
    const SID_UNSUP: &str = "1782988000003-uns";
    const SID_CTX: &str = "1782988000004-ctx";

    #[tokio::test]
    async fn write_next_production_success_persists_snapshot_and_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, _calls, _guard) = spawn_mock_llm().await;

        let runtime = rt67(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();

        let app = app67(runtime);
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SID_WRITE}","bookId":"b1","sessionKind":"book"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // free-text 明确写章命令 → 确认式任务分支（归一 write_next）。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"写下一章","sessionId":"{SID_WRITE}","actionSource":"free-text","clientRequestId":"req-67"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert!(
            parsed["response"].as_str().unwrap_or("").starts_with("已为 b1 完成第"),
            "response: {parsed}"
        );

        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "sub_agent");
        assert_eq!(exec["agent"], "writer");
        assert_eq!(exec["label"], "写作");
        assert_eq!(exec["status"], "completed");
        assert_eq!(exec["args"]["bookId"], "b1");
        assert_eq!(exec["args"]["agent"], "writer");
        assert_eq!(exec["details"]["kind"], "chapter_written");
        assert_eq!(exec["details"]["bookId"], "b1");
        let stages = exec["stages"].as_array().unwrap();
        assert_eq!(stages.len(), 7);
        assert!(stages.iter().all(|s| s["status"] == "completed"));
        assert!(
            exec["logs"].as_array().unwrap().iter().any(|log| log.as_str().unwrap().contains("正在为 b1 写下一章")),
            "logs: {exec}"
        );
        assert_eq!(parsed["session"]["activeBookId"], "b1");
        assert_eq!(parsed["session"]["sessionKind"], "book");

        // 任务快照落盘（sourceRequestId 透传）。
        let snapshot_raw = std::fs::read_to_string(studio_task_snapshot_path(&root, SID_WRITE)).unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot_raw).unwrap();
        assert_eq!(snapshot["requestedIntent"], "write_next");
        assert_eq!(snapshot["sourceRequestId"], "req-67");
        assert_eq!(snapshot["execution"]["status"], "completed");
        assert_eq!(snapshot["execution"]["tool"], "sub_agent");
        assert_eq!(snapshot["execution"]["agent"], "writer");
        assert_eq!(snapshot["execution"]["label"], "写作");

        // transcript：user 先写 + 收尾助手工具消息（toolUse + legacyDisplay）。
        let transcript = std::fs::read_to_string(transcript_path(&root, SID_WRITE)).unwrap();
        assert!(transcript.contains("\"content\":\"写下一章\""), "transcript: {transcript}");
        assert!(transcript.contains("\"stopReason\":\"toolUse\""), "transcript: {transcript}");
        assert!(transcript.contains("\"toolExecutions\""), "transcript: {transcript}");

        // SSE 顺序：agent:start → tool:start(background) → tool:end → agent:complete。
        assert_eq!(subscriber.recv().await.unwrap().event, "agent:start");
        let tool_start = subscriber.recv().await.unwrap();
        assert_eq!(tool_start.event, "tool:start");
        assert!(tool_start.data.contains("\"background\":true"), "data: {}", tool_start.data);
        let tool_end = subscriber.recv().await.unwrap();
        assert_eq!(tool_end.event, "tool:end");
        assert!(tool_end.data.contains("\"isError\":false"), "data: {}", tool_end.data);
        assert_eq!(subscriber.recv().await.unwrap().event, "agent:complete");
    }

    #[tokio::test]
    async fn reserved_session_gate_returns_409() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, _calls, _guard) = spawn_mock_llm().await;
        let app = app67(rt67(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SID_GATE}","bookId":"b1","sessionKind":"book"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 预留表已有本会话（模拟并发第一确认请求）→ 第二请求 409。
        reserved_production_sessions()
            .lock()
            .unwrap()
            .insert(SID_GATE.to_string(), "direct-write_next-x".to_string());
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"写下一章","sessionId":"{SID_GATE}","requestedIntent":"write_next"}}"#
            )),
        )
        .await;
        reserved_production_sessions().lock().unwrap().remove(SID_GATE);
        assert_eq!(status, StatusCode::CONFLICT, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "PRODUCTION_TASK_ALREADY_RUNNING");
        assert!(
            parsed["response"].as_str().unwrap_or("").contains("已有一个生产任务在运行"),
            "body: {parsed}"
        );
    }

    #[tokio::test]
    async fn create_book_migrates_session_and_broadcasts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let llm = spawn_mock58().await;

        let runtime = rt67(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();

        let app = app67(runtime);
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SID_BOOK}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"创建一本青云修仙小说","sessionId":"{SID_BOOK}","actionSource":"button","requestedIntent":"create_book","actionPayload":{{"createBook":{{"title":"青云仙路","genre":"xianxia"}}}}}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(
            parsed["response"],
            "Book \"青云仙路\" (青云仙路) initialised successfully. Foundation files are ready."
        );
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "sub_agent");
        assert_eq!(exec["agent"], "architect");
        assert_eq!(exec["label"], "建书");
        assert_eq!(exec["details"]["kind"], "book_created");
        assert_eq!(exec["details"]["bookId"], "青云仙路");
        assert_eq!(exec["args"]["title"], "青云仙路");
        assert_eq!(exec["args"]["genre"], "xianxia");
        assert_eq!(parsed["session"]["activeBookId"], "青云仙路");

        // 书落盘（staging 原子 rename 完成性）。
        let book_dir = root.join("books").join("青云仙路");
        assert!(book_dir.join("book.json").is_file());
        assert!(book_dir.join("story").join("story_bible.md").is_file());

        // 会话迁移：GET /sessions/:id → bookId 绑定新书。
        let (status, session_payload) = call(
            app,
            "GET",
            &format!("/api/v1/sessions/{SID_BOOK}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {session_payload}");
        assert_eq!(session_payload["session"]["bookId"], "青云仙路");

        // 快照 + SSE（book:creating 在 book:created 前，tool 卡全程广播）。
        let snapshot: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(studio_task_snapshot_path(&root, SID_BOOK)).unwrap(),
        )
        .unwrap();
        assert_eq!(snapshot["requestedIntent"], "create_book");
        assert_eq!(snapshot["execution"]["status"], "completed");
        assert_eq!(snapshot["execution"]["agent"], "architect");

        let mut saw_creating = false;
        let mut saw_created = false;
        let mut events = Vec::new();
        for _ in 0..6 {
            let event = tokio::time::timeout(std::time::Duration::from_secs(10), subscriber.recv())
                .await
                .expect("SSE 事件应到达")
                .unwrap();
            if event.event == "book:creating" {
                saw_creating = true;
                assert!(event.data.contains("青云仙路"));
            }
            if event.event == "book:created" {
                saw_created = true;
                assert!(event.data.contains("\"sessionId\""));
            }
            events.push(event.event);
        }
        assert!(saw_creating, "events: {events:?}");
        assert!(saw_created, "events: {events:?}");
        assert_eq!(
            events.iter().position(|e| e == "book:creating").unwrap(),
            1,
            "book:creating 应紧随 agent:start（先于 tool:start）: {events:?}"
        );
        assert!(
            events.iter().position(|e| e == "book:creating").unwrap()
                < events.iter().position(|e| e == "tool:start").unwrap(),
            "book:creating 应在 tool:start 前: {events:?}"
        );
        assert!(
            events.iter().position(|e| e == "book:created").unwrap()
                > events.iter().position(|e| e == "tool:end").unwrap(),
            "book:created 应在 tool:end 后: {events:?}"
        );
    }

    #[tokio::test]
    async fn unsupported_intent_and_missing_title_fail_with_502() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let llm = spawn_mock58().await;
        let app = app67(rt67(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SID_UNSUP}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 未支持意图（short_run 域未迁移）→ 502 AGENT_ACTION_FAILED。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"写个短篇","sessionId":"{SID_UNSUP}","actionSource":"button","requestedIntent":"short_run"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "AGENT_ACTION_FAILED");
        assert_eq!(parsed["error"]["message"], "Unsupported confirmed action: short_run");

        // create_book 缺 title → 502 + 中文缺字段文案（TS：ApiError 被分支 catch 转写）。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"创建一本小说","sessionId":"{SID_UNSUP}","actionSource":"button","requestedIntent":"create_book","actionPayload":{{"createBook":{{}}}}}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "AGENT_ACTION_FAILED");
        assert!(
            parsed["error"]["message"].as_str().unwrap().contains("确认建书缺少书名"),
            "body: {parsed}"
        );
    }

    #[tokio::test]
    async fn chat_turn_injects_running_task_context() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, systems) = mock_capture_system().await;

        // 构造运行中任务：快照 running + 注册表持有同 id（本进程确认）。
        let task_id = "direct-write_next-running01";
        let snapshot = serde_json::json!({
            "version": 1,
            "sessionId": SID_CTX,
            "requestedIntent": "write_next",
            "updatedAt": 1782988000000_f64,
            "execution": {
                "id": task_id,
                "tool": "sub_agent",
                "agent": "writer",
                "label": "写作",
                "status": "running",
                "startedAt": 1782988000000_f64,
            },
        });
        std::fs::create_dir_all(root.join(".inkos").join("tasks")).unwrap();
        std::fs::write(
            studio_task_snapshot_path(&root, SID_CTX),
            format!("{snapshot}\n"),
        )
        .unwrap();
        active_confirmed_tasks().lock().unwrap().insert(
            task_id.to_string(),
            Arc::new(Mutex::new(false)),
        );

        let app = app67(rt67(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SID_CTX}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 非写章命令（chat 会话）→ 聊天分支 + system 注入背景任务状态块。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"你好","sessionId":"{SID_CTX}","actionSource":"free-text"}}"#
            )),
        )
        .await;
        active_confirmed_tasks().lock().unwrap().remove(task_id);
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let systems = systems.lock().unwrap();
        assert!(
            systems.iter().any(|system| system.contains("## 后台任务状态")
                && system.contains("正在后台运行的生产任务")
                && system.contains("写作")),
            "captured systems: {systems:?}"
        );
    }
}

mod agent68_e2e {
    //! 68 号：abort 端点接确认任务注册表（scope 语义 + controller 链）+
    //! restoreAgentMessages 历史回放（summary + dialogue + boundary 注入）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::interaction::session::SessionKind;
    use inkos_engine::interaction::session_transcript::{transcript_path, TranscriptEvent};
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_production::{
        active_confirmed_tasks, reserved_production_sessions,
    };
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt68(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 8192,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    fn app68(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .route(
                "/api/v1/sessions/:sessionId/abort",
                axum::routing::post(session_routes::abort_session),
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
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) };
        (status, parsed)
    }

    /// 捕获完整 messages 的聊天 mock。
    async fn mock_capture_messages() -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let messages_log = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let log = messages_log.clone();
        let app = axum::Router::new()
            .route(
                "/chat/completions",
                axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let log = log.clone();
                    async move {
                        log.lock().unwrap().push(body["messages"].clone());
                        let chunk = serde_json::json!({ "choices": [{ "delta": { "content": "收到。" } }] });
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 9, "completion_tokens": 7, "total_tokens": 16 } });
                        axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), messages_log)
    }

    const SID_ABORT_ALL: &str = "1782989000000-ab1";
    const SID_ABORT_CHAT: &str = "1782989000002-ab2";
    const SID_ABORT_NONE: &str = "1782989000003-ab3";
    const SID_REPLAY: &str = "1782989000001-replay";

    fn write_transcript(root: &std::path::Path, session_id: &str, events: Vec<TranscriptEvent>) {
        use std::fmt::Write as _;
        std::fs::create_dir_all(root.join(".inkos").join("sessions")).unwrap();
        let mut payload = String::new();
        for event in events {
            let line = serde_json::to_string(&event).unwrap();
            writeln!(payload, "{line}").unwrap();
        }
        std::fs::write(transcript_path(root, session_id), payload).unwrap();
    }

    fn msg_event(seq: u64, request_id: &str, role: &str, session_id: &str, message: serde_json::Value) -> TranscriptEvent {
        TranscriptEvent::Message {
            version: 1,
            session_id: session_id.into(),
            request_id: request_id.into(),
            uuid: format!("u{seq}"),
            parent_uuid: None,
            seq,
            timestamp: 1782989000000 + seq,
            role: role.into(),
            pi_turn_index: None,
            tool_call_id: None,
            source_tool_assistant_uuid: None,
            legacy_display: None,
            message,
        }
    }

    #[tokio::test]
    async fn abort_all_stops_running_production_task() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let runtime = rt68(&root, "http://127.0.0.1:9");
        let mut subscriber = runtime.hub.subscribe();
        let app = app68(runtime);
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SID_ABORT_ALL}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 插桩：确认任务运行中（reserved + active 注册表，内存优先路径）。
        let task_flag = Arc::new(Mutex::new(false));
        reserved_production_sessions()
            .lock()
            .unwrap()
            .insert(SID_ABORT_ALL.to_string(), "direct-write_next-t68".to_string());
        active_confirmed_tasks()
            .lock()
            .unwrap()
            .insert("direct-write_next-t68".to_string(), task_flag.clone());

        let (status, parsed) = call(
            app,
            "POST",
            &format!("/api/v1/sessions/{SID_ABORT_ALL}/abort"),
            Some("{}"),
        )
        .await;
        active_confirmed_tasks().lock().unwrap().remove("direct-write_next-t68");
        reserved_production_sessions().lock().unwrap().remove(SID_ABORT_ALL);
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["aborted"], true);
        assert!(*task_flag.lock().unwrap(), "任务 abort 句柄应已置位");

        let event = subscriber.recv().await.unwrap();
        assert_eq!(event.event, "agent:aborted");
        assert!(event.data.contains("\"aborted\":true"), "data: {}", event.data);
    }

    #[tokio::test]
    async fn abort_scope_chat_spares_task_and_stops_chat_turn() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let app = app68(rt68(&root, "http://127.0.0.1:9"));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SID_ABORT_CHAT}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 确认任务运行中 + scope=chat → 任务不动。
        let task_flag = Arc::new(Mutex::new(false));
        reserved_production_sessions()
            .lock()
            .unwrap()
            .insert(SID_ABORT_CHAT.to_string(), "direct-write_next-t68b".to_string());
        active_confirmed_tasks()
            .lock()
            .unwrap()
            .insert("direct-write_next-t68b".to_string(), task_flag.clone());
        let (status, parsed) = call(
            app.clone(),
            "POST",
            &format!("/api/v1/sessions/{SID_ABORT_CHAT}/abort"),
            Some(r#"{ "scope": "chat" }"#),
        )
        .await;
        assert_eq!(parsed["aborted"], false, "body: {parsed}");
        assert!(!*task_flag.lock().unwrap(), "scope=chat 不应置位任务句柄");

        // 聊天轮注册表条目 + scope=chat → aborted=true 且 loop 轮询句柄置位
        //（68 号连通：注册表与 loop 共享同一 Arc）。
        let chat_flag = Arc::new(Mutex::new(false));
        agent_route::running_agent_sessions().lock().unwrap().insert(
            SID_ABORT_CHAT.to_string(),
            Arc::new(Mutex::new(agent_route::AgentSessionHandle {
                abort_flag: chat_flag.clone(),
            })),
        );
        let (_status, parsed) = call(
            app,
            "POST",
            &format!("/api/v1/sessions/{SID_ABORT_CHAT}/abort"),
            Some(r#"{ "scope": "chat" }"#),
        )
        .await;
        active_confirmed_tasks().lock().unwrap().remove("direct-write_next-t68b");
        reserved_production_sessions().lock().unwrap().remove(SID_ABORT_CHAT);
        agent_route::running_agent_sessions().lock().unwrap().remove(SID_ABORT_CHAT);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["aborted"], true, "body: {parsed}");
        assert!(*chat_flag.lock().unwrap(), "聊天轮 loop 句柄应已置位");
        assert!(!*task_flag.lock().unwrap(), "任务句柄仍不应置位");
    }

    #[tokio::test]
    async fn abort_without_activity_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let app = app68(rt68(&root, "http://127.0.0.1:9"));
        let (status, parsed) = call(
            app,
            "POST",
            &format!("/api/v1/sessions/{SID_ABORT_NONE}/abort"),
            Some("{}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["aborted"], false);
    }

    #[tokio::test]
    async fn history_replay_injects_summary_dialogue_and_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, messages_log) = mock_capture_messages().await;

        // transcript：工具轮（含 toolCall/toolResult）+ 对话轮。
        write_transcript(
            &root,
            SID_REPLAY,
            vec![
                TranscriptEvent::RequestStarted {
                    version: 1,
                    session_id: SID_REPLAY.into(),
                    seq: 1,
                    timestamp: 1782989000000,
                    request_id: "r1".into(),
                    session_kind: Some(SessionKind::Chat),
                    input: "看看设定文件".into(),
                },
                msg_event(2, "r1", "user", SID_REPLAY, serde_json::json!({
                    "role": "user", "content": "看看设定文件",
                })),
                msg_event(3, "r1", "assistant", SID_REPLAY, serde_json::json!({
                    "role": "assistant",
                    "content": [{ "type": "toolCall", "id": "tc1", "name": "read", "arguments": { "path": "note.md" } }],
                })),
                msg_event(4, "r1", "toolResult", SID_REPLAY, serde_json::json!({
                    "role": "toolResult",
                    "toolCallId": "tc1",
                    "toolName": "read",
                    "content": [{ "type": "text", "text": "主角名叫林动。" }],
                })),
                TranscriptEvent::RequestCommitted {
                    version: 1,
                    session_id: SID_REPLAY.into(),
                    seq: 5,
                    timestamp: 1782989000004,
                    request_id: "r1".into(),
                },
                TranscriptEvent::RequestStarted {
                    version: 1,
                    session_id: SID_REPLAY.into(),
                    seq: 6,
                    timestamp: 1782989000005,
                    request_id: "r2".into(),
                    session_kind: Some(SessionKind::Chat),
                    input: "主角叫什么？".into(),
                },
                msg_event(7, "r2", "user", SID_REPLAY, serde_json::json!({
                    "role": "user", "content": "主角叫什么？",
                })),
                msg_event(8, "r2", "assistant", SID_REPLAY, serde_json::json!({
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "主角名叫林动。" }],
                })),
                TranscriptEvent::RequestCommitted {
                    version: 1,
                    session_id: SID_REPLAY.into(),
                    seq: 9,
                    timestamp: 1782989000008,
                    request_id: "r2".into(),
                },
            ],
        );

        let app = app68(rt68(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{SID_REPLAY}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"他还有什么特点？","sessionId":"{SID_REPLAY}","actionSource":"free-text"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");

        // 注入序：system（agent 提示词）→ summary（历史状态摘要）→ 历史
        // user/assistant 对话 → boundary（已完成的历史上下文）→ 本轮指令。
        let log = messages_log.lock().unwrap();
        let messages = log.last().expect("应捕获 LLM messages").as_array().unwrap();
        let roles: Vec<&str> = messages.iter().map(|m| m["role"].as_str().unwrap_or("")).collect();
        assert_eq!(roles.len(), 6, "messages: {messages:?}");
        assert_eq!(roles, ["system", "system", "user", "assistant", "system", "user"]);
        assert!(
            messages[1]["content"].as_str().unwrap().starts_with("[历史状态摘要]"),
            "summary: {messages:?}"
        );
        assert!(
            messages[1]["content"].as_str().unwrap().contains("- read completed — 主角名叫林动。"),
            "summary: {messages:?}"
        );
        assert_eq!(messages[2]["content"], "主角叫什么？");
        assert_eq!(messages[3]["content"], "主角名叫林动。");
        assert!(
            messages[4]["content"].as_str().unwrap().starts_with("[已完成的历史上下文]"),
            "boundary: {messages:?}"
        );
        assert_eq!(messages[5]["content"], "他还有什么特点？");
    }
}

mod films69_e2e {
    //! 69 号：interactive-films / projects 域（图谱列表 / delta rev 链 / 校验 /
    //! 分析 / 三导出 / tar.gz / 生图不可用面）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::interactive_film_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt69(root: &std::path::Path) -> BooksRuntime {
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

    fn app69(root: &std::path::Path) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/interactive-films",
                axum::routing::get(interactive_film_routes::list_interactive_films),
            )
            .route(
                "/api/v1/projects/:id/story-graph/delta",
                axum::routing::post(interactive_film_routes::post_story_graph_delta),
            )
            .route(
                "/api/v1/projects/:id/story-graph",
                axum::routing::get(interactive_film_routes::get_story_graph),
            )
            .route(
                "/api/v1/projects/:id/export",
                axum::routing::get(interactive_film_routes::get_project_export),
            )
            .route(
                "/api/v1/projects/:id/story-graph/validation",
                axum::routing::get(interactive_film_routes::get_story_graph_validation),
            )
            .route(
                "/api/v1/projects/:id/story-graph/analysis",
                axum::routing::get(interactive_film_routes::get_story_graph_analysis),
            )
            .route(
                "/api/v1/projects/:id/export/json",
                axum::routing::get(interactive_film_routes::get_export_json),
            )
            .route(
                "/api/v1/projects/:id/export/ink",
                axum::routing::get(interactive_film_routes::get_export_ink),
            )
            .route(
                "/api/v1/projects/:id/export/html",
                axum::routing::get(interactive_film_routes::get_export_html),
            )
            .route(
                "/api/v1/projects/:id/nodes/:nodeId/image",
                axum::routing::post(interactive_film_routes::post_node_image),
            )
            .with_state(rt69(root))
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
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) };
        (status, parsed)
    }

    async fn call_raw(app: axum::Router, uri: &str) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
        use tower::ServiceExt;
        let request = axum::http::Request::builder().uri(uri).body(axum::body::Body::empty()).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_str().unwrap_or("").to_string()))
            .collect();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap().to_vec();
        (status, headers, bytes)
    }

    /// 最小合法图谱（camelCase——TS 磁盘形态）。
    fn write_graph(root: &std::path::Path, id: &str, graph: serde_json::Value) {
        let dir = root.join("interactive-films").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("story-graph.json"),
            format!("{}\n", serde_json::to_string_pretty(&graph).unwrap()),
        )
        .unwrap();
    }

    fn valid_graph(id: &str, title: &str) -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "projectId": id,
            "title": title,
            "variables": [{ "name": "courage", "type": "counter", "default": 1 }],
            "nodes": [
                { "id": "start", "type": "start", "choices": [
                    { "id": "a", "text": "前进", "targetNodeId": "end1", "effects": [{ "var": "courage", "op": "add", "value": 1 }] }
                ]},
                { "id": "end1", "type": "ending" }
            ],
            "endings": [{ "id": "e1", "nodeId": "end1", "title": "终章", "type": "good" }]
        })
    }

    #[tokio::test]
    async fn list_delta_and_story_graph_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        write_graph(&root, "bbb", valid_graph("bbb", "B 影游"));
        write_graph(&root, "aaa", valid_graph("aaa", "A 影游"));
        // 无效目录：unsafe id + 空 project（无图谱）→ 都不列。
        std::fs::create_dir_all(root.join("interactive-films").join("../escape")).unwrap();
        std::fs::create_dir_all(root.join("interactive-films").join("empty")).unwrap();

        let app = app69(&root);
        let (status, parsed) = call(app.clone(), "GET", "/api/v1/interactive-films", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let films = parsed["films"].as_array().unwrap();
        assert_eq!(films.len(), 2, "films: {films:?}");
        assert_eq!(films[0]["projectId"], "aaa", "按标题排序: {films:?}");
        assert_eq!(films[0]["title"], "A 影游");

        // delta：nodes upsert + endings → rev 1 + graph；二次 → rev 2 + 快照。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/projects/bbb/story-graph/delta",
            Some(r#"{ "delta": { "nodes": { "upsert": [{ "id": "extra", "type": "normal", "choices": [{ "id": "x", "text": "进", "targetNodeId": "end1" }] }] } } }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["rev"], 1);
        assert_eq!(parsed["graph"]["nodes"].as_array().unwrap().len(), 3);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/projects/bbb/story-graph/delta",
            Some(r#"{ "delta": { "endings": { "remove": ["e1"] } } }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["rev"], 2);
        assert!(parsed["graph"]["endings"].as_array().unwrap().is_empty());

        // pre-rev 快照：0 与 1 存在（2 是活文件不快照）。
        let snapshots = root.join("interactive-films").join("bbb").join("snapshots");
        assert!(snapshots.join("0.json").is_file());
        assert!(snapshots.join("1.json").is_file());
        assert!(!snapshots.join("2.json").exists());

        // authoring-state rev 落盘。
        let state: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("interactive-films").join("bbb").join("authoring-state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(state["rev"], 2);

        // GET story-graph：原样回显（含 delta 后内容）。
        let (status, parsed) = call(app, "GET", "/api/v1/projects/bbb/story-graph", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["projectId"], "bbb");
        assert!(parsed["endings"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn delta_invalid_id_and_bad_reference() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let app = app69(&root);
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/projects/.%2Fescape/story-graph/delta",
            Some(r#"{ "delta": {} }"#),
        )
        .await;
        // 路径参数已 percent 解码为 ./escape → unsafe → INVALID_ID。
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "INVALID_ID");

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/projects/ok1/story-graph/delta",
            Some(r#"{ "delta": { "endings": { "upsert": [{ "id": "e9", "nodeId": "ghost", "title": "悬", "type": "bad" }] } } }"#),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "body: {parsed}");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("references missing node"));
    }

    #[tokio::test]
    async fn validation_and_analysis_reports() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let mut broken = valid_graph("bad1", "断链");
        broken["nodes"][0]["choices"][0]["targetNodeId"] = "ghost".into();
        write_graph(&root, "bad1", broken);

        let app = app69(&root);
        let (status, parsed) = call(app.clone(), "GET", "/api/v1/projects/bad1/story-graph/validation", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], false);
        let codes: Vec<&str> = parsed["issues"].as_array().unwrap().iter().map(|i| i["code"].as_str().unwrap()).collect();
        assert!(codes.contains(&"BROKEN_LINK"), "codes: {codes:?}");
        assert!(codes.contains(&"DEAD_END"), "codes: {codes:?}");
        assert!(codes.contains(&"NO_PATH_TO_ENDING"), "codes: {codes:?}");

        // analysis：report + arcs + distribution 三段。
        write_graph(&root, "good1", valid_graph("good1", "完好"));
        let (status, parsed) = call(app, "GET", "/api/v1/projects/good1/story-graph/analysis", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["report"]["ok"], true);
        assert_eq!(parsed["arcs"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["distribution"]["total"], 1);
        assert_eq!(parsed["distribution"]["byEnding"]["e1"], 1);

        // 缺失 → 404 NOT_FOUND。
        let (status, parsed) = call(app69(&root), "GET", "/api/v1/projects/none/story-graph/validation", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"]["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn exports_ink_html_json_and_tar_gz() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        write_graph(&root, "film1", valid_graph("film1", "样本影游"));
        // 可内嵌资产：start 无图 → 补一个带 assetRef 的节点资产（covers 前缀合法）。
        std::fs::create_dir_all(root.join("covers")).unwrap();
        std::fs::write(root.join("covers").join("pic.png"), b"\x89PNG-fake").unwrap();
        let mut graph = valid_graph("film1", "样本影游");
        graph["nodes"][1]["imageSlot"] = serde_json::json!({ "prompt": "", "assetRef": "covers/pic.png" });
        write_graph(&root, "film1", graph);

        let app = app69(&root);
        // ink
        let (status, headers, body) = call_raw(app.clone(), "/api/v1/projects/film1/export/ink").await;
        assert_eq!(status, StatusCode::OK);
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("// 样本影游 — exported from InkOS interactive film"));
        assert!(text.contains("VAR courage = 1"));
        assert!(text.contains("-> node_start"));
        assert!(headers.iter().any(|(k, v)| k == "content-type" && v == "text/plain; charset=utf-8"));
        assert!(headers.iter().any(|(k, v)| k == "content-disposition" && v.contains("film1.ink")));

        // html（资产 data URI 内嵌）
        let (status, _, body) = call_raw(app.clone(), "/api/v1/projects/film1/export/html").await;
        assert_eq!(status, StatusCode::OK);
        let html = String::from_utf8(body).unwrap();
        assert!(html.contains("if-player"));
        assert!(html.contains("data:image/png;base64,"), "资产应内嵌");

        // json（pretty + 尾换行）
        let (status, _, body) = call_raw(app.clone(), "/api/v1/projects/film1/export/json").await;
        assert_eq!(status, StatusCode::OK);
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("\"schemaVersion\": 1"));
        assert!(text.ends_with("}\n"));

        // tar.gz：gunzip + tar 头（ustar magic）
        let (status, headers, body) = call_raw(app.clone(), "/api/v1/projects/film1/export").await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers.iter().any(|(k, v)| k == "content-type" && v == "application/gzip"));
        assert!(headers.iter().any(|(k, v)| k == "content-disposition" && v.contains("film1.tar.gz")));
        use std::io::Read as _;
        let mut decoder = flate2::read::GzDecoder::new(&body[..]);
        let mut tar = Vec::new();
        decoder.read_to_end(&mut tar).unwrap();
        assert!(tar.len() > 1024, "tar 应含至少头块+尾块");
        assert_eq!(&tar[257..262], b"ustar", "ustar magic");
        let name = String::from_utf8_lossy(&tar[0..100]);
        let end = name.find('\0').unwrap_or(name.len());
        assert!(name[..end].starts_with("film1/"), "首条目名: {name:?}");

        // 项目缺失 → 404。
        let (status, parsed) = call(app, "GET", "/api/v1/projects/none/export", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"]["code"], "NOT_FOUND");
    }


    #[tokio::test]
    async fn node_image_endpoint_surface() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        write_graph(&root, "film1", valid_graph("film1", "影游"));

        let app = app69(&root);
        // 节点存在 → 生图链未接线（69 号偏差备案）→ 503。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/projects/film1/nodes/start/image",
            Some("{}"),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "IMAGE_GENERATION_UNAVAILABLE");

        // 节点缺失 → 404 NODE_NOT_FOUND。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/projects/film1/nodes/ghost/image",
            Some("{}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"]["code"], "NODE_NOT_FOUND");
    }
}

mod translations70_e2e {
    //! 70 号：translations 域六端点（upload → create → detail → run → export 全链）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::translation_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt70(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 8192,
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    fn app70(root: &std::path::Path, llm: &str) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/translations",
                axum::routing::get(translation_routes::list_translations),
            )
            .route(
                "/api/v1/translations/upload",
                axum::routing::post(translation_routes::upload_translation),
            )
            .route(
                "/api/v1/translations/create",
                axum::routing::post(translation_routes::create_translation),
            )
            .route(
                "/api/v1/translations/:id",
                axum::routing::get(translation_routes::get_translation_detail),
            )
            .route(
                "/api/v1/translations/:id/run",
                axum::routing::post(translation_routes::run_translation),
            )
            .route(
                "/api/v1/translations/:id/export",
                axum::routing::post(translation_routes::export_translation),
            )
            .with_state(rt70(root, llm))
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
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) };
        (status, parsed)
    }

    /// 翻译 LLM mock：按 system 关键词分流（Translation Agent → 逐段回译 JSON；
    /// Review Agent → passed）。
    async fn mock_translation_llm() -> String {
        let app = axum::Router::new()
            .route(
                "/chat/completions",
                axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                    let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                    let user = body["messages"][1]["content"].as_str().unwrap_or("").to_string();
                    let content = if system.contains("Translation Agent") {
                        // 从请求 JSON 抽 segments，逐段回译。
                        let parsed: serde_json::Value = serde_json::from_str(&user).unwrap_or_default();
                        let segments: Vec<serde_json::Value> = parsed["segments"]
                            .as_array()
                            .map(|items| {
                                items
                                    .iter()
                                    .map(|segment| {
                                        serde_json::json!({
                                            "index": segment["index"],
                                            "target": format!("[译]{}", segment["source"].as_str().unwrap_or("")),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        serde_json::json!({
                            "segments": segments,
                            "glossary": [{ "source": "Mana", "target": "魔力" }],
                        })
                        .to_string()
                    } else if system.contains("Translation Review Agent") {
                        serde_json::json!({ "passed": true, "summary": "全部通过。", "issues": [] }).to_string()
                    } else {
                        "PASS".to_string()
                    };
                    let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    /// 上传并创建一个翻译项目，返回 projectId。
    async fn seed_project(root: &std::path::Path, app: &axum::Router) -> String {
        let novel = "Chapter 1\n\nThe mountain stood tall.\n\nMana flowed.\n";
        use base64::Engine as _;
        let data_url = format!(
            "data:text/plain;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(novel)
        );
        let (status, upload) = call(
            app.clone(),
            "POST",
            "/api/v1/translations/upload",
            Some(&serde_json::json!({ "filename": "novel.txt", "dataUrl": data_url }).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {upload}");
        let stored_path = upload["storedPath"].as_str().unwrap().to_string();
        assert!(stored_path.starts_with(".inkos/uploads/translation/"), "{stored_path}");
        assert!(stored_path.ends_with("-novel.txt"), "{stored_path}");
        assert!(root.join(&stored_path).is_file());
        assert_eq!(upload["mimeType"], "text/plain");

        let (status, created) = call(
            app.clone(),
            "POST",
            "/api/v1/translations/create",
            Some(&serde_json::json!({
                "filePath": stored_path,
                "sourceLanguage": "en",
                "targetLanguage": "zh",
                "title": "山河之书"
            }).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {created}");
        assert_eq!(created["title"], "山河之书");
        created["projectId"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn upload_create_detail_run_export_full_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let llm = mock_translation_llm().await;
        let app = app70(&root, &llm);
        let project_id = seed_project(&root, &app).await;

        // detail：manifest + 空报告 + 章节段（target 空）。
        let (status, detail) = call(
            app.clone(),
            "GET",
            &format!("/api/v1/translations/{project_id}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {detail}");
        assert_eq!(detail["manifest"]["title"], "山河之书");
        assert_eq!(detail["manifest"]["chapters"].as_array().unwrap().len(), 1);
        assert_eq!(detail["manifest"]["chapters"][0]["status"], "pending");
        assert_eq!(detail["chapters"][0]["segments"].as_array().unwrap().len(), 2);
        assert_eq!(detail["chapters"][0]["segments"][0]["target"], "");
        assert_eq!(detail["report"], "# Translation Review\n\nPending.\n");

        // run：翻译 + 评审全链。
        let (status, run) = call(
            app.clone(),
            "POST",
            &format!("/api/v1/translations/{project_id}/run"),
            Some("{}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {run}");
        assert_eq!(run["translatedSegments"], 2);
        assert_eq!(run["reviewedChapters"], 1);
        assert_eq!(run["reportPath"], format!("translations/{project_id}/review-report.md"));

        // detail：状态推进 + 段回填 + 报告更新 + 术语表。
        let (status, detail) = call(
            app.clone(),
            "GET",
            &format!("/api/v1/translations/{project_id}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(detail["manifest"]["chapters"][0]["status"], "reviewed");
        assert_eq!(detail["chapters"][0]["segments"][0]["target"], "[译]The mountain stood tall.");
        assert!(detail["report"].as_str().unwrap().contains("- passed: yes"));
        let glossary_raw = std::fs::read_to_string(
            root.join("translations").join(&project_id).join("glossary.json"),
        )
        .unwrap();
        assert!(glossary_raw.contains("魔力"), "{glossary_raw}");

        // export：md + epub（zip 读回）。
        let (status, exported) = call(
            app.clone(),
            "POST",
            &format!("/api/v1/translations/{project_id}/export"),
            Some(r#"{ "format": "md" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {exported}");
        assert_eq!(exported["format"], "md");
        assert_eq!(exported["chaptersExported"], 1);
        let md_path = exported["outputPath"].as_str().unwrap();
        let md = std::fs::read_to_string(root.join(md_path)).unwrap();
        assert!(md.contains("# 山河之书"), "{md}");
        assert!(md.contains("[译]Mana flowed."), "{md}");

        let (status, exported) = call(
            app.clone(),
            "POST",
            &format!("/api/v1/translations/{project_id}/export"),
            Some(r#"{ "format": "epub" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let epub_path = exported["outputPath"].as_str().unwrap();
        assert!(epub_path.ends_with(".epub"));
        let epub_bytes = std::fs::read(root.join(epub_path)).unwrap();
        let mut reader = zip::ZipArchive::new(std::io::Cursor::new(&epub_bytes[..])).unwrap();
        assert_eq!(
            reader.by_name("mimetype").unwrap().compression(),
            zip::CompressionMethod::Stored
        );
        let mut opf = String::new();
        {
            let mut archive = reader.clone();
            let mut file = archive.by_name("OEBPS/content.opf").unwrap();
            std::io::Read::read_to_string(&mut file, &mut opf).unwrap();
        }
        assert!(opf.contains("山河之书"), "{opf}");

        // 列表：摘要形态。
        let (status, listed) = call(app, "GET", "/api/v1/translations", None).await;
        assert_eq!(status, StatusCode::OK);
        let translations = listed["translations"].as_array().unwrap();
        assert_eq!(translations.len(), 1);
        assert_eq!(translations[0]["projectId"], project_id);
        assert_eq!(translations[0]["title"], "山河之书");
        assert_eq!(translations[0]["chapters"], 1);
    }

    #[tokio::test]
    async fn validation_and_error_surface() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let llm = mock_translation_llm().await;
        let app = app70(&root, &llm);

        // upload 缺 dataUrl → 400。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/translations/upload",
            Some(r#"{ "filename": "x.txt" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_TRANSLATION_UPLOAD");

        // create 缺 filePath / 缺语言 → 400。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/translations/create",
            Some(r#"{ "sourceLanguage": "en" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "MISSING_FILE_PATH");
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/translations/create",
            Some(r#"{ "filePath": "a.txt", "sourceLanguage": " " }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "MISSING_LANGUAGES");

        // detail：非法 id / 缺失项目。
        // percent 编码的单段 id（../etc）命中 :id 路由 → INVALID_ID；多段
        // ../.. 不匹配单段路由 → 404（TS Hono 同行为）。
        let (status, parsed) = call(app.clone(), "GET", "/api/v1/translations/%2e%2e%2fetc", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "INVALID_ID");
        let (status, parsed) = call(app.clone(), "GET", "/api/v1/translations/ghost", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(parsed["error"]["code"], "NOT_FOUND");

        // export 非法 format → 400（TS 会按 txt 兜底，Rust 显式拒绝——偏差备案）。
        let project_id = seed_project(&root, &app).await;
        let (status, parsed) = call(
            app.clone(),
            "POST",
            &format!("/api/v1/translations/{project_id}/export"),
            Some(r#"{ "format": "pdf" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"]["code"], "INVALID_FORMAT");
    }

    #[tokio::test]
    async fn run_upstream_failure_maps_502() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // LLM 端口不可达 → chat 错误 → 上游分类 502。
        let app = app70(&root, "http://127.0.0.1:9");
        let project_id = seed_project(&root, &app).await;
        let (status, parsed) = call(
            app,
            "POST",
            &format!("/api/v1/translations/{project_id}/run"),
            Some("{}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "TRANSLATION_RUN_FAILED");
    }
}
