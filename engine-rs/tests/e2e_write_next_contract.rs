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

        // schema 校验失败（name 缺失 / llm.model 空串）同样 500。
        std::fs::write(root.join("inkos.json"), r#"{ "llm": { "provider": "custom", "baseUrl": "https://x.io", "model": "m" } }"#).unwrap();
        let (status, parsed) = call(app56(rt56(&root)), "GET", "/api/v1/project", None).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(parsed["error"]["code"], "PROJECT_CONFIG_INVALID");
        std::fs::write(
            root.join("inkos.json"),
            r#"{ "name": "d", "llm": { "provider": "custom", "baseUrl": "https://x.io", "model": "" } }"#,
        )
        .unwrap();
        let (status, _) = call(app56(rt56(&root)), "GET", "/api/v1/project", None).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
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

    const ARCHITECT_OUTPUT: &str = r#"=== SECTION: story_frame ===
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

    const REVIEW_PASS: &str = "\
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

    async fn spawn_mock58() -> String {
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
