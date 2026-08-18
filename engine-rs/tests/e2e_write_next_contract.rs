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

    #[tokio::test]
    async fn revise_book_level_always_gate_overrides_project_default() {
        // 116 号：book.writing.revisionGate=always——同恶化审计下 strict 拒绝而
        // always 应用（book 覆盖 project 链生效证据）。
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture47(&root);
        // book 级 always（项目级未设 → 链：book.always）。
        let book_config_path = root.join("books").join("b1").join("book.json");
        let mut book: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&book_config_path).unwrap()).unwrap();
        book["writing"] = serde_json::json!({ "revisionGate": "always" });
        std::fs::write(&book_config_path, book.to_string()).unwrap();
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
        assert_eq!(parsed["applied"], true, "body: {parsed}");
        // 应用路径无 revisionDiagnostics（拒绝路径专属——88 号形态）。
        // 应用后章节文件含修订文本。
        let saved = std::fs::read_to_string(root.join("books").join("b1").join("chapters").join("0001_风起.md"))
            .unwrap();
        assert!(saved.contains("今日起讨回"), "{saved}");
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
    async fn review_mode_missing_writing_key_falls_back_to_auto() {
        // 118 号对跑勘误：writing.reviewMode 键缺失 → 200 auto（TS
        // readProjectChapterReviewMode 默认），404 仅限 inkos.json/book.json 文件缺失。
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture48(&root);
        std::fs::write(root.join("inkos.json"), r#"{ "name": "x" }"#).unwrap();
        let (status, parsed) =
            call(app48(rt48(&root)), "GET", "/api/v1/books/b1/chapter-review-mode", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["mode"], "auto");
        assert_eq!(parsed["projectMode"], "auto");
        assert!(parsed["bookMode"].is_null(), "body: {parsed}");
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
            r#"{"id":"b1","title":"诊断书","platform":"other","genre":"xianxia","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
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
                            // 首轮：发起 read 工具调用（books/ 作用域路径）
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_read_1", "function": { "name": "read", "arguments": "{\"path\":\"b1/note.md\"}" } },
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
        // 105 号：read 为书会话工具（books/ 作用域）——夹具走 books/b1/。
        std::fs::create_dir_all(root.join("books").join("b1")).unwrap();
        std::fs::write(
            root.join("books").join("b1").join("book.json"),
            r#"{"id":"b1","title":"测试书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\n---\n正文指导\n",
        )
        .unwrap();
        std::fs::write(root.join("books").join("b1").join("note.md"), "这是主角设定草稿。").unwrap();
        let (llm, _guard) = mock_tool_llm().await;
        let _ = call(
            app66(&root, &llm),
            "POST",
            "/api/v1/sessions",
            Some(&format!(
                r#"{{"sessionId":"{SESSION_ID}","bookId":"b1","sessionKind":"book"}}"#
            )),
        )
        .await;

        let app = app66(&root, &llm);
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"看看设定笔记写了什么","sessionId":"{SESSION_ID}","activeBookId":"b1"}}"#
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
    async fn short_run_invalid_draft_and_missing_title_fail_with_502() {
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

        // short_run 已接线（78 号）：mock LLM 无有效章块 → 整稿校验失败 →
        // 502 统一错误面（空章列表文案）。
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
        assert!(
            parsed["error"]["message"].as_str().unwrap().contains("Short-hit draft is incomplete"),
            "body: {parsed}"
        );

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
        // 节点存在但无 prompt/sceneDesc 且无 cover 配置 → TS generateNodeImage
        // throw 消息逐字（端点 500）；74 号起生图链已接通，此 fixture 无
        // prompt 源先命中该分支。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/projects/film1/nodes/start/image",
            Some("{}"),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "body: {parsed}");
        assert!(
            parsed["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("has no imageSlot.prompt or sceneDesc"),
            "body: {parsed}"
        );

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

mod play71_e2e {
    //! 71 号：play 域四端点（run 聚合 + 插图 sidecar + 生图兜底 + 图片回读）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::play_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt71(root: &std::path::Path) -> BooksRuntime {
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

    fn app71(root: &std::path::Path) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/play/runs/:worldId/:runId",
                axum::routing::get(play_routes::get_play_run),
            )
            .route(
                "/api/v1/play/runs/:worldId/:runId/image-settings",
                axum::routing::put(play_routes::put_play_image_settings),
            )
            .route(
                "/api/v1/play/runs/:worldId/:runId/generate-image",
                axum::routing::post(play_routes::post_play_generate_image),
            )
            .route(
                "/api/v1/play/runs/:worldId/:runId/images/:file",
                axum::routing::get(play_routes::get_play_image),
            )
            .with_state(rt71(root))
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

    /// world + run fixture：world.json / transcript / current / play-graph.json /
    /// 插图 manifest + settings + 一张实体图。
    fn fixture(root: &std::path::Path) {
        let world_dir = root.join("worlds").join("w1");
        let run_dir = world_dir.join("runs").join("r1");
        std::fs::create_dir_all(run_dir.join("images")).unwrap();
        std::fs::create_dir_all(run_dir.join("state")).unwrap();
        std::fs::create_dir_all(run_dir.join("projections")).unwrap();
        std::fs::write(
            world_dir.join("world.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "w1", "title": "雪夜世界", "premise": "大雪封山的世界",
                "worldContract": "", "visualContract": "水墨", "mode": "open",
                "language": "zh", "createdAt": "t", "updatedAt": "t"
            })).unwrap(),
        )
        .unwrap();
        std::fs::write(
            run_dir.join("transcript.jsonl"),
            "{\"role\":\"user\",\"content\":\"进屋\",\"timestamp\":1}\n{\"role\":\"assistant\",\"content\":\"你推门而入。\",\"timestamp\":2}\n",
        )
        .unwrap();
        std::fs::write(
            run_dir.join("state").join("current.json"),
            serde_json::to_string_pretty(&serde_json::json!({ "turn": 3, "location": "老宅" })).unwrap(),
        )
        .unwrap();
        std::fs::write(
            run_dir.join("projections").join("scene.md"),
            "雪夜，孤灯摇曳。",
        )
        .unwrap();
        std::fs::write(
            run_dir.join("play-graph.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "entities": {
                    "hero": { "id": "hero", "type": "actor", "label": "林动", "summary": "少年" },
                    "npc": { "id": "npc", "type": "actor", "label": "老者", "summary": "" }
                },
                "edges": {}, "stateSlots": {},
                "events": { "e1": { "id": "e1", "turn": 1, "actionKind": "user_input", "rawInput": "进屋", "createdAt": "t" } }
            })).unwrap(),
        )
        .unwrap();
        std::fs::write(
            run_dir.join("images").join("manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "hero": { "status": "ready", "file": "hero.png" },
                "npc": { "status": "failed", "error": "boom" },
                "scene-turn-3": { "status": "ready", "file": "scene-turn-3.png" },
                "scene-turn-1": { "status": "ready", "file": "scene-turn-1.png" }
            })).unwrap(),
        )
        .unwrap();
        std::fs::write(run_dir.join("images").join("hero.png"), b"\x89PNG-hero").unwrap();
        std::fs::write(run_dir.join("images").join("scene-turn-3.png"), b"\x89PNG-scene").unwrap();
    }

    #[tokio::test]
    async fn get_run_merges_graph_transcript_and_images() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root);

        let (status, parsed) = call(app71(&root), "GET", "/api/v1/play/runs/w1/r1", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["worldId"], "w1");
        assert_eq!(parsed["runId"], "r1");
        assert_eq!(parsed["title"], "雪夜世界");
        assert_eq!(parsed["transcript"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["currentState"]["turn"], 3);
        assert_eq!(parsed["graph"]["entities"].as_array().unwrap().len(), 2);

        // 实体插图注入：hero ready 有图；npc failed 无图。
        let entities = parsed["graph"]["entities"].as_array().unwrap();
        let hero = entities.iter().find(|e| e["id"] == "hero").unwrap();
        assert_eq!(
            hero["imageUrl"],
            "/api/v1/play/runs/w1/r1/images/hero.png",
            "hero: {hero}"
        );
        let npc = entities.iter().find(|e| e["id"] == "npc").unwrap();
        assert!(npc.get("imageUrl").is_none(), "npc: {npc}");

        // scene-turn-* URL 表 + 当前回合插图（turn=3）。
        assert_eq!(
            parsed["sceneImageUrls"]["scene-turn-3"],
            "/api/v1/play/runs/w1/r1/images/scene-turn-3.png"
        );
        assert_eq!(
            parsed["sceneImageUrl"],
            "/api/v1/play/runs/w1/r1/images/scene-turn-3.png"
        );
        // 默认全关的插图开关。
        assert_eq!(parsed["imageSettings"]["actors"], false);
    }

    #[tokio::test]
    async fn get_run_reads_sqlite_backend() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let run_dir = root.join("worlds").join("w1").join("runs").join("r1");
        std::fs::create_dir_all(&run_dir).unwrap();
        {
            let conn = rusqlite::Connection::open(run_dir.join("play.db")).unwrap();
            conn.execute_batch(
                "CREATE TABLE entities (id TEXT PRIMARY KEY, type TEXT, label TEXT, summary TEXT, status TEXT, created_event TEXT, updated_event TEXT);
                 CREATE TABLE edges (id TEXT PRIMARY KEY, from_id TEXT, type TEXT, to_id TEXT, value_json TEXT, valid_from_event TEXT, valid_until_event TEXT, source_event_id TEXT, visibility_json TEXT, strength REAL, confidence REAL);
                 CREATE TABLE state_slots (id TEXT PRIMARY KEY, owner_entity_id TEXT, kind TEXT, label TEXT, value_json TEXT, updated_event TEXT);
                 CREATE TABLE events (id TEXT PRIMARY KEY, turn INTEGER, action_kind TEXT, raw_input TEXT, outcome_summary TEXT, created_at TEXT);
                 INSERT INTO entities VALUES ('hero','actor','林动','','','','');",
            )
            .unwrap();
        }
        let (status, parsed) = call(app71(&root), "GET", "/api/v1/play/runs/w1/r1", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["graph"]["entities"][0]["label"], "林动");
        assert_eq!(parsed["title"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn image_settings_roundtrip_and_generate_surface() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root);

        // PUT：三开关覆写（缺 inventory → false）。
        let (status, parsed) = call(
            app71(&root),
            "PUT",
            "/api/v1/play/runs/w1/r1/image-settings",
            Some(r#"{ "actors": true, "moments": true }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["imageSettings"]["actors"], true);
        assert_eq!(parsed["imageSettings"]["inventory"], false);
        let written = std::fs::read_to_string(
            root.join("worlds").join("w1").join("runs").join("r1").join("images").join("settings.json"),
        )
        .unwrap();
        assert!(written.contains("\"actors\": true"), "{written}");

        // generate-image：缺 entityId → 400 平铺。
        let (status, parsed) = call(
            app71(&root),
            "POST",
            "/api/v1/play/runs/w1/r1/generate-image",
            Some(r#"{ "target": "entity" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "entityId is required for an entity image");

        // 实体不存在 → 404。
        let (status, parsed) = call(
            app71(&root),
            "POST",
            "/api/v1/play/runs/w1/r1/generate-image",
            Some(r#"{ "target": "entity", "entityId": "ghost" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "body: {parsed}");
        assert_eq!(parsed["error"], "entity not found: ghost");

        // 实体存在但生图链未接线 → needsCoverConfig 兜底（TS 未配置分支同形）。
        let (status, parsed) = call(
            app71(&root),
            "POST",
            "/api/v1/play/runs/w1/r1/generate-image",
            Some(r#"{ "target": "entity", "entityId": "hero" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["needsCoverConfig"], true);

        // scene 无文本且无投影 → 400。
        let empty_dir = tempfile::tempdir().unwrap();
        let empty_root = empty_dir.path().to_path_buf();
        std::fs::create_dir_all(empty_root.join("worlds").join("w1").join("runs").join("r1")).unwrap();
        let (status, parsed) = call(
            app71(&empty_root),
            "POST",
            "/api/v1/play/runs/w1/r1/generate-image",
            Some(r#"{ "target": "scene" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "no current scene to illustrate");
    }

    #[tokio::test]
    async fn image_file_serving_and_validation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture(&root);

        let app = app71(&root);
        // png 回读 + content-type。
        let (status, headers, body) = {
            use tower::ServiceExt;
            let request = axum::http::Request::builder()
                .uri("/api/v1/play/runs/w1/r1/images/hero.png")
                .body(axum::body::Body::empty())
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            let status = response.status();
            let content_type = response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap().to_vec();
            (status, content_type, bytes)
        };
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.as_deref(), Some("image/png"));
        assert_eq!(body, b"\x89PNG-hero");

        // 路径穿越 / 缺失。
        let (status, parsed) = call(app.clone(), "GET", "/api/v1/play/runs/w1/r1/images/%2e%2e%2fhero.png", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["error"], "Invalid image file");
        let (status, _) = call(app, "GET", "/api/v1/play/runs/w1/r1/images/ghost.png", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // 非法 worldId 段（斜杠）→ 400 INVALID_BOOK_ID（normalizeApiBookId 同码）。
        let (status, parsed) = call(app71(&root), "GET", "/api/v1/play/runs/%2e%2e/r1", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "INVALID_BOOK_ID");
    }
}

mod ops72_e2e {
    //! 72 号：daemon 生命周期 / logs / doctor / radar / foundation revise 收官面。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::book_create_routes::revise_foundation;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::ops_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt72(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app72_with(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/daemon", axum::routing::get(ops_routes::get_daemon))
            .route("/api/v1/daemon/start", axum::routing::post(ops_routes::post_daemon_start))
            .route("/api/v1/daemon/stop", axum::routing::post(ops_routes::post_daemon_stop))
            .route("/api/v1/logs", axum::routing::get(ops_routes::get_logs))
            .route("/api/v1/doctor", axum::routing::get(ops_routes::get_doctor))
            .route("/api/v1/radar/scan", axum::routing::post(ops_routes::post_radar_scan))
            .route("/api/v1/radar/history", axum::routing::get(ops_routes::get_radar_history))
            .route("/api/v1/books/:id/foundation/revise", axum::routing::post(revise_foundation))
            .with_state(runtime)
    }

    fn app72(root: &std::path::Path, llm: &str) -> axum::Router {
        app72_with(rt72(root, llm))
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

    /// 雷达 + 架构 + 审核 mock（按 system 关键词分流；榜单外网抓取由 5s 预算容错）。
    async fn mock_llm() -> String {
        let app = axum::Router::new()
            .route(
                "/chat/completions",
                axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                    let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                    let content = if system.contains("网络小说市场分析师") {
                        serde_json::json!({
                            "recommendations": [
                                { "platform": "番茄小说", "genre": "都市脑洞", "concept": "外卖员觉醒系统", "confidence": 0.82 }
                            ],
                            "marketSummary": "都市脑洞热度持续。"
                        })
                        .to_string()
                    } else if system.contains("网络小说架构师") || system.contains("总架构师") {
                        super::books58_e2e::ARCHITECT_OUTPUT.to_string()
                    } else if system.contains("资深小说编辑") {
                        super::books58_e2e::REVIEW_PASS.to_string()
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

    #[tokio::test]
    async fn daemon_lifecycle_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let runtime = rt72(&root, "http://127.0.0.1:9");
        let mut subscriber = runtime.hub.subscribe();
        let app = app72_with(runtime);

        // 初始 idle。
        let (status, parsed) = call(app.clone(), "GET", "/api/v1/daemon", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["running"], false);

        // start → {ok,running:true} + daemon:started；无书写循环空转不报错。
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/daemon/start", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["running"], true);
        assert_eq!(subscriber.recv().await.unwrap().event, "daemon:started");

        let (_status, parsed) = call(app.clone(), "GET", "/api/v1/daemon", None).await;
        assert_eq!(parsed["running"], true);

        // 重复 start → 400。
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/daemon/start", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["error"], "Daemon already running");

        // stop → daemon:stopped；重复 stop → 400。
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/daemon/stop", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["running"], false);
        assert_eq!(subscriber.recv().await.unwrap().event, "daemon:stopped");
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/daemon/stop", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "Daemon not running");
        let (_, parsed) = call(app, "GET", "/api/v1/daemon", None).await;
        assert_eq!(parsed["running"], false);
    }

    #[tokio::test]
    async fn logs_tail_json_and_plain_lines() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(
            root.join("inkos.log"),
            "{\"level\":\"info\",\"message\":\"first\"}\nplain line 两\n{\"level\":\"warn\",\"message\":\"last\"}\n",
        )
        .unwrap();
        let (status, parsed) = call(app72(&root, "http://127.0.0.1:9"), "GET", "/api/v1/logs", None).await;
        assert_eq!(status, StatusCode::OK);
        let entries = parsed["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0]["message"], "first");
        assert_eq!(entries[1]["message"], "plain line 两");
        assert!(entries[1].get("level").is_none(), "裸行仅 message: {entries:?}");
        assert_eq!(entries[2]["level"], "warn");

        // 缺失文件 → 空表。
        let empty = tempfile::tempdir().unwrap();
        let (status, parsed) = call(app72(empty.path(), "http://127.0.0.1:9"), "GET", "/api/v1/logs", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["entries"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn radar_scan_and_history_full_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 预置一条历史。
        std::fs::create_dir_all(root.join("radar")).unwrap();
        std::fs::write(
            root.join("radar").join("scan-2026-08-01T00-00-00-000Z.json"),
            r#"{ "timestamp": "2026-08-01T00:00:00.000Z", "marketSummary": "历史扫描", "recommendations": [] }"#,
        )
        .unwrap();
        let llm = mock_llm().await;
        let runtime = rt72(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();
        let app = app72_with(runtime);

        let (status, parsed) = call(app.clone(), "POST", "/api/v1/radar/scan", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["marketSummary"], "都市脑洞热度持续。");
        assert_eq!(parsed["recommendations"].as_array().unwrap().len(), 1);
        assert!(parsed["timestamp"].as_str().is_some_and(|t| t.ends_with('Z')));
        assert_eq!(subscriber.recv().await.unwrap().event, "radar:start");
        assert_eq!(subscriber.recv().await.unwrap().event, "radar:complete");

        // 历史两条（新扫描 file 名更大 → 降序在前）。
        let (status, parsed) = call(app, "GET", "/api/v1/radar/history", None).await;
        assert_eq!(status, StatusCode::OK);
        let items = parsed["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "items: {items:?}");
        assert_eq!(items[0]["marketSummary"], "都市脑洞热度持续。");
        assert_eq!(items[1]["summaryPreview"], "历史扫描");
        assert!(items[0]["file"].as_str().unwrap().starts_with("scan-2026-08-1"));
    }

    #[tokio::test]
    async fn doctor_checks_file_surface_and_book_count() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("inkos.json"), "{}").unwrap();
        std::fs::write(root.join(".env"), "INKOS_LLM_API_KEY=x").unwrap();
        std::fs::create_dir_all(root.join("books").join("b1")).unwrap();
        std::fs::write(
            root.join("books").join("b1").join("book.json"),
            r#"{"id":"b1","title":"诊断书","platform":"other","genre":"xianxia","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();

        // LLM 不可达：3s 预算内判 false（不挂起）。
        let started = std::time::Instant::now();
        let (status, parsed) = call(app72(&root, "http://127.0.0.1:9"), "GET", "/api/v1/doctor", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert!(started.elapsed() < std::time::Duration::from_secs(6), "doctor 应有界: {:?}", started.elapsed());
        assert_eq!(parsed["inkosJson"], true);
        assert_eq!(parsed["projectEnv"], true);
        assert_eq!(parsed["booksDir"], true);
        assert_eq!(parsed["bookCount"], 1);
        assert_eq!(parsed["llmConnected"], false, "body: {parsed}");
    }

    #[tokio::test]
    async fn foundation_revise_backs_up_and_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        // phase4 legacy 四文件。
        let story = root.join("books").join("b1").join("story");
        std::fs::create_dir_all(&story).unwrap();
        std::fs::write(story.join("story_bible.md"), "旧圣经").unwrap();
        std::fs::write(story.join("volume_outline.md"), "旧卷纲").unwrap();
        std::fs::write(story.join("book_rules.md"), "旧规则").unwrap();
        std::fs::write(story.join("character_matrix.md"), "旧人物").unwrap();
        let llm = mock_llm().await;
        let runtime = rt72(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();
        let app = app72_with(runtime);

        // 缺 feedback → 400 平铺。
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/books/b1/foundation/revise", Some("{}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(parsed["error"], "feedback is required");

        // 修订成功：备份 + 新布局落盘 + 广播。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/books/b1/foundation/revise",
            Some(r#"{ "feedback": "加强群像" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true);
        assert!(story.join("outline").join("story_frame.md").is_file());
        let backup_dirs: Vec<_> = std::fs::read_dir(&story)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".backup-phase4-"))
            .collect();
        assert_eq!(backup_dirs.len(), 1, "应有 phase4 备份目录");
        let backup = backup_dirs[0].path();
        assert_eq!(std::fs::read_to_string(backup.join("story_bible.md")).unwrap(), "旧圣经");
        assert_eq!(subscriber.recv().await.unwrap().event, "foundation:revised");

        // 书缺失 → 500 + foundation:error。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/books/ghost/foundation/revise",
            Some(r#"{ "feedback": "x" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "body: {parsed}");
        assert!(parsed["error"].is_string());
    }

    #[tokio::test]
    async fn daemon_write_cycle_broadcasts_real_chapter_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, _calls, _guard) = spawn_mock_llm().await;
        let runtime = rt72(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();
        let app = app72_with(runtime);

        let (status, _) = call(app.clone(), "POST", "/api/v1/daemon/start", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(subscriber.recv().await.unwrap().event, "daemon:started");

        // 首轮写循环立即执行 → daemon:chapter 带真实章号/状态（104 号——
        // TS onChapterComplete(bookId, result.chapterNumber, result.status)）。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let data = loop {
            assert!(
                std::time::Instant::now() < deadline,
                "20s 内应收到 daemon:chapter"
            );
            let event = tokio::time::timeout(std::time::Duration::from_secs(10), subscriber.recv())
                .await
                .expect("等待事件超时")
                .unwrap();
            if event.event == "daemon:chapter" {
                break event.data;
            }
        };
        assert!(data.contains("\"bookId\":\"b1\""), "data: {data}");
        assert!(data.contains("\"chapter\":1"), "真实章号而非定值 0：{data}");
        assert!(
            data.contains("\"status\":\"ready-for-review\""),
            "data: {data}"
        );

        // 章节真实落盘（广播与磁盘一致）。
        let chapters: Vec<String> = std::fs::read_dir(root.join("books").join("b1").join("chapters"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".md"))
            .collect();
        assert_eq!(chapters.len(), 1, "chapters: {chapters:?}");

        let (status, _) = call(app, "POST", "/api/v1/daemon/stop", None).await;
        assert_eq!(status, StatusCode::OK);
    }
}

mod play73_e2e {
    //! 73 号：play_start 确认意图执行器 + 写入面 + runner 回合全链。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt73(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app73(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .route(
                "/api/v1/play/runs/:worldId/:runId",
                axum::routing::get(inkos_engine::server::play_routes::get_play_run),
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

    /// play 三代理 mock：interpreter（look）/ mutator（开场播种/回合推进 JSON）/
    /// renderer（sceneText + suggestedActions），按 system 关键词分流。
    async fn mock_play_llm() -> String {
        let app = axum::Router::new()
            .route(
                "/chat/completions",
                axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                    let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                    let user = body["messages"][1]["content"].as_str().unwrap_or("").to_string();
                    let content = if system.contains("动作理解器") {
                        serde_json::json!({ "actionKind": "look", "intent": "环顾四周", "secondaryActions": [] }).to_string()
                    } else if system.contains("世界状态草案员") {
                        if user.contains("只播种这个互动世界开场已经成立的状态") {
                            serde_json::json!({
                                "eventId": "evt-0", "turn": 0, "actionKind": "look",
                                "summary": "开场状态已播种",
                                "entities": { "upsert": [
                                    { "id": "actor_player", "type": "actor", "label": "夜行人", "summary": "潜入宅邸的人", "updatedEventId": "evt-0" },
                                    { "id": "item_lantern", "type": "item", "label": "灯笼", "summary": "手里的灯笼", "updatedEventId": "evt-0" }
                                ]},
                                "edges": { "upsert": [
                                    { "fromId": "actor_player", "type": "持有", "toId": "item_lantern",
                                      "value": { "role": "holding" }, "validFromEventId": "evt-0", "sourceEventId": "evt-0" }
                                ]}
                            }).to_string()
                        } else {
                            serde_json::json!({
                                "eventId": "evt-1", "turn": 1, "actionKind": "look",
                                "summary": "玩家看清了厅堂",
                                "timeAdvance": { "elapsed": "片刻", "anchor": "深夜", "rationale": "只是环顾", "synchronized": [] },
                                "entities": { "upsert": [
                                    { "id": "location_hall", "type": "location", "label": "厅堂", "summary": "正厅", "updatedEventId": "evt-1" }
                                ]}
                            }).to_string()
                        }
                    } else if system.contains("互动小说场景") {
                        serde_json::json!({
                            "sceneText": "灯笼的光晃了一下，厅堂深处有人影一闪。",
                            "suggestedActions": ["追上去", "吹灭灯笼"]
                        }).to_string()
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

    #[tokio::test]
    async fn play_start_confirmed_action_full_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let llm = mock_play_llm().await;
        let runtime = rt73(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();
        let app = app73(runtime);

        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782991000000-play01"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // play_start 确认意图（button）→ 建世界 + 首场景 + 开场播种。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{
                "instruction": "开一个雪夜古宅的世界",
                "sessionId": "1782991000000-play01",
                "actionSource": "button",
                "requestedIntent": "play_start",
                "playMode": "guided",
                "actionPayload": { "playStart": {
                    "title": "雪夜古宅",
                    "premise": "大雪封山的旧宅，藏着一段旧案。",
                    "suggestedActions": ["查看门厅", "上二楼"]
                }}
            }"#.replace(char::is_whitespace, " ").as_str()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        // play_start 在 suppressManualTextForTool 表 → response 空。
        assert_eq!(parsed["response"], "");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "play_start");
        assert_eq!(exec["status"], "completed");
        assert_eq!(exec["args"]["title"], "雪夜古宅");
        let details = &exec["details"];
        assert_eq!(details["kind"], "play_world_started");
        assert_eq!(details["worldId"], "1782991000000-play01");
        assert_eq!(details["runId"], "main");
        assert_eq!(details["mode"], "guided");
        assert_eq!(details["sceneText"], "你进入「雪夜古宅」。\n大雪封山的旧宅，藏着一段旧案。");
        assert_eq!(details["suggestedActions"].as_array().unwrap().len(), 2);
        // 开场播种成功 → 图谱带 actor_player + holding 边。
        let graph = &details["graph"];
        assert!(graph["entities"]
            .as_array()
            .is_some_and(|entities| entities.iter().any(|e| e["id"] == "actor_player")), "graph: {graph}");
        assert_eq!(parsed["session"]["sessionId"], "1782991000000-play01");

        // 磁盘形态：world.json + transcript + current state + scene 投影 + 图（file 后端）。
        let world_json = root.join("worlds").join("1782991000000-play01").join("world.json");
        assert!(world_json.is_file());
        let run = root.join("worlds").join("1782991000000-play01").join("runs").join("main");
        assert!(run.join("transcript.jsonl").is_file());
        assert!(run.join("state").join("current.json").is_file());
        assert!(run.join("projections").join("scene.md").is_file());
        assert!(run.join("projections").join("state.md").is_file());
        assert!(run.join("play-graph.json").is_file());
        let transcript = std::fs::read_to_string(run.join("transcript.jsonl")).unwrap();
        assert!(transcript.contains("雪夜古宅"), "{transcript}");

        // 71 号读取面（GET run）读取同一 world：图谱合并 + 场景插图位。
        let (status, run_view) = call(
            app,
            "GET",
            "/api/v1/play/runs/1782991000000-play01/main",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {run_view}");
        assert_eq!(run_view["title"], "雪夜古宅");
        assert!(run_view["graph"]["entities"]
            .as_array()
            .is_some_and(|entities| entities.iter().any(|e| e["id"] == "actor_player")));

        let first = subscriber.recv().await.unwrap();
        assert_eq!(first.event, "agent:start");
    }

    #[tokio::test]
    async fn play_start_missing_title_fails_with_502() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let llm = mock_play_llm().await;
        let app = app73(rt73(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782991000001-play02"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"开世界","sessionId":"1782991000001-play02","actionSource":"button","requestedIntent":"play_start"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "AGENT_ACTION_FAILED");
        assert!(
            parsed["error"]["message"].as_str().unwrap().contains("缺少标题"),
            "body: {parsed}"
        );
    }

    #[tokio::test]
    async fn runner_step_persists_turn_all_or_nothing() {
        use inkos_engine::play_runner::{PlayAgents, PlayRunner};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let llm = mock_play_llm().await;
        let runtime = rt73(&root, &llm);

        // 先建世界（直调写入面，绕过 agent 端点）。
        inkos_engine::play::create_world(
            &root,
            &inkos_engine::play::PlayWorldInput {
                id: "w-step",
                title: "厅堂夜探",
                premise: "深夜宅邸",
                world_contract: "",
                visual_contract: "",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();
        inkos_engine::play::ensure_run(&root, "w-step", "main").await.unwrap();
        inkos_engine::play::append_transcript_turn(&root, "w-step", "main", "user", "开始").await.unwrap();

        let agents = PlayAgents { router: &runtime.router, root: &root };
        let runner = PlayRunner {
            project_root: &root,
            world_id: "w-step".to_string(),
            run_id: "main".to_string(),
        };
        let outcome = runner
            .step(&agents, &agents, &agents, None, "我环顾四周", None)
            .await
            .unwrap();
        assert!(outcome.scene_text.contains("厅堂"), "scene: {outcome:?}");
        assert_eq!(outcome.suggested_actions.len(), 2);
        assert_eq!(outcome.action["actionKind"], "look");

        // 全部落盘：事件 + state + scene 投影 + transcript 双轮 + 图（file 后端）。
        let run = root.join("worlds").join("w-step").join("runs").join("main");
        let events = std::fs::read_to_string(run.join("events.jsonl")).unwrap();
        assert!(events.contains("evt-1"), "{events}");
        assert!(events.contains("看清了厅堂"), "{events}");
        let current: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(run.join("state").join("current.json")).unwrap()).unwrap();
        assert_eq!(current["turn"], 1);
        assert_eq!(current["lastEventId"], "evt-1");
        assert_eq!(current["blocked"], false);
        let scene = std::fs::read_to_string(run.join("projections").join("scene.md")).unwrap();
        assert!(scene.contains("厅堂深处"));
        let transcript = std::fs::read_to_string(run.join("transcript.jsonl")).unwrap();
        assert!(transcript.contains("我环顾四周"));
        assert!(transcript.contains("灯笼的光"));
        let graph: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(run.join("play-graph.json")).unwrap()).unwrap();
        assert!(graph["entities"]
            .as_object()
            .is_some_and(|entities| entities.contains_key("location_hall")));

        // 空输入 → Err。
        assert!(runner
            .step(&agents, &agents, &agents, None, "   ", None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn play_start_english_premise_infers_en_world_and_scene() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let llm = mock_play_llm().await;
        let app = app73(rt73(&root, &llm));
        let session = "1782991000000-playen";

        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 英文 title/premise → inferLanguage 判 en（104 号）→ world.json
        // language "en" + 英文缺省开场正文（TS en 分支逐字）。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{
                "instruction": "Open a haunted manor world",
                "sessionId": "1782991000000-playen",
                "actionSource": "button",
                "requestedIntent": "play_start",
                "actionPayload": { "playStart": {
                    "title": "Haunted Manor",
                    "premise": "A snowbound manor hides an old case."
                }}
            }"#
            .replace(char::is_whitespace, " ")
            .as_str()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "play_start");
        assert_eq!(exec["status"], "completed", "body: {parsed}");
        assert_eq!(
            exec["details"]["sceneText"],
            "You enter \"Haunted Manor\".\nA snowbound manor hides an old case.",
            "body: {parsed}"
        );
        let world: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("worlds").join(session).join("world.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(world["language"], "en", "world: {world}");
    }
}

mod image74_e2e {
    //! 74 号：生图链解锁（node-image + play generate-image）+ 三执行器接线。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::interactive_film_routes;
    use inkos_engine::server::play_routes;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt74(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app74(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
            .route(
                "/api/v1/projects/:id/nodes/:nodeId/image",
                axum::routing::post(interactive_film_routes::post_node_image),
            )
            .route(
                "/api/v1/play/runs/:worldId/:runId/generate-image",
                axum::routing::post(play_routes::post_play_generate_image),
            )
            .route(
                "/api/v1/play/runs/:worldId/:runId",
                axum::routing::get(play_routes::get_play_run),
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

    /// 生图 mock（images API，b64_png）+ LLM mock（draft_structure 编剧）双端口。
    async fn mock_image_and_llm() -> (String, String) {
        use base64::Engine as _;
        let png_b64 = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG-fake-image");
        let image_payload = format!(r#"{{"data":[{{"b64_json":"{png_b64}"}}]}}"#);
        let image_payload_for_server = image_payload.clone();
        let image_app = axum::Router::new().route(
            "/images/generations",
            axum::routing::post(move || async move {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    image_payload_for_server,
                )
            }),
        );
        let image_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let image_addr = image_listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(image_listener, image_app).await.unwrap(); });

        let llm_app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                let content = if system.contains("互动影游编剧") {
                    r#"{"nodes":[
                        {"id":"start","type":"start","title":"开场","choices":[{"id":"c1","text":"走左","targetNodeId":"end_good"}]},
                        {"id":"end_good","type":"ending","title":"善终"}
                    ]}"#.to_string()
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
        let llm_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let llm_addr = llm_listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(llm_listener, llm_app).await.unwrap(); });

        (format!("http://{image_addr}"), format!("http://{llm_addr}"))
    }

    #[tokio::test]
    async fn node_image_generates_and_attaches_via_delta() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (image_base, _llm) = mock_image_and_llm().await;
        // cover 配置：env base URL（避免污染进程 env，走 inkos.json）。
        std::fs::write(
            root.join("inkos.json"),
            format!(r#"{{"llm":{{"cover":{{"service":"kkaiapi","baseUrl":"{image_base}"}}}}}}"#),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{"services":{"cover:kkaiapi":{"apiKey":"sk-test"}}}"#,
        )
        .unwrap();

        // 项目图谱（含 sceneDesc 作生图 prompt 源）。
        let graph = serde_json::json!({
            "schemaVersion": 1, "projectId": "film1", "title": "影游",
            "nodes": [
                { "id": "start", "type": "start", "sceneDesc": "雪夜宅邸门口", "choices": [] }
            ],
            "endings": []
        });
        let dir_film = root.join("interactive-films").join("film1");
        std::fs::create_dir_all(&dir_film).unwrap();
        std::fs::write(dir_film.join("story-graph.json"), graph.to_string()).unwrap();

        let app = app74(rt74(&root, "http://127.0.0.1:9"));
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/projects/film1/nodes/start/image",
            Some("{}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["assetRef"], "interactive-films/film1/assets/nodes/start.png");
        assert_eq!(parsed["rev"], 1);
        // 图片落盘 + delta 回写（imageSlot 注入 + authoring rev）。
        assert_eq!(
            std::fs::read(root.join("interactive-films/film1/assets/nodes/start.png")).unwrap(),
            b"\x89PNG-fake-image"
        );
        let updated: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir_film.join("story-graph.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(updated["nodes"][0]["imageSlot"]["assetRef"], "interactive-films/film1/assets/nodes/start.png");
        assert_eq!(updated["nodes"][0]["imageSlot"]["prompt"], "雪夜宅邸门口");

        // 未配置 cover → 400 needsCoverConfig。
        let bare = tempfile::tempdir().unwrap();
        let bare_root = bare.path().to_path_buf();
        let dir_bare = bare_root.join("interactive-films").join("film2");
        std::fs::create_dir_all(&dir_bare).unwrap();
        std::fs::write(dir_bare.join("story-graph.json"), graph.to_string()).unwrap();
        let (status, parsed) = call(
            app74(rt74(&bare_root, "http://127.0.0.1:9")),
            "POST",
            "/api/v1/projects/film2/nodes/start/image",
            Some("{}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["needsCoverConfig"], true);
    }

    #[tokio::test]
    async fn play_generate_image_writes_manifest_and_url() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (image_base, _llm) = mock_image_and_llm().await;
        std::fs::write(
            root.join("inkos.json"),
            format!(r#"{{"llm":{{"cover":{{"service":"kkaiapi","baseUrl":"{image_base}"}}}}}}"#),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{"services":{"cover:kkaiapi":{"apiKey":"sk-test"}}}"#,
        )
        .unwrap();

        // play world + run（实体 actor_hero + scene 投影）。
        let run = root.join("worlds").join("w1").join("runs").join("main");
        std::fs::create_dir_all(run.join("projections")).unwrap();
        std::fs::create_dir_all(run.join("images")).unwrap();
        std::fs::write(
            root.join("worlds").join("w1").join("world.json"),
            r#"{"id":"w1","title":"雪夜","premise":"","worldContract":"","visualContract":"水墨","mode":"open","language":"zh","createdAt":"t","updatedAt":"t"}"#,
        )
        .unwrap();
        std::fs::write(
            run.join("play-graph.json"),
            r#"{"entities":{"actor_hero":{"id":"actor_hero","type":"actor","label":"林动","summary":"少年"}},"edges":{},"stateSlots":{},"events":{}}"#,
        )
        .unwrap();
        std::fs::write(run.join("projections").join("scene.md"), "雪夜孤灯，风声掠过檐角。").unwrap();

        let app = app74(rt74(&root, "http://127.0.0.1:9"));
        // scene 生图（投影文本兜底）。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/play/runs/w1/main/generate-image",
            Some(r#"{ "target": "scene" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["key"], "scene-turn-0");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["status"], "ready");
        assert_eq!(parsed["file"], "scene-turn-0.png");
        assert_eq!(parsed["url"], "/api/v1/play/runs/w1/main/images/scene-turn-0.png");
        assert_eq!(
            std::fs::read(run.join("images").join("scene-turn-0.png")).unwrap(),
            b"\x89PNG-fake-image"
        );
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(run.join("images").join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["scene-turn-0"]["status"], "ready");

        // entity 生图。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/play/runs/w1/main/generate-image",
            Some(r#"{ "target": "entity", "entityId": "actor_hero" }"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["key"], "actor_hero");
        assert_eq!(parsed["file"], "actor_hero.png");

        // GET run：实体 ready 注入 imageUrl（71 号读取面合并）。
        let (status, run_view) = call(app, "GET", "/api/v1/play/runs/w1/main", None).await;
        assert_eq!(status, StatusCode::OK);
        let hero = run_view["graph"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == "actor_hero")
            .cloned()
            .unwrap();
        assert_eq!(hero["imageUrl"], "/api/v1/play/runs/w1/main/images/actor_hero.png");
    }

    #[tokio::test]
    async fn film_executor_intents_confirm_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (_image, llm) = mock_image_and_llm().await;
        let app = app74(rt74(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782992000000-film01"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // draft_structure：LLM 骨架 → 图谱建立（rev 1）。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"生成双结局骨架","sessionId":"1782992000000-film01","actionSource":"button","requestedIntent":"draft_structure","actionPayload":{"draftStructure":{"projectId":"film-d"}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "draft_structure");
        assert_eq!(exec["status"], "completed");
        assert_eq!(exec["details"]["kind"], "graph_updated");
        assert_eq!(exec["details"]["rev"], 1);
        assert_eq!(exec["result"], "Structure drafted: 2 nodes (rev 1).");
        let graph_raw = std::fs::read_to_string(root.join("interactive-films/film-d/story-graph.json")).unwrap();
        assert!(graph_raw.contains("\"start\""), "{graph_raw}");

        // connect_choice：完整节点 upsert（改选项）→ rev 2。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"连线","sessionId":"1782992000000-film01","actionSource":"button","requestedIntent":"connect_choice","actionPayload":{"connectChoice":{"projectId":"film-d","node":{"id":"start","type":"start","title":"开场","choices":[{"id":"c2","text":"走右","targetNodeId":"end_good"}]}}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "connect_choice");
        assert_eq!(exec["details"]["rev"], 2);

        // remove_node：nodeId 删除 → rev 3 + 节点消失。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"删节点","sessionId":"1782992000000-film01","actionSource":"button","requestedIntent":"remove_node","actionPayload":{"removeNode":{"projectId":"film-d","nodeId":"end_good"}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "remove_node");
        assert_eq!(exec["details"]["rev"], 3);
        let after: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("interactive-films/film-d/story-graph.json")).unwrap(),
        )
        .unwrap();
        assert!(after["nodes"].as_array().unwrap().iter().all(|n| n["id"] != "end_good"));

        // 缺 nodeId → 502 + 中文文案。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"删","sessionId":"1782992000000-film01","actionSource":"button","requestedIntent":"remove_node","actionPayload":{"removeNode":{}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(parsed["error"]["message"].as_str().unwrap().contains("缺少 nodeId"), "body: {parsed}");
    }
}

mod translation75_e2e {
    //! 75 号：translation_create 确认意图 + actionPayload strict 校验面。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt75(root: &std::path::Path) -> BooksRuntime {
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

    fn app75(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    #[tokio::test]
    async fn translation_create_confirm_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("novel.txt"), "Chapter 1\n\nThe mountain stood.\n\nMana flowed.").unwrap();
        let app = app75(rt75(&root));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782993000000-tr01"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 确认意图：摄取分段建项（无 LLM）。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"翻译这本书","sessionId":"1782993000000-tr01","actionSource":"button","requestedIntent":"translation_create","actionPayload":{"translationCreate":{"filePath":"novel.txt","sourceLanguage":"en","targetLanguage":"zh","title":"山之书","segmentMaxChars":1200}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        // translation_create 不在 suppressManualTextForTool 表 → response 为结果文本。
        assert!(
            parsed["response"].as_str().unwrap_or_default().contains("Translation project \"山之书\" created."),
            "response: {parsed}"
        );
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "translation_create");
        assert_eq!(exec["status"], "completed");
        assert_eq!(exec["label"], "翻译项目");
        assert_eq!(exec["args"]["title"], "山之书");
        assert_eq!(exec["args"]["segmentMaxChars"], 1200);
        let details = &exec["details"];
        assert_eq!(details["kind"], "translation_project_created");
        let manifest = &details["manifest"];
        assert_eq!(manifest["title"], "山之书");
        assert_eq!(manifest["sourceLanguage"], "en");
        assert_eq!(manifest["chapters"].as_array().unwrap().len(), 1);
        assert_eq!(manifest["source"]["kind"], "text");
        assert!(details["manifestPath"].as_str().unwrap().starts_with("translations/"));
        assert!(details["projectDir"].as_str().unwrap().starts_with("translations/"));
        // 结果文本五段。
        assert!(exec["result"].as_str().unwrap().contains("Translation project \"山之书\" created."));
        assert!(exec["result"].as_str().unwrap().contains("Chapters: 1"));

        // 缺 filePath → 502 中文文案。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"翻译","sessionId":"1782993000000-tr01","actionSource":"button","requestedIntent":"translation_create","actionPayload":{"translationCreate":{"sourceLanguage":"en","targetLanguage":"zh"}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("缺少文件路径"), "body: {parsed}");

        // 文件不存在 → 502（创建失败面）。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"翻译","sessionId":"1782993000000-tr01","actionSource":"button","requestedIntent":"translation_create","actionPayload":{"translationCreate":{"filePath":"ghost.txt","sourceLanguage":"en","targetLanguage":"zh"}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
    }

    #[tokio::test]
    async fn action_payload_strict_endpoint_surface() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let app = app75(rt75(&root));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782993000001-tr02"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 顶层 unknown 键 → 400 INVALID_ACTION_PAYLOAD。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"你好","sessionId":"1782993000001-tr02","actionPayload":{"bogus":{}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "INVALID_ACTION_PAYLOAD");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("Unrecognized key: bogus"), "body: {parsed}");

        // 子域 unknown 键（createBook strict）→ 400。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"建书","sessionId":"1782993000001-tr02","actionPayload":{"createBook":{"title":"X","extra":1}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("createBook: Unrecognized key: extra"), "body: {parsed}");

        // 枚举非法（platform）→ 400。
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"建书","sessionId":"1782993000001-tr02","actionPayload":{"createBook":{"platform":"nope"}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // shortRun 联动（zh+700 越界）→ 400。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"写短篇","sessionId":"1782993000001-tr02","actionPayload":{"shortRun":{"language":"zh","charsPerChapter":700}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("charsPerChapter"), "body: {parsed}");

        // 非 strict 子域（draftStructure）unknown 键放行 → 走意图分支（无 projectId → 502）。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"骨架","sessionId":"1782993000001-tr02","actionSource":"button","requestedIntent":"draft_structure","actionPayload":{"draftStructure":{"instruction":"i","extra":1}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("project id"), "body: {parsed}");
    }
}

mod script76_e2e {
    //! 76 号：script_create / storyboard_create / generate_cover 三确认全链。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt76(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app76(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    /// LLM（剧本/分镜双代理）+ 生图（images b64）双 mock。
    async fn mocks() -> (String, String) {
        let script_body = "# 山雨 剧本\n\n## 剧本正文\n\n# 第一集 夜雨\n\n场景：旧宅门口\n\n字幕：第九集完".to_string();
        let storyboard_body = "# 山雨 分镜\n\n## 分镜表\n\n| 镜号 | 画面 |\n| --- | --- |\n| 01 | 雨夜街口 |\n\n## 图像提示词\n\nPrompt: 雨夜街口，水墨远景\nPrompt: 灯下人影，半身近景".to_string();
        let llm_app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let script_body = script_body.clone();
                let storyboard_body = storyboard_body.clone();
                async move {
                    let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                    let content = if system.contains("剧本创作工具") {
                        script_body
                    } else if system.contains("分镜创作工具") {
                        storyboard_body
                    } else {
                        "PASS".to_string()
                    };
                    let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let llm_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let llm_addr = llm_listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(llm_listener, llm_app).await.unwrap(); });

        use base64::Engine as _;
        let png_b64 = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG-cover");
        let image_payload = format!(r#"{{"data":[{{"b64_json":"{png_b64}"}}]}}"#);
        let image_app = axum::Router::new().route(
            "/images/generations",
            axum::routing::post(move || async move {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    image_payload,
                )
            }),
        );
        let image_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let image_addr = image_listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(image_listener, image_app).await.unwrap(); });

        (format!("http://{llm_addr}"), format!("http://{image_addr}"))
    }

    #[tokio::test]
    async fn script_create_confirm_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, _image) = mocks().await;
        let app = app76(rt76(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782994000000-sc01"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"改编成竖屏短剧","sessionId":"1782994000000-sc01","actionSource":"button","requestedIntent":"script_create","actionPayload":{"scriptCreate":{"title":"山雨","targetFormat":"vertical_short_drama","episodeCount":8}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "script_create");
        assert_eq!(exec["label"], "剧本创作");
        assert_eq!(exec["status"], "completed");
        let details = &exec["details"];
        assert_eq!(details["kind"], "script_project_created");
        assert_eq!(details["projectId"], "山雨");
        assert_eq!(details["baseDir"], "dramas/山雨");
        assert_eq!(details["specPath"], "dramas/山雨/script-spec.md");
        assert_eq!(details["scriptPath"], "dramas/山雨/script.md");

        // 落盘：spec + script（集尾标签归一"字幕：第九集完"→第一集）+ status。
        let script = std::fs::read_to_string(root.join("dramas/山雨/script.md")).unwrap();
        assert!(script.contains("字幕：第一集完"), "{script}");
        let spec = std::fs::read_to_string(root.join("dramas/山雨/script-spec.md")).unwrap();
        assert!(spec.contains("# 山雨 剧本创作规格"), "{spec}");
        assert!(spec.contains("- 交付类型：竖屏短剧"), "{spec}");
        let status_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("dramas/山雨/status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(status_json["kind"], "script");
        assert_eq!(status_json["status"], "completed");

        // 缺 title → 502 中文。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"写剧本","sessionId":"1782994000000-sc01","actionSource":"button","requestedIntent":"script_create"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(parsed["error"]["message"].as_str().unwrap().contains("缺少标题"), "body: {parsed}");
    }

    #[tokio::test]
    async fn storyboard_create_confirm_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, _image) = mocks().await;
        let app = app76(rt76(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782994000001-sb01"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"拆分镜","sessionId":"1782994000001-sb01","actionSource":"button","requestedIntent":"storyboard_create","actionPayload":{"storyboardCreate":{"title":"山雨","visualStyle":"水墨","maxShots":12}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "storyboard_create");
        assert_eq!(exec["label"], "分镜创作");
        let details = &exec["details"];
        assert_eq!(details["kind"], "storyboard_project_created");
        assert_eq!(details["baseDir"], "storyboards/山雨");
        assert_eq!(details["assetsManifestPath"], "storyboards/山雨/assets.json");

        // image-prompts 提取（Prompt: 行编号化）。
        let prompts = std::fs::read_to_string(root.join("storyboards/山雨/image-prompts.md")).unwrap();
        assert!(prompts.contains("1. 雨夜街口，水墨远景"), "{prompts}");
        assert!(prompts.contains("2. 灯下人影，半身近景"), "{prompts}");
        // assets manifest：两 shot + prompt_ready + 三目录。
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("storyboards/山雨/assets.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["kind"], "storyboard_assets");
        assert_eq!(manifest["assets"].as_array().unwrap().len(), 2);
        assert_eq!(manifest["assets"][0]["shotId"], "shot-001");
        assert_eq!(manifest["assets"][0]["status"], "prompt_ready");
        assert!(root.join("storyboards/山雨/assets/source").is_dir());
        assert!(root.join("storyboards/山雨/assets/generated").is_dir());
        assert!(root.join("storyboards/山雨/assets/selected").is_dir());

        // 缺 title → 502。
        let (status, _) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"分镜","sessionId":"1782994000001-sb01","actionSource":"button","requestedIntent":"storyboard_create"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn generate_cover_confirm_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (_llm, image) = mocks().await;
        std::fs::write(
            root.join("inkos.json"),
            format!(r#"{{"llm":{{"cover":{{"service":"kkaiapi","baseUrl":"{image}"}}}}}}"#),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{"services":{"cover:kkaiapi":{"apiKey":"sk-test"}}}"#,
        )
        .unwrap();

        let app = app76(rt76(&root, "http://127.0.0.1:9"));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782994000002-cv01"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"生成封面","sessionId":"1782994000002-cv01","actionSource":"button","requestedIntent":"generate_cover","actionPayload":{"generateCover":{"title":"山雨","intro":"一个雨夜的复仇故事","sellingPoints":"节奏快；反转强"}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "generate_cover");
        assert_eq!(exec["label"], "生成封面");
        assert_eq!(exec["status"], "completed");
        let details = &exec["details"];
        assert_eq!(details["kind"], "cover_generated");
        assert_eq!(details["outputDir"], "covers/山雨");
        assert_eq!(details["coverPromptPath"], "covers/山雨/cover-prompt.md");
        assert_eq!(details["coverImagePath"], "covers/山雨/cover.png");
        // 响应文本三行。
        assert!(parsed["response"].as_str().unwrap().contains("Cover generated for \"山雨\"."), "body: {parsed}");

        // 落盘：prompt（generic zh）+ png。
        let prompt = std::fs::read_to_string(root.join("covers/山雨/cover-prompt.md")).unwrap();
        assert!(prompt.starts_with("按用户给出的标题、简介、卖点和视觉要求生成封面图。"), "{prompt}");
        assert!(prompt.contains("卖点：节奏快；反转强"), "{prompt}");
        assert_eq!(
            std::fs::read(root.join("covers/山雨/cover.png")).unwrap(),
            b"\x89PNG-cover"
        );

        // 缺 title → 502；未配置 cover → 502 needsCoverConfig 经统一错误面。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"封面","sessionId":"1782994000002-cv01","actionSource":"button","requestedIntent":"generate_cover"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(parsed["error"]["message"].as_str().unwrap().contains("缺少标题"), "body: {parsed}");

        let bare = tempfile::tempdir().unwrap();
        let bare_root = bare.path().to_path_buf();
        let app_bare = app76(rt76(&bare_root, "http://127.0.0.1:9"));
        let _ = call(
            app_bare.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782994000003-cv02"}"#),
        )
        .await;
        let (status, parsed) = call(
            app_bare,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"封面","sessionId":"1782994000003-cv02","actionSource":"button","requestedIntent":"generate_cover","actionPayload":{"generateCover":{"title":"无配置书"}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
        assert!(
            parsed["error"]["message"].as_str().unwrap().contains("cover endpoint is required"),
            "body: {parsed}"
        );
    }
}

mod film77_e2e {
    //! 77 号：interactive_film_create 确认全链（五节交付稿 + story graph + fallback）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt77(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app77(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    /// LLM mock：按系统提示词关键词分流五节创作代理（"互动影游创作工具"）与
    /// story graph 编剧（"互动影游编剧"）。graph_body 传 None → 返回非 JSON（触发 fallback）。
    async fn llm_mock(graph_body: Option<String>) -> String {
        let package_body = "# 迷雾宅邸 互动影游方案\n\n## 剧情树\n\n主线三幕，末段双结局。\n\n## 变量与旗标表\n\n| 变量 | 含义 |\n| --- | --- |\n| trust | 信任度 |\n\n## 多结局路径\n\n结局 A：完成主线。\n\n## 互动剧本\n\n# 第一集 夜宅\n\n场景：旧宅门口\n\n字幕：第九集完\n\n## 分镜与图像提示词\n\n| 镜号 | 画面 |\n| --- | --- |\n| 01 | 雨夜街口 |\n\nPrompt: 雨夜街口，水墨远景\nPrompt: 灯下人影，半身近景".to_string();
        let llm_app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let package_body = package_body.clone();
                let graph_body = graph_body.clone();
                async move {
                    let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                    let content = if system.contains("互动影游创作工具") {
                        package_body
                    } else if system.contains("互动影游编剧") {
                        graph_body.unwrap_or_else(|| "NOT JSON".to_string())
                    } else {
                        "PASS".to_string()
                    };
                    let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, llm_app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn llm_graph() -> String {
        r#"```json
{"schemaVersion":1,"projectId":"旧id","title":"迷雾宅邸","variables":[{"name":"trust","type":"counter","default":0,"desc":""}],"nodes":[{"id":"start","title":"开场","type":"start","sceneDesc":"夜宅","choices":[{"id":"c1","text":"进入","targetNodeId":"end-good","effects":[]}]},{"id":"end-good","title":"好结局","type":"ending","sceneDesc":"","choices":[],"act":"end"}],"endings":[{"id":"e1","nodeId":"end-good","title":"好结局","type":"good","description":""}]}
```"#
            .to_string()
    }

    #[tokio::test]
    async fn interactive_film_create_confirm_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let llm = llm_mock(Some(llm_graph())).await;
        let app = app77(rt77(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782994000007-if01"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"做成互动影游","sessionId":"1782994000007-if01","actionSource":"button","requestedIntent":"interactive_film_create","actionPayload":{"interactiveFilmCreate":{"title":"迷雾宅邸","episodeCount":5,"targetAudience":"青年玩家","referenceMode":"底片风"}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "interactive_film_create");
        assert_eq!(exec["label"], "互动影游");
        assert_eq!(exec["status"], "completed");
        // suppressManualTextForTool 名单内 → 助手气泡留空。
        assert_eq!(parsed["response"].as_str().unwrap_or("NULL"), "", "body: {parsed}");
        let details = &exec["details"];
        assert_eq!(details["kind"], "interactive_film_created");
        assert_eq!(details["projectId"], "迷雾宅邸");
        assert_eq!(details["baseDir"], "interactive-films/迷雾宅邸");
        assert_eq!(details["storyGraphPath"], "interactive-films/迷雾宅邸/story-graph.json");
        assert_eq!(details["specPath"], "interactive-films/迷雾宅邸/interactive-spec.md");
        assert_eq!(details["storyTreePath"], "interactive-films/迷雾宅邸/story-tree.md");
        assert_eq!(details["flagsPath"], "interactive-films/迷雾宅邸/flags.md");
        assert_eq!(details["scriptPath"], "interactive-films/迷雾宅邸/script.md");
        assert_eq!(details["assetsManifestPath"], "interactive-films/迷雾宅邸/assets.json");
        let text = exec["result"].as_str().unwrap_or_default();
        assert!(text.starts_with("Interactive film \"迷雾宅邸\" completed."), "text: {text}");
        assert!(text.contains("Story graph: interactive-films/迷雾宅邸/story-graph.json"), "text: {text}");

        // 五节落盘：spec（含目标受众/参考模式）+ 树/旗标/剧本（集尾标签归一）。
        let spec = std::fs::read_to_string(root.join("interactive-films/迷雾宅邸/interactive-spec.md")).unwrap();
        assert!(spec.starts_with("# 迷雾宅邸 互动影游创作规格"), "{spec}");
        assert!(spec.contains("- 目标受众：青年玩家"), "{spec}");
        assert!(spec.contains("- 参考模式：底片风"), "{spec}");
        let tree = std::fs::read_to_string(root.join("interactive-films/迷雾宅邸/story-tree.md")).unwrap();
        assert!(tree.contains("主线三幕"), "{tree}");
        assert!(!tree.contains("剧情树"), "小节提取后不含自身标题：{tree}");
        let flags = std::fs::read_to_string(root.join("interactive-films/迷雾宅邸/flags.md")).unwrap();
        assert!(flags.contains("trust"), "{flags}");
        let script = std::fs::read_to_string(root.join("interactive-films/迷雾宅邸/script.md")).unwrap();
        assert!(script.contains("字幕：第一集完"), "{script}");
        // image-prompts 编号化 + assets manifest（LLM graph 走通）。
        let prompts = std::fs::read_to_string(root.join("interactive-films/迷雾宅邸/image-prompts.md")).unwrap();
        assert!(prompts.contains("1. 雨夜街口，水墨远景"), "{prompts}");
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("interactive-films/迷雾宅邸/assets.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["assets"].as_array().unwrap().len(), 2);
        assert!(root.join("interactive-films/迷雾宅邸/assets/selected").is_dir());
        // story-graph.json：LLM 输出 + projectId 注入。
        let graph: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("interactive-films/迷雾宅邸/story-graph.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(graph["projectId"], "迷雾宅邸");
        assert_eq!(graph["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(graph["endings"][0]["type"], "good");
        let status_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("interactive-films/迷雾宅邸/status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(status_json["kind"], "interactive_film");

        // 缺 title → 502 中文。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"互动影游","sessionId":"1782994000007-if01","actionSource":"button","requestedIntent":"interactive_film_create"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(parsed["error"]["message"].as_str().unwrap().contains("确认创建互动影游缺少标题"), "body: {parsed}");
    }

    #[tokio::test]
    async fn interactive_film_create_graph_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // graph 编剧返回非 JSON → 最小可玩图回退。
        let llm = llm_mock(None).await;
        let app = app77(rt77(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782994000008-if02"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"做成互动影游","sessionId":"1782994000008-if02","actionSource":"button","requestedIntent":"interactive_film_create","actionPayload":{"interactiveFilmCreate":{"title":"雾中灯","episodeCount":3,"requirements":"轻悬疑"}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["status"], "completed", "body: {parsed}");
        // fallback 图：start + act-1..3 + 双结局，story_progress counter。
        let graph: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("interactive-films/雾中灯/story-graph.json")).unwrap(),
        )
        .unwrap();
        let nodes = graph["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 6, "graph: {graph}");
        assert_eq!(nodes[3]["id"], "act-3");
        assert_eq!(nodes[3]["type"], "branch");
        assert_eq!(nodes[3]["choices"].as_array().unwrap().len(), 2);
        assert_eq!(graph["variables"][0]["name"], "story_progress");
        // storyCore = merge 后 requirements（instruction + 补充要求）。
        assert!(graph["worldAnchor"]["storyCore"].as_str().unwrap().contains("轻悬疑"), "graph: {graph}");
        // 进度日志含 fallback 说明。
        let logs = exec["logs"].as_array().cloned().unwrap_or_default();
        let joined = logs
            .iter()
            .filter_map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Story graph JSON generation failed; writing a minimal playable graph."), "logs: {joined}");
    }
}

mod short78_e2e {
    //! 78 号：short_run 确认全链（可恢复三段生产 + 补章循环 + 修订降级 + 最终语言断言）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt78(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app78(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    // ── mock 输出体 ──────────────────────────────────────────────

    fn outline_body() -> String {
        "=== SHORT_FICTION_PLAN_TITLE ===\n《夜雨追凶》\n\n=== SHORT_FICTION_PLAN ===\n十二幕追凶方案：雨夜开局，双线收束，末章收网。".to_string()
    }

    fn full_draft_body(chapters: usize) -> String {
        let mut out = String::from(
            "=== SHORT_FICTION_TITLE ===\n夜雨追凶\n\n=== SHORT_FICTION_OPENING_HOOK ===\n雨夜，凶案发生，追查开始。\n\n",
        );
        for n in 1..=chapters {
            out.push_str(&format!(
                "=== CHAPTER {n} TITLE ===\n夜行{n}\n=== CHAPTER {n} CONTENT ===\n第{n}章正文：雨夜追查推进，线索浮出水面，压力升级。\n\n"
            ));
        }
        out
    }

    fn chapter12_body() -> String {
        "=== CHAPTER 12 TITLE ===\n收网\n=== CHAPTER 12 CONTENT ===\n第12章正文：真相收网，雨停天明。\n".to_string()
    }

    fn package_body() -> String {
        "=== SHORT_FICTION_PACKAGE_TITLE ===\n夜雨追凶\n=== SHORT_FICTION_INTRO ===\n雨夜凶案，双线追凶，末章收网。\n=== SHORT_FICTION_SELLING_POINTS ===\n- 节奏快\n- 反转强\n=== SHORT_FICTION_COVER_PROMPT ===\n雨夜街口，灯下人影，3:4 竖图\n".to_string()
    }

    /// 分流：系统提示词关键词定位代理；writer 三形态按用户提示词区分。
    /// `revise_chapters` 控制修订稿完整度（12=完整采用 / 1=触发降级）。
    fn short_llm_dispatch(system: &str, user: &str, revise_chapters: usize) -> String {
        if system.contains("短篇小说总编") {
            outline_body()
        } else if system.contains("短篇审纲编辑") {
            "审纲意见：中段可再收紧，双线交汇点后移。".to_string()
        } else if system.contains("BatchWriter") {
            if user.contains("只补写缺失章节") {
                chapter12_body()
            } else if user.contains("根据审稿意见") {
                full_draft_body(revise_chapters)
            } else {
                full_draft_body(11)
            }
        } else if system.contains("成稿审稿编辑") {
            "审稿意见：结尾稍赶，可在第 11 章埋一笔伏笔。".to_string()
        } else if system.contains("包装编辑") {
            package_body()
        } else {
            "PASS".to_string()
        }
    }

    async fn llm_mock_short(revise_chapters: usize) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_in = calls.clone();
        let llm_app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let calls_in = calls_in.clone();
                async move {
                    calls_in.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                    let user = body["messages"]
                        .as_array()
                        .map(|messages| {
                            messages
                                .iter()
                                .filter_map(|m| m["content"].as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    let content = short_llm_dispatch(&system, &user, revise_chapters);
                    let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, llm_app).await.unwrap(); });
        (format!("http://{addr}"), calls)
    }

    #[tokio::test]
    async fn short_run_confirm_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, _calls) = llm_mock_short(12).await;
        let app = app78(rt78(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782994000009-sf01"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"写一个短篇","sessionId":"1782994000009-sf01","actionSource":"button","requestedIntent":"short_run","actionPayload":{"shortRun":{"direction":"雨夜追凶，双线收束","storyId":"night-rain","chapters":12,"charsPerChapter":900,"cover":false}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["tool"], "short_fiction_run");
        assert_eq!(exec["label"], "短篇生产");
        assert_eq!(exec["status"], "completed", "body: {parsed}");
        // 不在 suppress 名单 → response 为结果文本。
        let response_text = parsed["response"].as_str().unwrap_or_default();
        assert!(response_text.starts_with("Short fiction \"night-rain\" completed."), "response: {response_text}");
        assert!(response_text.contains("Final: shorts/night-rain/final/full.md"), "response: {response_text}");
        assert!(response_text.contains("Sales package: shorts/night-rain/final/sales-package.md"), "response: {response_text}");
        assert!(response_text.contains("Cover prompt: shorts/night-rain/final/cover-prompt.md"), "response: {response_text}");
        assert!(response_text.contains("Cover image: not generated."), "response: {response_text}");
        assert!(response_text.contains("Cover image reason: disabled"), "response: {response_text}");
        let details = &exec["details"];
        assert_eq!(details["kind"], "short_fiction_created");
        assert_eq!(details["storyId"], "night-rain");
        assert_eq!(details["outlinePath"], "shorts/night-rain/outline/v002.md");
        assert_eq!(details["outlineReviewPath"], "shorts/night-rain/reviews/outline-v001.md");
        assert_eq!(details["draftReviewPath"], "shorts/night-rain/reviews/draft-v001.md");
        assert_eq!(details["finalMarkdownPath"], "shorts/night-rain/final/full.md");
        assert_eq!(details["finalJsonPath"], "shorts/night-rain/final/short-story.json");
        assert_eq!(details["salesPackagePath"], "shorts/night-rain/final/sales-package.md");
        assert_eq!(details["coverPromptPath"], "shorts/night-rain/final/cover-prompt.md");
        assert_eq!(details["coverError"], "disabled");
        assert!(details.get("coverImagePath").is_none(), "details: {details}");

        // 三段 outline 落盘。
        let v1 = std::fs::read_to_string(root.join("shorts/night-rain/outline/v001.md")).unwrap();
        assert!(v1.contains("SHORT_FICTION_PLAN"), "{v1}");
        let v2 = std::fs::read_to_string(root.join("shorts/night-rain/outline/v002.md")).unwrap();
        assert!(v2.contains("十二幕追凶方案"), "{v2}");
        let outline_review = std::fs::read_to_string(root.join("shorts/night-rain/reviews/outline-v001.md")).unwrap();
        assert!(outline_review.contains("审纲意见"), "{outline_review}");
        // 首写缺 12 章 → v001-partial + 补章循环。
        assert!(root.join("shorts/night-rain/drafts/v001-partial/full.md").is_file());
        let draft_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("shorts/night-rain/drafts/v001/draft.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(draft_json["chapters"].as_array().unwrap().len(), 12);
        assert_eq!(draft_json["chapters"][11]["title"], "收网");
        let chapter_file = std::fs::read_to_string(root.join("shorts/night-rain/drafts/v001/chapters/0012.md")).unwrap();
        assert!(chapter_file.starts_with("# 第12章 收网"), "{chapter_file}");
        let draft_review = std::fs::read_to_string(root.join("shorts/night-rain/reviews/draft-v001.md")).unwrap();
        assert!(draft_review.contains("审稿意见"), "{draft_review}");
        // 修订完整 → v002 采用。
        assert!(root.join("shorts/night-rain/drafts/v002/full.md").is_file());
        assert!(!root.join("shorts/night-rain/reviews/draft-v002-warning.md").exists());

        // final：标题命名副本 + 12 章（修订稿）+ short-story.json camelCase。
        let full = std::fs::read_to_string(root.join("shorts/night-rain/final/full.md")).unwrap();
        assert!(full.starts_with("# 夜雨追凶"), "{full}");
        assert!(full.contains("## 开篇钩子"), "{full}");
        assert!(full.contains("## 第12章 夜行12"), "{full}");
        assert!(root.join("shorts/night-rain/final/夜雨追凶.md").is_file());
        let story_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("shorts/night-rain/final/short-story.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(story_json["storyTitle"], "夜雨追凶");
        assert_eq!(story_json["chapters"].as_array().unwrap().len(), 12);
        assert!(story_json["chapters"][0].get("charCount").is_some(), "{story_json}");
        assert!(root.join("shorts/night-rain/final/chapters/0001.md").is_file());
        // 包装三件套。
        let sales = std::fs::read_to_string(root.join("shorts/night-rain/final/sales-package.md")).unwrap();
        assert!(sales.starts_with("# 夜雨追凶\n\n## 简介\n\n雨夜凶案，双线追凶，末章收网。\n\n## 卖点\n\n- 节奏快\n- 反转强\n\n## 封面提示词"), "{sales}");
        let cover_prompt = std::fs::read_to_string(root.join("shorts/night-rain/final/cover-prompt.md")).unwrap();
        assert!(cover_prompt.contains("3:4 竖图"), "{cover_prompt}");
        let package_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("shorts/night-rain/final/sales-package.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(package_json["sellingPoints"].as_array().unwrap().len(), 2);
        // 无修订警告 + cover=false → 不写 complete 状态（TS 仅 warning 时写 status）。
        assert!(!root.join("shorts/night-rain/status.json").exists());
    }

    #[tokio::test]
    async fn short_run_revision_degrades_and_final_language_guard() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 修订稿只回 1 章 → validate 失败 → 降级保留 v1 + warning 文件 + complete 状态。
        let (llm, _calls) = llm_mock_short(1).await;
        let app = app78(rt78(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782994000010-sf02"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"写一个短篇","sessionId":"1782994000010-sf02","actionSource":"button","requestedIntent":"short_run","actionPayload":{"shortRun":{"direction":"雨夜追凶","chapters":12,"charsPerChapter":900,"cover":false}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["status"], "completed", "body: {parsed}");
        // v1（补章后完整 12 章）为最终稿；v2 未采用。
        assert!(!root.join("shorts/夜雨追凶/drafts/v002/full.md").exists());
        let warning = std::fs::read_to_string(root.join("shorts/夜雨追凶/reviews/draft-v002-warning.md")).unwrap();
        assert!(warning.starts_with("# 第二轮改稿未采用"), "{warning}");
        assert!(warning.contains("Short-hit draft is incomplete"), "{warning}");
        let full = std::fs::read_to_string(root.join("shorts/夜雨追凶/final/full.md")).unwrap();
        assert!(full.contains("## 第12章 收网"), "{full}");
        // complete 状态带 revision skipped 警告。
        let status_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("shorts/夜雨追凶/status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(status_json["status"], "complete");
        assert!(status_json["warning"].as_str().unwrap().starts_with("revision skipped:"), "{status_json}");
        assert!(status_json.get("updatedAt").is_some());

        // 最终语言断言：zh 会话（payload 无 language）+ charsPerChapter 700 过
        // 确认卡并集校验（600-1200）→ 执行层 zh 范围（900-1200）拦截 → 502 双语。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"写一个短篇","sessionId":"1782994000010-sf02","actionSource":"button","requestedIntent":"short_run","actionPayload":{"shortRun":{"direction":"再来一个","charsPerChapter":700}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
        let message = parsed["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("charsPerChapter=700 超出中文短篇的合法范围（每章 900-1200 个汉字）"), "message: {message}");
        assert!(message.contains("is outside the valid range for Chinese shorts (900-1200 characters per chapter)"), "message: {message}");
    }

    #[tokio::test]
    async fn short_run_resume_already_complete() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // 已完成的稳定 storyId → 直接按原样返回（零 LLM 调用）。
        std::fs::create_dir_all(root.join("shorts/existing/final")).unwrap();
        std::fs::write(root.join("shorts/existing/final/full.md"), "# 旧稿\n\n已完成的短篇。\n").unwrap();
        let (llm, calls) = llm_mock_short(12).await;
        let app = app78(rt78(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(r#"{"sessionId":"1782994000011-sf03"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(r#"{"instruction":"雨夜追凶","sessionId":"1782994000011-sf03","actionSource":"button","requestedIntent":"short_run","actionPayload":{"shortRun":{"storyId":"existing","cover":false}}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["status"], "completed", "body: {parsed}");
        // direction 兜底 instruction（args 面）。
        assert_eq!(exec["args"]["direction"], "雨夜追凶");
        let details = &exec["details"];
        assert_eq!(details["storyId"], "existing");
        assert_eq!(details["coverError"], "already-complete");
        assert_eq!(details["finalMarkdownPath"], "shorts/existing/final/full.md");
        let text = parsed["response"].as_str().unwrap_or_default();
        assert!(text.starts_with("Short fiction \"existing\" completed."), "text: {text}");
        // 零 LLM 调用 + 不触发任何生产落盘。
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(!root.join("shorts/existing/outline").exists());
    }
}

mod play79_e2e {
    //! 79 号：sceneReconciler 对账 + regenerate 变体/checkpoint 面（真实 PlayAgents +
    //! mock LLM 分流——reconciler 补充入图、重写约束注入、变体对、恢复第一版）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::state::manager::StateManager;
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn rt79(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    /// mock：五路分流（interpreter/seed-mutator/turn-mutator/renderer/reconciler）；
    /// renderer 计数出场景 A/B，重写约束注入以原子标志捕获。
    async fn mock_play79_llm() -> (String, Arc<AtomicUsize>, Arc<AtomicBool>) {
        let render_calls = Arc::new(AtomicUsize::new(0));
        let saw_replay_constraint = Arc::new(AtomicBool::new(false));
        let render_in = render_calls.clone();
        let flag_in = saw_replay_constraint.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let render_in = render_in.clone();
                let flag_in = flag_in.clone();
                async move {
                    let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                    let user = body["messages"][1]["content"].as_str().unwrap_or("").to_string();
                    let content = if system.contains("动作理解器") {
                        serde_json::json!({ "actionKind": "look", "intent": "环顾四周", "secondaryActions": [] }).to_string()
                    } else if system.contains("世界状态草案员") {
                        if user.contains("只播种这个互动世界开场已经成立的状态") {
                            serde_json::json!({
                                "eventId": "evt-0", "turn": 0, "actionKind": "look",
                                "summary": "开场状态已播种",
                                "entities": { "upsert": [
                                    { "id": "actor_player", "type": "actor", "label": "夜行人", "summary": "潜入者", "updatedEventId": "evt-0" },
                                    { "id": "item_lantern", "type": "item", "label": "灯笼", "summary": "照明", "updatedEventId": "evt-0" }
                                ]},
                                "edges": { "upsert": [
                                    { "fromId": "actor_player", "type": "持有", "toId": "item_lantern",
                                      "value": { "role": "holding" }, "validFromEventId": "evt-0", "sourceEventId": "evt-0" }
                                ]}
                            }).to_string()
                        } else {
                            serde_json::json!({
                                "eventId": "evt-1", "turn": 1, "actionKind": "look",
                                "summary": "玩家看清了厅堂",
                                "timeAdvance": { "elapsed": "片刻", "anchor": "深夜", "rationale": "环顾", "synchronized": [] },
                                "entities": { "upsert": [
                                    { "id": "location_hall", "type": "location", "label": "厅堂", "summary": "正厅", "updatedEventId": "evt-1" }
                                ]}
                            }).to_string()
                        }
                    } else if system.contains("互动小说场景") {
                        if user.contains("重写约束") {
                            flag_in.store(true, Ordering::SeqCst);
                        }
                        let n = render_in.fetch_add(1, Ordering::SeqCst);
                        let scene = if n == 0 {
                            serde_json::json!({ "sceneText": "场景甲：灯笼的光晃了一下。", "suggestedActions": [] })
                        } else {
                            serde_json::json!({ "sceneText": "场景乙：重写后的厅堂静得反常。", "suggestedActions": [] })
                        };
                        scene.to_string()
                    } else if system.contains("把互动小说正文和世界图谱对齐") {
                        serde_json::json!({
                            "eventId": "evt-1", "turn": 1, "actionKind": "look",
                            "summary": "正文补充：纽扣落地",
                            "entities": { "upsert": [
                                { "id": "clue_button", "type": "clue", "label": "铜纽扣", "summary": "灯下闪光", "updatedEventId": "evt-1" }
                            ]},
                            "edges": { "upsert": [
                                { "fromId": "actor_player", "type": "持有", "toId": "clue_button",
                                  "value": { "role": "holding", "physical": true }, "validFromEventId": "evt-1", "sourceEventId": "evt-1" }
                            ]},
                            "notes": ["对账补充"]
                        }).to_string()
                    } else {
                        "PASS".to_string()
                    };
                    let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
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
        (format!("http://{addr}"), render_calls, saw_replay_constraint)
    }

    #[tokio::test]
    async fn reconcile_merge_and_regenerate_variant_chain() {
        use inkos_engine::play_runner::{PlayAgents, PlayRunner};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, _render_calls, saw_replay_constraint) = mock_play79_llm().await;
        let runtime = rt79(&root, &llm);

        inkos_engine::play::create_world(
            &root,
            &inkos_engine::play::PlayWorldInput {
                id: "w79e2e",
                title: "厅堂夜探",
                premise: "深夜宅邸",
                world_contract: "",
                visual_contract: "",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();

        let agents = PlayAgents { router: &runtime.router, root: &root };
        let runner = PlayRunner {
            project_root: &root,
            world_id: "w79e2e".to_string(),
            run_id: "main".to_string(),
        };

        // seed → step（带 reconciler）。
        runner.seed_opening(&agents, "雨夜开场。", &[]).await.unwrap();
        let step1 = runner
            .step(&agents, &agents, &agents, Some(&agents), "我环顾四周", None)
            .await
            .unwrap();
        assert_eq!(step1.scene_text, "场景甲：灯笼的光晃了一下。");
        // reconcile 补充：summary 合成 + clue_button 入图 + holding 边。
        let summary = step1.mutation.get("summary").and_then(Value::as_str).unwrap();
        assert!(summary.contains("玩家看清了厅堂"), "{summary}");
        assert!(summary.contains("正文补充：纽扣落地"), "{summary}");
        let run = root.join("worlds/w79e2e/runs/main");
        let graph: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(run.join("play-graph.json")).unwrap()).unwrap();
        let entity_ids: Vec<&str> = graph["entities"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert!(entity_ids.contains(&"clue_button"), "reconcile 实体入图：{entity_ids:?}");
        assert!(graph["edges"].as_object().unwrap().values().any(|edge| {
            edge.get("toId").and_then(Value::as_str) == Some("clue_button")
                && edge.pointer("/value/role").and_then(Value::as_str) == Some("holding")
        }), "graph: {graph}");
        // checkpoint 前置 + state 投影含补充实体。
        assert!(run.join("checkpoints/before-turn-1.json").is_file());
        let state_md = std::fs::read_to_string(run.join("projections/state.md")).unwrap();
        assert!(state_md.contains("clue_button"), "{state_md}");

        // regenerate：重放原输入 → 场景乙 + 重写约束注入 + 变体对 + 事件不涨。
        let replay = runner
            .regenerate_last_turn(&agents, &agents, &agents, Some(&agents), None)
            .await
            .unwrap();
        assert_eq!(replay.scene_text, "场景乙：重写后的厅堂静得反常。");
        assert_eq!(replay.replayed_input, "我环顾四周");
        assert!(saw_replay_constraint.load(Ordering::SeqCst), "重写约束应注入 renderer");
        let previous_variant = replay.previous_variant_id.clone().unwrap();
        assert_ne!(previous_variant, replay.variant_id.clone().unwrap());
        let events = std::fs::read_to_string(run.join("events.jsonl")).unwrap();
        assert_eq!(events.lines().filter(|l| !l.trim().is_empty()).count(), 1, "{events}");
        let variant_files: Vec<_> = std::fs::read_dir(run.join("variants/turn-1")).unwrap().collect();
        assert_eq!(variant_files.len(), 2, "变体对：{variant_files:?}");

        // 恢复第一版：场景甲回放。
        let restored = runner.restore_variant(1, &previous_variant).await.unwrap();
        assert_eq!(restored.scene_text, "场景甲：灯笼的光晃了一下。");
        let scene_md = std::fs::read_to_string(run.join("projections/scene.md")).unwrap();
        assert!(scene_md.contains("场景甲"), "{scene_md}");

        // 无回合可重做 / 变体缺失的错误面。
        let fresh = PlayRunner {
            project_root: &root,
            world_id: "w79e2e".to_string(),
            run_id: "empty".to_string(),
        };
        let error = fresh
            .regenerate_last_turn(&agents, &agents, &agents, Some(&agents), None)
            .await
            .unwrap_err();
        assert_eq!(error, "No Play turn to regenerate.");
        let missing = runner.restore_variant(5, "v-none").await.unwrap_err();
        assert!(missing.contains("Play variant not found: turn 5 / v-none"), "{missing}");
        let _ = StatusCode::OK;
    }
}

mod play80_e2e {
    //! 80 号：play_step / play_revise 聊天工具面——chat 循环内分发 +
    //! play 会话工具面注册 + play 系统提示词（mock 捕获）+ 落盘推进/重做。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn rt80(root: &std::path::Path, llm: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: llm.into(),
                    api_key: "k".into(),
                    max_tokens: 8192,
                    model: "m".into(),
                    extra_headers: HashMap::new(),
                },
                HashMap::new(),
            )),
            builtin_genres_dir: root.join("assets").join("genres"),
            revision_gate: Default::default(),
        }
    }

    fn app80(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
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

    /// mock：play 五代理（同 79 号）+ studio-agent 聊天面（play 系统提示词 →
    /// 按指令关键词发 play_step / play_revise 工具调用，收到 tool 结果后收束
    /// 成终文）。捕获 tools 名单与 system 文本供注册面断言。
    async fn mock_play80_llm() -> (String, Arc<Mutex<Vec<String>>>, Arc<Mutex<String>>, Arc<AtomicUsize>) {
        let tool_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let systems: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let render_calls = Arc::new(AtomicUsize::new(0));
        let tools_in = tool_names.clone();
        let systems_in = systems.clone();
        let render_in = render_calls.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let tools_in = tools_in.clone();
                let systems_in = systems_in.clone();
                let render_in = render_in.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                    let content = if system.contains("动作理解器") {
                        serde_json::json!({ "actionKind": "look", "intent": "环顾四周", "secondaryActions": [] }).to_string()
                    } else if system.contains("世界状态草案员") {
                        let user = messages.get(1).and_then(|m| m["content"].as_str()).unwrap_or("");
                        if user.contains("只播种这个互动世界开场已经成立的状态") {
                            serde_json::json!({
                                "eventId": "evt-0", "turn": 0, "actionKind": "look",
                                "summary": "开场状态已播种",
                                "entities": { "upsert": [
                                    { "id": "actor_player", "type": "actor", "label": "夜行人", "summary": "潜入者", "updatedEventId": "evt-0" },
                                    { "id": "item_lantern", "type": "item", "label": "灯笼", "summary": "照明", "updatedEventId": "evt-0" }
                                ]},
                                "edges": { "upsert": [
                                    { "fromId": "actor_player", "type": "持有", "toId": "item_lantern",
                                      "value": { "role": "holding" }, "validFromEventId": "evt-0", "sourceEventId": "evt-0" }
                                ]}
                            }).to_string()
                        } else {
                            serde_json::json!({
                                "eventId": "evt-1", "turn": 1, "actionKind": "look",
                                "summary": "玩家看清了厅堂",
                                "timeAdvance": { "elapsed": "片刻", "anchor": "深夜", "rationale": "环顾", "synchronized": [] },
                                "entities": { "upsert": [
                                    { "id": "location_hall", "type": "location", "label": "厅堂", "summary": "正厅", "updatedEventId": "evt-1" }
                                ]}
                            }).to_string()
                        }
                    } else if system.contains("互动小说场景") {
                        let n = render_in.fetch_add(1, Ordering::SeqCst);
                        let scene = if n == 0 {
                            serde_json::json!({ "sceneText": "场景甲：灯笼的光晃了一下。", "suggestedActions": [] })
                        } else {
                            serde_json::json!({ "sceneText": "场景乙：重写后的厅堂静得反常。", "suggestedActions": [] })
                        };
                        scene.to_string()
                    } else if system.contains("把互动小说正文和世界图谱对齐") {
                        serde_json::json!({
                            "eventId": "evt-1", "turn": 1, "actionKind": "look",
                            "summary": "正文补充：纽扣落地",
                            "entities": { "upsert": [
                                { "id": "clue_button", "type": "clue", "label": "铜纽扣", "summary": "灯下闪光", "updatedEventId": "evt-1" }
                            ]},
                            "notes": ["对账补充"]
                        }).to_string()
                    } else {
                        // studio-agent 聊天面：捕获 tools/system；按指令关键词分流
                        // 工具调用，tool 结果回填后收束终文。
                        *systems_in.lock().unwrap() = system.clone();
                        if let Some(tools) = body["tools"].as_array() {
                            let names = tools
                                .iter()
                                .filter_map(|t| t["function"]["name"].as_str().map(String::from))
                                .collect::<Vec<_>>();
                            *tools_in.lock().unwrap() = names;
                        }
                        let last_user = messages
                            .iter()
                            .rev()
                            .find(|m| m["role"] == "user")
                            .and_then(|m| m["content"].as_str())
                            .unwrap_or("")
                            .to_string();
                        let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                        if has_tool_result {
                            if last_user.contains("重来") {
                                "（上一回合已重做。）".to_string()
                            } else if last_user.contains("恢复") {
                                "（恢复未完成。）".to_string()
                            } else {
                                "（这一步推进完成。）".to_string()
                            }
                        } else {
                            let (name, arguments) = if last_user.contains("重来") {
                                ("play_revise", r#"{"action":"regenerate_last"}"#.to_string())
                            } else if last_user.contains("恢复") {
                                ("play_revise", r#"{"action":"restore_variant","turn":1,"variantId":"v-none"}"#.to_string())
                            } else {
                                ("play_step", r#"{"input":"我环顾四周"}"#.to_string())
                            };
                            let call = serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_play_1", "function": { "name": name, "arguments": arguments } },
                            ] } }] });
                            let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                            return axum::response::IntoResponse::into_response((
                                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                                format!("data: {call}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                            ));
                        }
                    };
                    let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
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
        (format!("http://{addr}"), tool_names, systems, render_calls)
    }

    async fn seed_world(root: &std::path::Path, world_id: &str) {
        inkos_engine::play::create_world(
            root,
            &inkos_engine::play::PlayWorldInput {
                id: world_id,
                title: "厅堂夜探",
                premise: "深夜宅邸",
                world_contract: "",
                visual_contract: "",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn play_step_chat_tool_advances_session_world() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, tool_names, systems, _render_calls) = mock_play80_llm().await;
        let session_id = "1783001000001-p80a";
        let runtime = rt80(&root, &llm);
        let app = app80(runtime);

        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        seed_world(&root, session_id).await;

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"我环顾四周","sessionId":"{session_id}","sessionKind":"play"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（这一步推进完成。）");

        // 工具执行卡：play_step 完成，结果文本 = 场景正文（场景甲）。
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0]["tool"], "play_step");
        assert_eq!(execs[0]["status"], "completed");
        assert!(execs[0]["result"].as_str().unwrap().contains("场景甲"), "body: {parsed}");

        // 注册面：tools 含 play_step/play_revise（105 号：play 会话无文件三件）。
        let names = tool_names.lock().unwrap().clone();
        assert!(names.contains(&"play_step".to_string()), "{names:?}");
        assert!(names.contains(&"play_revise".to_string()), "{names:?}");
        assert!(!names.contains(&"read".to_string()), "play 会话不应注册 read：{names:?}");
        let system = systems.lock().unwrap().clone();
        assert!(system.contains("【铁律】") && system.contains("play_step"), "{system}");

        // 落盘：事件 + state + 场景投影 + 图（会话绑定的世界）。
        let run = root.join("worlds").join(session_id).join("runs").join("main");
        let events = std::fs::read_to_string(run.join("events.jsonl")).unwrap();
        assert!(events.contains("evt-1") && events.contains("看清了厅堂"), "{events}");
        let current: Value =
            serde_json::from_str(&std::fs::read_to_string(run.join("state").join("current.json")).unwrap()).unwrap();
        assert_eq!(current["turn"], 1);
        let scene = std::fs::read_to_string(run.join("projections").join("scene.md")).unwrap();
        assert!(scene.contains("场景甲"), "{scene}");
        let graph: Value =
            serde_json::from_str(&std::fs::read_to_string(run.join("play-graph.json")).unwrap()).unwrap();
        assert!(graph["entities"]
            .as_object()
            .is_some_and(|entities| entities.contains_key("location_hall")), "graph: {graph}");
    }

    #[tokio::test]
    async fn play_revise_chat_tool_regenerates_and_restore_error_surfaces() {
        use inkos_engine::play_runner::{PlayAgents, PlayRunner};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, _tool_names, _systems, _render_calls) = mock_play80_llm().await;
        let session_id = "1783001000002-p80b";
        let runtime = rt80(&root, &llm);
        let shared_router = runtime.router.clone();
        let app = app80(runtime);

        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        seed_world(&root, session_id).await;

        // 直调 runner 先走一回合（场景甲），聊天面负责重做。
        let agents = PlayAgents { router: &shared_router, root: &root };
        let runner = PlayRunner {
            project_root: &root,
            world_id: session_id.to_string(),
            run_id: "main".to_string(),
        };
        runner.seed_opening(&agents, "雨夜开场。", &[]).await.unwrap();
        let step1 = runner
            .step(&agents, &agents, &agents, Some(&agents), "我环顾四周", None)
            .await
            .unwrap();
        assert_eq!(step1.scene_text, "场景甲：灯笼的光晃了一下。");

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"重来上一回合","sessionId":"{session_id}","sessionKind":"play"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（上一回合已重做。）");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0]["tool"], "play_revise");
        assert_eq!(execs[0]["status"], "completed");
        assert!(execs[0]["result"].as_str().unwrap().contains("场景乙"), "body: {parsed}");

        // 重做落盘：变体对 + 事件不涨。
        let run = root.join("worlds").join(session_id).join("runs").join("main");
        let events = std::fs::read_to_string(run.join("events.jsonl")).unwrap();
        assert_eq!(events.lines().filter(|l| !l.trim().is_empty()).count(), 1, "{events}");
        let variant_files: Vec<_> = std::fs::read_dir(run.join("variants").join("turn-1")).unwrap().collect();
        assert_eq!(variant_files.len(), 2, "变体对：{variant_files:?}");

        // 恢复不存在变体：错误透传为 error 执行卡（TS restore 分支无 catch）。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"恢复第一版","sessionId":"{session_id}","sessionKind":"play"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs[0]["tool"], "play_revise");
        assert_eq!(execs[0]["status"], "error");
        assert!(
            execs[0]["error"].as_str().unwrap().contains("Play variant not found: turn 1 / v-none"),
            "body: {parsed}"
        );
    }
}

mod play81_e2e {
    //! 81 号：play_edit 聊天工具——改世界契约/persona/实体卡不推进回合，
    //! world.json + currentState 写回（chat 循环内分发，零 LLM 域调用）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;
    use serde_json::Value;

    fn rt81(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app81(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
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

    /// mock：studio-agent 聊天面（play 提示词）——按指令发 play_edit 工具调用，
    /// tool 结果回填后收束终文；捕获 tools 名单。
    async fn mock_play81_llm() -> (String, Arc<Mutex<Vec<String>>>) {
        let tool_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let tools_in = tool_names.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let tools_in = tools_in.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("");
                    assert!(system.contains("InkOS Play 助手"), "play 系统提示词：{system}");
                    if let Some(tools) = body["tools"].as_array() {
                        let names = tools
                            .iter()
                            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
                            .collect::<Vec<_>>();
                        *tools_in.lock().unwrap() = names;
                    }
                    let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                    let (content, is_tool_call) = if has_tool_result {
                        ("（世界规则已更新。）".to_string(), false)
                    } else {
                        (r#"{"worldContractReplacements":[{"from":"可能追责 / 不能公开","to":"涉及追责 / 需要主任签字"}],"playerPersona":"我是查清停电夜的租客。","note":"风险重量已替换。"}"#.to_string(), true)
                    };
                    let payload = if is_tool_call {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_edit_1", "function": { "name": "play_edit", "arguments": content } },
                        ] } }] })
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": content } }] })
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), tool_names)
    }

    #[tokio::test]
    async fn play_edit_chat_tool_persists_contracts_without_advancing_turn() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, tool_names) = mock_play81_llm().await;
        let session_id = "1783002000001-p81a";
        let runtime = rt81(&root, &llm);
        let app = app81(runtime);

        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        inkos_engine::play::create_world(
            &root,
            &inkos_engine::play::PlayWorldInput {
                id: session_id,
                title: "午夜药房",
                premise: "实习药剂师值夜班。",
                world_contract: "风险重量：普通差错 / 需要复核 / 可能追责 / 不能公开。",
                visual_contract: "监控冷光。",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();
        inkos_engine::play::ensure_run(&root, session_id, "main").await.unwrap();
        inkos_engine::play::save_current_state(
            &root,
            session_id,
            "main",
            &serde_json::json!({ "turn": 2, "lastEventId": "evt-2" }),
        )
        .await
        .unwrap();

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"把规则里的可能追责改成涉及追责并需要主任签字","sessionId":"{session_id}","sessionKind":"play"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（世界规则已更新。）");

        // 注册面：play_edit 在 tools 里。
        let names = tool_names.lock().unwrap().clone();
        assert!(names.contains(&"play_edit".to_string()), "{names:?}");
        assert!(names.contains(&"play_step".to_string()), "{names:?}");

        // 执行卡：completed，result = note 文本。
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0]["tool"], "play_edit");
        assert_eq!(execs[0]["status"], "completed");
        assert_eq!(execs[0]["result"], "风险重量已替换。");

        // world.json：替换生效 + updatedAt 重写。
        let world = inkos_engine::play::load_world(&root, session_id).await.unwrap();
        let contract = world["worldContract"].as_str().unwrap();
        assert!(contract.contains("涉及追责 / 需要主任签字"), "{contract}");
        assert!(!contract.contains("可能追责 / 不能公开"), "{contract}");

        // currentState：合并写回（turn 保留 + graphEditedAt + 新契约）。
        let state = inkos_engine::play::load_current_state(&root, session_id, "main")
            .await
            .unwrap();
        assert_eq!(state["turn"], 2);
        assert_eq!(state["lastEventId"], "evt-2");
        assert!(
            state["worldContract"].as_str().unwrap().contains("涉及追责"),
            "{state}"
        );
        assert!(state["graphEditedAt"].as_str().is_some_and(|v| !v.is_empty()));

        // persona → actor_player 实体（manual-edit 事件位）。
        let run_dir = root.join("worlds").join(session_id).join("runs").join("main");
        let graph: Value =
            serde_json::from_str(&std::fs::read_to_string(run_dir.join("play-graph.json")).unwrap()).unwrap();
        let player = graph["entities"]
            .as_object()
            .unwrap()
            .get("actor_player")
            .cloned()
            .unwrap_or(Value::Null);
        assert_eq!(player["summary"], "我是查清停电夜的租客。", "graph: {graph}");
        assert_eq!(player["updatedEventId"], "manual-edit");
        // 不推进回合：无事件新增。
        assert!(!run_dir.join("events.jsonl").exists());
    }
}

mod play82_e2e {
    //! 82 号：play en 提示词全链——en 世界的 interpreter/mutator/renderer/
    //! reconciler 四代理走 en 系统提示词与 en 标签；renderer 空输出 → en
    //! 兜底文案。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::state::manager::StateManager;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn rt82(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    /// mock：四代理 en 分流 + 首个 system/user 文本捕获；renderer 空输出模式
    /// （fail-open 兜底测试）。
    async fn mock_play82_llm(renderer_empty: bool) -> (String, Arc<Mutex<Vec<String>>>) {
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let render_calls = Arc::new(AtomicUsize::new(0));
        let captured_in = captured.clone();
        let render_in = render_calls.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let captured_in = captured_in.clone();
                let render_in = render_in.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                    let user = messages.get(1).and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                    if system.contains("action interpreter") {
                        captured_in.lock().unwrap().push(format!("INTERPRETER_SYS:{system}"));
                        captured_in.lock().unwrap().push(format!("INTERPRETER_USER:{user}"));
                    } else if system.contains("world-state drafter") {
                        captured_in.lock().unwrap().push(format!("MUTATOR_SYS:{system}"));
                        captured_in.lock().unwrap().push(format!("MUTATOR_USER:{user}"));
                    } else if system.contains("scene-response author") {
                        captured_in.lock().unwrap().push(format!("RENDERER_SYS:{system}"));
                        captured_in.lock().unwrap().push(format!("RENDERER_USER:{user}"));
                    } else if system.contains("You reconcile") {
                        captured_in.lock().unwrap().push(format!("RECONCILER_SYS:{system}"));
                    }
                    let content = if system.contains("action interpreter") {
                        serde_json::json!({ "actionKind": "look", "intent": "survey the hall", "secondaryActions": [] }).to_string()
                    } else if system.contains("world-state drafter") {
                        if user.contains("Seed only the state") {
                            serde_json::json!({
                                "eventId": "evt-0", "turn": 0, "actionKind": "look",
                                "summary": "Opening state seeded",
                                "entities": { "upsert": [
                                    { "id": "actor_player", "type": "actor", "label": "The Tenant", "summary": "New tenant", "updatedEventId": "evt-0" },
                                    { "id": "item_ledger", "type": "item", "label": "Ledger", "summary": "On the counter", "updatedEventId": "evt-0" }
                                ]},
                                "edges": { "upsert": [
                                    { "fromId": "actor_player", "type": "holds", "toId": "item_ledger",
                                      "value": { "role": "holding" }, "validFromEventId": "evt-0", "sourceEventId": "evt-0" }
                                ]}
                            }).to_string()
                        } else {
                            serde_json::json!({
                                "eventId": "evt-1", "turn": 1, "actionKind": "look",
                                "summary": "The tenant surveys the hall",
                                "timeAdvance": { "elapsed": "a few breaths", "anchor": "still night", "rationale": "a glance", "synchronized": [] },
                                "entities": { "upsert": [
                                    { "id": "location_hall", "type": "location", "label": "Hall", "summary": "The main hall", "updatedEventId": "evt-1" }
                                ]}
                            }).to_string()
                        }
                    } else if system.contains("scene-response author") {
                        let _n = render_in.fetch_add(1, Ordering::SeqCst);
                        if renderer_empty {
                            String::new()
                        } else {
                            serde_json::json!({
                                "sceneText": "The lantern gutters; the hall holds its breath.",
                                "suggestedActions": []
                            }).to_string()
                        }
                    } else if system.contains("You reconcile") {
                        serde_json::json!({
                            "eventId": "evt-1", "turn": 1, "actionKind": "look",
                            "summary": "Prose supplement: a button lies on the floor",
                            "entities": { "upsert": [
                                { "id": "clue_button", "type": "clue", "label": "Brass button", "summary": "catches the light", "updatedEventId": "evt-1" }
                            ]},
                            "notes": ["reconciled"]
                        }).to_string()
                    } else {
                        "PASS".to_string()
                    };
                    let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
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
        (format!("http://{addr}"), captured)
    }

    async fn seed_en_world(root: &std::path::Path, world_id: &str) {
        inkos_engine::play::create_world(
            root,
            &inkos_engine::play::PlayWorldInput {
                id: world_id,
                title: "Manor on a Snowy Night",
                premise: "A snowbound manor hides an old case.",
                world_contract: "Time flows by action semantics.",
                visual_contract: "Cold lantern light, no game UI.",
                mode: "open",
                language: "en",
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn en_world_full_chain_uses_english_agent_prompts() {
        use inkos_engine::play_runner::{PlayAgents, PlayRunner};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, captured) = mock_play82_llm(false).await;
        let runtime = rt82(&root, &llm);
        seed_en_world(&root, "w82en").await;

        let agents = PlayAgents { router: &runtime.router, root: &root };
        let runner = PlayRunner {
            project_root: &root,
            world_id: "w82en".to_string(),
            run_id: "main".to_string(),
        };
        runner.seed_opening(&agents, "The hall is dim.", &[]).await.unwrap();
        let step = runner
            .step(&agents, &agents, &agents, Some(&agents), "I look around", None)
            .await
            .unwrap();
        assert_eq!(step.scene_text, "The lantern gutters; the hall holds its breath.");

        // 四代理 en 系统提示词 + en user 标签逐项断言。
        let logs = captured.lock().unwrap().clone();
        let get = |prefix: &str| logs.iter().find(|l| l.starts_with(prefix)).cloned().unwrap_or_default();
        let interpreter_sys = get("INTERPRETER_SYS:");
        assert!(interpreter_sys.contains("You are an interactive-fiction action interpreter."), "{interpreter_sys}");
        let interpreter_user = get("INTERPRETER_USER:");
        assert!(interpreter_user.contains("Current scene:"), "{interpreter_user}");
        assert!(interpreter_user.contains("Player input:"), "{interpreter_user}");
        let mutator_sys = get("MUTATOR_SYS:");
        assert!(mutator_sys.contains("You are an interactive-fiction world-state drafter."), "{mutator_sys}");
        let mutator_user = get("MUTATOR_USER:");
        assert!(mutator_user.contains("Player's words:"), "{mutator_user}");
        assert!(mutator_user.contains("Action interpretation:"), "{mutator_user}");
        // 开场播种（首次 mutator 调用）走 en 播种指令；step 调用走 en 标签。
        assert!(logs.iter().any(|l| l.starts_with("MUTATOR_USER:") && l.contains("Seed only the state")), "{logs:?}");
        let renderer_sys = get("RENDERER_SYS:");
        assert!(renderer_sys.contains("You are an interactive-fiction scene-response author."), "{renderer_sys}");
        let renderer_user = get("RENDERER_USER:");
        assert!(renderer_user.contains("World setting (always obey):"), "{renderer_user}");
        assert!(renderer_user.contains("Player's words:"), "{renderer_user}");
        assert!(renderer_user.contains("Applied changes this turn:"), "{renderer_user}");
        assert!(renderer_user.contains("Current state summary:"), "{renderer_user}");
        let reconciler_sys = get("RECONCILER_SYS:");
        assert!(reconciler_sys.contains("You reconcile an interactive-fiction scene"), "{reconciler_sys}");

        // 落盘：事件 + 场景 + 图（reconcile 补充入图）。
        let run = root.join("worlds/w82en/runs/main");
        let events = std::fs::read_to_string(run.join("events.jsonl")).unwrap();
        assert!(events.contains("evt-1") && events.contains("surveys the hall"), "{events}");
        let scene = std::fs::read_to_string(run.join("projections/scene.md")).unwrap();
        assert!(scene.contains("lantern gutters"), "{scene}");
        let graph: Value =
            serde_json::from_str(&std::fs::read_to_string(run.join("play-graph.json")).unwrap()).unwrap();
        assert!(graph["entities"].as_object().unwrap().contains_key("clue_button"), "graph: {graph}");
        let _ = StatusCode::OK;
    }

    #[tokio::test]
    async fn renderer_en_empty_output_falls_back_to_english_placeholder() {
        use inkos_engine::play_runner::{PlayAgents, PlayRunner};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, _captured) = mock_play82_llm(true).await;
        let runtime = rt82(&root, &llm);
        seed_en_world(&root, "w82fb").await;

        let agents = PlayAgents { router: &runtime.router, root: &root };
        let runner = PlayRunner {
            project_root: &root,
            world_id: "w82fb".to_string(),
            run_id: "main".to_string(),
        };
        runner.seed_opening(&agents, "The hall is dim.", &[]).await.unwrap();
        let step = runner
            .step(&agents, &agents, &agents, Some(&agents), "I look around", None)
            .await
            .unwrap();
        // renderer 三轮空输出 → en 兜底文案（不抛错，回合仍提交）。
        assert_eq!(step.scene_text, "(The moment holds, unresolved.)");
        let events = std::fs::read_to_string(
            root.join("worlds/w82fb/runs/main/events.jsonl"),
        )
        .unwrap();
        assert!(events.contains("evt-1"), "{events}");
    }
}

mod material83_e2e {
    //! 83 号：material 聊天工具面——ingest(file) → retrieve 全链 + URL 抓取
    //! 归档（真实 reqwest + mock HTTP 源）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt83(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app83(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
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

    /// mock LLM：按 tool 结果数分流——0 → ingest_material(file)；1 →
    /// retrieve_material；≥2 → 终文。捕获 tools 名单。
    async fn mock_material_llm() -> (String, Arc<Mutex<Vec<String>>>) {
        let tool_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let tools_in = tool_names.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let tools_in = tools_in.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    if let Some(tools) = body["tools"].as_array() {
                        let names = tools
                            .iter()
                            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
                            .collect::<Vec<_>>();
                        *tools_in.lock().unwrap() = names;
                    }
                    let tool_results = messages.iter().filter(|m| m["role"] == "tool").count();
                    let payload = if tool_results == 0 {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_ing_1", "function": { "name": "ingest_material", "arguments": "{\"sourceKind\":\"file\",\"filePath\":\"材料.md\",\"title\":\"冷库账页\"}" } },
                        ] } }] })
                    } else if tool_results == 1 {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_ret_1", "function": { "name": "retrieve_material", "arguments": "{\"query\":\"冷库赔偿款\"}" } },
                        ] } }] })
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "（材料已归档并召回关键片段。）" } }] })
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), tool_names)
    }

    #[tokio::test]
    async fn chat_ingests_file_then_retrieves_snippet() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(
            root.join("材料.md"),
            "第 0607 号账页记载：冷库赔偿款已分三期拨付，经手人签字齐全。",
        )
        .unwrap();
        let (llm, tool_names) = mock_material_llm().await;
        let session_id = "1783003000001-m83a";
        let app = app83(rt83(&root, &llm));

        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"帮我归档这份材料再查冷库赔偿款","sessionId":"{session_id}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（材料已归档并召回关键片段。）");

        // 注册面：material 双工具对所有会话可见（105 号：chat 会话无文件三件）。
        let names = tool_names.lock().unwrap().clone();
        assert!(names.contains(&"ingest_material".to_string()), "{names:?}");
        assert!(names.contains(&"retrieve_material".to_string()), "{names:?}");
        assert!(!names.contains(&"read".to_string()), "chat 会话不应注册 read：{names:?}");

        // 两张执行卡：ingest → completed + 归档文本；retrieve → completed + 片段。
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 2);
        assert_eq!(execs[0]["tool"], "ingest_material");
        assert_eq!(execs[0]["status"], "completed");
        let ingest_text = execs[0]["result"].as_str().unwrap();
        assert!(ingest_text.starts_with("Material ingested: .inkos/materials/"), "{ingest_text}");
        assert!(ingest_text.contains("Kind: text"), "{ingest_text}");
        assert_eq!(execs[1]["tool"], "retrieve_material");
        assert_eq!(execs[1]["status"], "completed");
        let retrieve_text = execs[1]["result"].as_str().unwrap();
        assert!(retrieve_text.contains("Retrieved 1 material snippet."), "{retrieve_text}");
        assert!(retrieve_text.contains("## 1. 冷库账页"), "{retrieve_text}");
        assert!(retrieve_text.contains("0607"), "{retrieve_text}");

        // 磁盘：.inkos/materials 卡 + manifest（camelCase）。
        let materials_dir = root.join(".inkos").join("materials");
        let mut json_count = 0;
        let mut markdown_count = 0;
        let mut manifest = serde_json::Value::Null;
        for entry in std::fs::read_dir(&materials_dir).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                json_count += 1;
                manifest = serde_json::from_str(&std::fs::read_to_string(materials_dir.join(&name)).unwrap()).unwrap();
            } else if name.ends_with(".md") {
                markdown_count += 1;
            }
        }
        assert_eq!((json_count, markdown_count), (1, 1));
        assert_eq!(manifest["title"], "冷库账页");
        assert_eq!(manifest["kind"], "text");
        assert!(manifest["markdownPath"].as_str().unwrap().starts_with(".inkos/materials/"));
    }

    #[tokio::test]
    async fn chat_ingests_url_as_webpage() {
        // mock HTTP 源：HTML + title + content-type。
        let source_app = axum::Router::new().route(
            "/docs/page.html",
            axum::routing::get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    "<html><head><title>冷库事故调查</title></head><body><p>事故报告正文：赔偿款去向说明。</p></body></html>",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source_addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, source_app).await.unwrap(); });

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let url = format!("http://{source_addr}/docs/page.html");
        let tool_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let tools_in = tool_names.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let tools_in = tools_in.clone();
                let url = url.clone();
                async move {
                    let _ = tools_in;
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let has_tool = messages.iter().any(|m| m["role"] == "tool");
                    let arguments = format!(
                        "{{\"sourceKind\":\"url\",\"url\":\"{url}\",\"purpose\":\"research\"}}"
                    );
                    let payload = if has_tool {
                        serde_json::json!({ "choices": [{ "delta": { "content": "（网页已归档。）" } }] })
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_url_1", "function": { "name": "ingest_material", "arguments": arguments } },
                        ] } }] })
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let llm_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let llm_addr = llm_listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(llm_listener, app).await.unwrap(); });
        let llm = format!("http://{llm_addr}");

        let session_id = "1783003000002-m83b";
        let app = app83(rt83(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"把这个网页归档进材料库","sessionId":"{session_id}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（网页已归档。）");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0]["tool"], "ingest_material");
        let text = execs[0]["result"].as_str().unwrap();
        assert!(text.contains("Kind: webpage"), "{text}");
        assert!(text.contains("事故报告正文"), "{text}");

        let materials_dir = root.join(".inkos").join("materials");
        let entries: Vec<_> = std::fs::read_dir(&materials_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .collect();
        assert_eq!(entries.len(), 1);
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(entries[0].path()).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["title"], "冷库事故调查");
        assert_eq!(manifest["kind"], "webpage");
        assert_eq!(manifest["purpose"], "research");
        assert_eq!(manifest["source"], format!("http://{source_addr}/docs/page.html"));
    }
}

mod propose84_e2e {
    //! 84 号：propose_action 确认卡聊天工具——卡片生成（结构化 actionPayload
    //! 填充）+ 必填断言错误面 + play 有世界时的注册剔除。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt84(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app84(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
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

    /// mock：按 system 与指令关键词分流 propose_action 工具调用；tool 结果
    /// 回填后收束终文；捕获 tools 名单。
    async fn mock_propose_llm() -> (String, Arc<Mutex<Vec<String>>>) {
        let tool_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let tools_in = tool_names.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let tools_in = tools_in.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                    if let Some(tools) = body["tools"].as_array() {
                        let names = tools
                            .iter()
                            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
                            .collect::<Vec<_>>();
                        *tools_in.lock().unwrap() = names;
                    }
                    let last_user = messages
                        .iter()
                        .rev()
                        .find(|m| m["role"] == "user")
                        .and_then(|m| m["content"].as_str())
                        .unwrap_or("");
                    let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                    let payload = if has_tool_result {
                        let text = if last_user.contains("开一个") {
                            "（该会话不提供确认卡。）"
                        } else if last_user.contains("缺标题") {
                            "（补全标题后可重新确认。）"
                        } else {
                            "（确认卡已生成，等待用户确认。）"
                        };
                        serde_json::json!({ "choices": [{ "delta": { "content": text } }] })
                    } else if system.contains("InkOS Play 助手") || last_user.contains("开一个") {
                        // play 有世界的会话：模型越权调用 → Unknown tool 错误面。
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_prop_1", "function": { "name": "propose_action", "arguments": "{\"action\":\"short_run\",\"instruction\":\"写个短篇\"}" } },
                        ] } }] })
                    } else if last_user.contains("缺标题") {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_prop_2", "function": { "name": "propose_action", "arguments": "{\"action\":\"create_book\",\"instruction\":\"写一本悬疑小说\",\"createBook\":{\"genre\":\"悬疑\"}}" } },
                        ] } }] })
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_prop_3", "function": { "name": "propose_action", "arguments": "{\"action\":\"create_book\",\"instruction\":\"写一本《雪夜谜案》悬疑小说，主角是刑警林昭，番茄平台连载。\",\"createBook\":{\"title\":\"雪夜谜案\",\"genre\":\"悬疑\",\"platform\":\"tomato\",\"targetChapters\":200}}" } },
                        ] } }] })
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), tool_names)
    }

    #[tokio::test]
    async fn propose_action_confirmation_card_from_chat() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, tool_names) = mock_propose_llm().await;
        let session_id = "1783004000001-p84a";
        let app = app84(rt84(&root, &llm));

        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"帮我建书：雪夜谜案悬疑长篇","sessionId":"{session_id}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（确认卡已生成，等待用户确认。）");

        // 注册面：propose_action + material + 文件工具同现。
        let names = tool_names.lock().unwrap().clone();
        assert!(names.contains(&"propose_action".to_string()), "{names:?}");
        assert!(names.contains(&"ingest_material".to_string()), "{names:?}");

        // 卡片：completed + 四行文本（回退标题/摘要 + Instruction）。
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0]["tool"], "propose_action");
        assert_eq!(execs[0]["status"], "completed");
        let text = execs[0]["result"].as_str().unwrap();
        assert_eq!(
            text,
            "创建长篇书籍\n确认后会切换到对应入口并执行这条需求。\n\nInstruction: 写一本《雪夜谜案》悬疑小说，主角是刑警林昭，番茄平台连载。"
        );
    }

    #[tokio::test]
    async fn propose_action_missing_title_error_and_play_world_exclusion() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, tool_names) = mock_propose_llm().await;
        let chat_session = "1783004000002-p84b";
        let play_session = "1783004000003-p84c";
        let app = app84(rt84(&root, &llm));

        for session in [chat_session, play_session] {
            let (status, _) = call(
                app.clone(),
                "POST",
                "/api/v1/sessions",
                Some(&format!(r#"{{"sessionId":"{session}"}}"#)),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }
        // play 会话绑定世界（propose_action 应被剔除）。
        inkos_engine::play::create_world(
            &root,
            &inkos_engine::play::PlayWorldInput {
                id: play_session,
                title: "厅堂夜探",
                premise: "深夜宅邸",
                world_contract: "",
                visual_contract: "",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();

        // chat 会话：create_book 缺 createBook.title → 固定错误文案卡。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"缺标题的建书请求","sessionId":"{chat_session}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs[0]["status"], "error");
        assert_eq!(
            execs[0]["error"],
            "propose_action is missing /createBook/title; retry with that field in the structured payload, not only in summary or instruction."
        );

        // play 有世界：tools 不含 propose_action；模型越权调用 → Unknown tool。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"开一个短篇确认","sessionId":"{play_session}","sessionKind":"play"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（该会话不提供确认卡。）");
        let names = tool_names.lock().unwrap().clone();
        assert!(!names.contains(&"propose_action".to_string()), "play 有世界不注册：{names:?}");
        assert!(names.contains(&"play_edit".to_string()), "{names:?}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs[0]["tool"], "propose_action");
        assert_eq!(execs[0]["status"], "error");
        assert!(
            execs[0]["error"].as_str().unwrap().contains("Unknown tool: propose_action"),
            "body: {parsed}"
        );
    }
}

mod research85_e2e {
    //! 85 号：research_web 聊天工具——mock Tavily 搜索源 + HTML 抓取源驱动
    //! 确定性研究链，报告落 .inkos/research/。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt85(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app85(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
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

    #[tokio::test]
    async fn chat_research_web_saves_report_from_mock_tavily() {
        // 同一 mock server：/search（Tavily）+ /docs/a、/docs/b（HTML 抓取源）
        // + /chat/completions（studio-agent 聊天分流）。
        let tool_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let search_hits: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let source_base = format!("http://{addr}");
        let tools_in = tool_names.clone();
        let search_hits_in = search_hits.clone();
        let search_base = source_base.clone();
        let app = axum::Router::new()
            .route(
                "/search",
                axum::routing::post(move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<serde_json::Value>| {
                    let search_hits_in = search_hits_in.clone();
                    let search_base = search_base.clone();
                    async move {
                        assert_eq!(
                            headers.get("authorization").and_then(|v| v.to_str().ok()),
                            Some("Bearer k"),
                            "Tavily Bearer 凭据"
                        );
                        search_hits_in
                            .lock()
                            .unwrap()
                            .push(body["query"].as_str().unwrap_or("").to_string());
                        axum::Json(serde_json::json!({
                            "results": [
                                { "title": "冷库账页史料", "url": format!("{search_base}/docs/a"), "content": "1990 年代县冷库采用三联账制度。" },
                                { "title": "冷库赔偿流程", "url": format!("{search_base}/docs/b"), "content": "赔偿需主任签字。" }
                            ]
                        }))
                    }
                }),
            )
            .route(
                "/docs/:page",
                axum::routing::get(|| async {
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                        "<html><head><title>冷库史料</title></head><body><p>冷库夜班每两小时抄表一次。账页分三联存根。这是第三句。第四句超限。</p></body></html>",
                    )
                }),
            )
            .route(
                "/chat/completions",
                axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let tools_in = tools_in.clone();
                    async move {
                        let messages = body["messages"].as_array().cloned().unwrap_or_default();
                        if let Some(tools) = body["tools"].as_array() {
                            let names = tools
                                .iter()
                                .filter_map(|t| t["function"]["name"].as_str().map(String::from))
                                .collect::<Vec<_>>();
                            *tools_in.lock().unwrap() = names;
                        }
                        let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                        let payload = if has_tool_result {
                            serde_json::json!({ "choices": [{ "delta": { "content": "（研究资料已归档。）" } }] })
                        } else {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_res_1", "function": { "name": "research_web", "arguments": "{\"topic\":\"1990 年代县冷库会计流程\",\"purpose\":\"era\",\"depth\":\"quick\"}" } },
                            ] } }] })
                        };
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                        axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ))
                    }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let base = source_base;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // researchSearch 配置指向 mock Tavily（enabled + baseUrl）。
        std::fs::write(
            root.join("inkos.json"),
            serde_json::json!({ "researchSearch": { "enabled": true, "apiKey": "k", "baseUrl": format!("{base}/search") } }).to_string(),
        )
        .unwrap();

        let session_id = "1783005000001-r85a";
        let app = app85(rt85(&root, &base));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"帮我查证 1990 年代县冷库会计流程","sessionId":"{session_id}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（研究资料已归档。）");

        // 注册面：research_web 与 material/propose 同现。
        let names = tool_names.lock().unwrap().clone();
        assert!(names.contains(&"research_web".to_string()), "{names:?}");
        assert!(names.contains(&"propose_action".to_string()), "{names:?}");

        // 执行卡：completed + 三行文本（2 源 medium + 无失败）。
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0]["tool"], "research_web");
        assert_eq!(execs[0]["status"], "completed");
        let text = execs[0]["result"].as_str().unwrap();
        let lines: Vec<&str> = text.split('\n').collect();
        assert!(lines[0].starts_with("Research report saved: "), "{text}");
        assert_eq!(lines[1], "Sources: 2; confidence: medium.");
        assert_eq!(lines[2], "Partial failures: none.");
        // quick：单查询（purpose hint 拼接）。
        let hits = search_hits.lock().unwrap().clone();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].contains("冷库会计流程"), "{hits:?}");
        assert!(hits[0].contains("年代 背景 制度 物价 生活"), "{hits:?}");

        // 报告落盘：标题/来源节/摘录前三句。
        let research_dir = root.join(".inkos").join("research");
        let entries: Vec<_> = std::fs::read_dir(&research_dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let markdown = std::fs::read_to_string(entries[0].as_ref().unwrap().path()).unwrap();
        assert!(markdown.starts_with("# Research: 1990 年代县冷库会计流程\n\n- Purpose: era\n- Depth: quick\n- Confidence: medium"), "{markdown}");
        assert!(markdown.contains("### [S1] 冷库账页史料"), "{markdown}");
        assert!(markdown.contains("冷库夜班每两小时抄表一次。账页分三联存根。这是第三句。"), "{markdown}");
        assert!(!markdown.contains("第四句超限"), "摘录只取前三句：{markdown}");
        assert!(markdown.contains("## Query log\n- 1990 年代县冷库会计流程 年代 背景 制度 物价 生活"), "{markdown}");
    }
}

mod import85_e2e {
    //! 85 号：import_chapters 聊天工具——聊天驱动全链导入（architect/analyzer
    //! mock）+ 既有章节守卫 + 无 bookId 守卫。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    const ARCHITECT_OUTPUT: &str = r#"=== SECTION: story_frame ===
## 分岔点
开篇之前。

=== SECTION: volume_map ===
### 第一卷（1-20章）新程
独立冲突开启。

=== SECTION: roles ===
---ROLE---
tier: major
name: 林动
---CONTENT---
## 核心标签
坚忍。

=== SECTION: book_rules ===
## 导入模式
- continuation

=== SECTION: pending_hooks ===
| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H01 | 0 | 新谜 | open | 0 | 第2卷 | 慢烧 | 无 | 第2卷 | true |  | 开篇之谜 |
"#;

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

    fn rt85b(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app85b(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
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

    async fn mock_import_llm() -> String {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                let messages = body["messages"].as_array().cloned().unwrap_or_default();
                let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                let payload = if system.contains("总架构师") || system.contains("网络小说架构师") {
                    sse_tool_chunk_free(ARCHITECT_OUTPUT)
                } else if system.contains("连续性分析") || system.contains("continuity analyst") {
                    sse_tool_chunk_free(ANALYZER_OUTPUT)
                } else {
                    // studio-agent：按指令分流（导入/二次导入守卫/无书）。
                    let last_user = messages
                        .iter()
                        .rev()
                        .find(|m| m["role"] == "user")
                        .and_then(|m| m["content"].as_str())
                        .unwrap_or("");
                    let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                    if has_tool_result {
                        let text = if last_user.contains("二次") {
                            "（需要 resumeFrom 才能追加。）"
                        } else if last_user.contains("没有书号") {
                            "（需要 bookId。）"
                        } else {
                            "（章节已导入，可以续写。）"
                        };
                        serde_json::json!({ "choices": [{ "delta": { "content": text } }] })
                    } else if last_user.contains("二次") {
                        tool_call("call_imp_2", "import_chapters", r#"{"bookId":"b85","sourcePath":"novel.txt"}"#)
                    } else if last_user.contains("没有书号") {
                        tool_call("call_imp_3", "import_chapters", r#"{"sourcePath":"novel.txt"}"#)
                    } else {
                        tool_call("call_imp_1", "import_chapters", r#"{"bookId":"b85","sourcePath":"novel.txt"}"#)
                    }
                };
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn sse_tool_chunk_free(content: &str) -> serde_json::Value {
        serde_json::json!({ "choices": [{ "delta": { "content": content } }] })
    }

    fn tool_call(id: &str, name: &str, arguments: &str) -> serde_json::Value {
        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
            { "index": 0, "id": id, "function": { "name": name, "arguments": arguments } },
        ] } }] })
    }

    #[tokio::test]
    async fn chat_imports_chapters_full_chain_and_guards() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // genre + 目标书 + 章节源。
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
        let book = root.join("books").join("b85");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b85","title":"导入书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("novel.txt"),
            "# 第一章 风起\n\n林动睁开双眼，灵气涌动。\n\n# 第二章 云涌\n\n坊市喧闹。",
        )
        .unwrap();

        let llm = mock_import_llm().await;
        let session_id = "1783005000002-i85b";
        let app = app85b(rt85b(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // ① 全链导入。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"把 novel.txt 导入成书","sessionId":"{session_id}","activeBookId":"b85"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（章节已导入，可以续写。）");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0]["tool"], "import_chapters");
        assert_eq!(execs[0]["status"], "completed");
        let text = execs[0]["result"].as_str().unwrap();
        assert!(text.starts_with("Imported 2 chapter(s) into book \"b85\".\nTotal imported length: "), "{text}");
        assert!(text.contains("Next chapter to write: 3."), "{text}");
        assert!(text.contains("Foundation and truth files were reverse-engineered"), "{text}");
        // 落盘（复用 59 号链断言面）。
        assert!(book.join("chapters").join("0001_风起.md").exists());
        let index: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(book.join("chapters").join("index.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(index.as_array().unwrap().len(), 2);
        assert_eq!(index[0]["status"], "imported");

        // ② 既有章节 + 无 resumeFrom → 守卫错误。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"二次导入同一本","sessionId":"{session_id}","activeBookId":"b85"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs[0]["status"], "error");
        assert!(
            execs[0]["error"]
                .as_str()
                .unwrap()
                .starts_with("Book \"b85\" already has 2 chapter(s). Pass resumeFrom=<n> to resume/append from chapter n, or ask the user to clear the existing chapters first."),
            "body: {parsed}"
        );

        // ③ chat 会话无 active book 且未给 bookId → 守卫错误。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"没有书号也想导入","sessionId":"{session_id}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs[0]["status"], "error");
        assert_eq!(
            execs[0]["error"],
            "import_chapters requires bookId when there is no active book."
        );
    }
}

mod details86_e2e {
    //! 86 号：聊天面卡 details 外露——LoopToolExecution.details 透传到
    //! details.toolExecutions[i].details（propose 确认卡 / play 回合卡）。
    use super::*;
    use axum::http::StatusCode;
    use serde_json::json;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt86(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app86(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
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

    /// mock：studio-agent 聊天按指令关键词发 propose_action / play_step；
    /// play 四代理服务 play_step 域调用。
    async fn mock_details_llm() -> String {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                let messages = body["messages"].as_array().cloned().unwrap_or_default();
                let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                // 域代理 → 文本 chunk；studio-agent 聊天 → 直接 payload。
                let agent_content: Option<String> = if system.contains("动作理解器") {
                    Some(serde_json::json!({ "actionKind": "look", "intent": "环顾四周", "secondaryActions": [] }).to_string())
                } else if system.contains("世界状态草案员") {
                    Some(serde_json::json!({
                        "eventId": "evt-1", "turn": 1, "actionKind": "look",
                        "summary": "玩家看清了厅堂",
                        "entities": { "upsert": [
                            { "id": "actor_player", "type": "actor", "label": "夜行人", "summary": "潜入者", "updatedEventId": "evt-0" },
                            { "id": "location_hall", "type": "location", "label": "厅堂", "summary": "正厅", "updatedEventId": "evt-1" }
                        ]}
                    }).to_string())
                } else if system.contains("互动小说场景") {
                    Some(serde_json::json!({
                        "sceneText": "灯笼的光晃了一下，厅堂深处有人影一闪。",
                        "suggestedActions": ["追上去", "吹灭灯笼"]
                    }).to_string())
                } else if system.contains("把互动小说正文和世界图谱对齐") {
                    Some(serde_json::json!({
                        "eventId": "evt-1", "turn": 1, "actionKind": "look",
                        "summary": "", "entities": { "upsert": [] }, "notes": []
                    }).to_string())
                } else {
                    None
                };
                let payload = if let Some(content) = agent_content {
                    serde_json::json!({ "choices": [{ "delta": { "content": content } }] })
                } else {
                    // studio-agent 聊天：按指令分流。
                    let last_user = messages
                        .iter()
                        .rev()
                        .find(|m| m["role"] == "user")
                        .and_then(|m| m["content"].as_str())
                        .unwrap_or("");
                    let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                    if has_tool_result {
                        let text = if last_user.contains("建书") {
                            "（确认卡已生成。）"
                        } else {
                            "（回合已推进。）"
                        };
                        serde_json::json!({ "choices": [{ "delta": { "content": text } }] })
                    } else if last_user.contains("建书") {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_d1", "function": { "name": "propose_action", "arguments": "{\"action\":\"create_book\",\"instruction\":\"写一本《雪夜谜案》。\",\"createBook\":{\"title\":\"雪夜谜案\",\"genre\":\"悬疑\",\"platform\":\"tomato\"}}" } },
                        ] } }] })
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_d2", "function": { "name": "play_step", "arguments": "{\"input\":\"我环顾四周\"}" } },
                        ] } }] })
                    }
                };
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn propose_card_details_surface_structured_payload() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let llm = mock_details_llm().await;
        let session_id = "1783006000001-d86a";
        let app = app86(rt86(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"帮我建书雪夜谜案","sessionId":"{session_id}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "propose_action");
        // 结构化 details 外露：确认卡消费面。
        let details = &card["details"];
        assert_eq!(details["kind"], "proposed_action");
        assert_eq!(details["action"], "create_book");
        assert_eq!(details["targetSessionKind"], "book-create");
        assert_eq!(details["sameSession"], false);
        assert_eq!(details["instruction"], "写一本《雪夜谜案》。");
        assert_eq!(
            details["actionPayload"]["createBook"],
            json!({ "title": "雪夜谜案", "genre": "悬疑", "platform": "tomato" })
        );
    }

    #[tokio::test]
    async fn play_step_card_details_surface_graph_and_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let llm = mock_details_llm().await;
        let session_id = "1783006000002-d86b";
        let runtime = rt86(&root, &llm);
        let app = app86(runtime);
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // 世界绑定会话（含开场播种实体）。
        inkos_engine::play::create_world(
            &root,
            &inkos_engine::play::PlayWorldInput {
                id: session_id,
                title: "厅堂夜探",
                premise: "深夜宅邸",
                world_contract: "",
                visual_contract: "",
                mode: "open",
                language: "zh",
            },
        )
        .await
        .unwrap();

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"我环顾四周","sessionId":"{session_id}","sessionKind":"play"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（回合已推进。）");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "play_step");
        assert_eq!(card["status"], "completed");
        let details = &card["details"];
        assert_eq!(details["kind"], "play_turn_advanced");
        assert_eq!(details["worldId"], session_id);
        assert_eq!(details["runId"], "main");
        assert_eq!(details["title"], "厅堂夜探");
        assert_eq!(details["sceneText"], "灯笼的光晃了一下，厅堂深处有人影一闪。");
        assert_eq!(details["suggestedActions"], json!(["追上去", "吹灭灯笼"]));
        assert_eq!(details["currentState"]["turn"], 1);
        assert!(details["graph"]["entities"]
            .as_array()
            .is_some_and(|entities| entities.iter().any(|e| e["id"] == "location_hall")), "graph: {}", details["graph"]);
    }
}

mod sub87_e2e {
    //! 87 号：sub_agent 聊天工具——writer 单章全链（复用 write-next mock 分流）
    //! + auditor + exporter 落盘 + 书会话注册面。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;
    use serde_json::Value;

    fn rt87(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app87(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
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

    /// mock：域链分流（planner/writer/auditor/settler——同全局 mock_llm_chat）
    /// + studio-agent 聊天按指令发 sub_agent 工具调用；捕获 tools 名单。
    async fn mock_sub87_llm() -> (String, Arc<Mutex<Vec<String>>>) {
        let tool_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let tools_in = tool_names.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let tools_in = tools_in.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                    if let Some(tools) = body["tools"].as_array() {
                        let names = tools
                            .iter()
                            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
                            .collect::<Vec<_>>();
                        *tools_in.lock().unwrap() = names;
                    }
                    let domain_content: Option<String> = if system.contains("创作总编") {
                        Some(PLANNER_RESPONSE.to_string())
                    } else if system.contains("作家") || system.contains("写手") {
                        Some(WRITER_RESPONSE.to_string())
                    } else if system.contains("审稿") {
                        Some("PASS\n95".to_string())
                    } else {
                        None
                    };
                    let payload = if let Some(content) = domain_content {
                        let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                        return axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ));
                    } else if !system.contains("创作助手") && !system.contains("Play 助手") {
                        // settler / 压缩 / 分析 / 校验等次要调用给最小合法输出。
                        let chunk = serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] });
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                        return axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ));
                    } else {
                        // studio-agent 聊天：按指令关键词发 sub_agent 工具调用。
                        let last_user = messages
                            .iter()
                            .rev()
                            .find(|m| m["role"] == "user")
                            .and_then(|m| m["content"].as_str())
                            .unwrap_or("");
                        let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                        if has_tool_result {
                            serde_json::json!({ "choices": [{ "delta": { "content": "（已交给子代理完成。）" } }] })
                        } else if last_user.contains("推进一章") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_w1", "function": { "name": "sub_agent", "arguments": "{\"agent\":\"writer\",\"instruction\":\"把故事往前推进一章\"}" } },
                            ] } }] })
                        } else if last_user.contains("审一章") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_a1", "function": { "name": "sub_agent", "arguments": "{\"agent\":\"auditor\",\"instruction\":\"审计最新章\",\"chapterNumber\":1}" } },
                            ] } }] })
                        } else if last_user.contains("导出") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_e1", "function": { "name": "sub_agent", "arguments": "{\"agent\":\"exporter\",\"instruction\":\"导出 txt\"}" } },
                            ] } }] })
                        } else {
                            serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                        }
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), tool_names)
    }

    #[tokio::test]
    async fn book_session_sub_agent_writer_auditor_exporter() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, tool_names) = mock_sub87_llm().await;
        let session_id = "1783007000001-s87a";
        let runtime = rt87(&root, &llm);
        let app = app87(runtime);
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b1"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // ① writer 单章全链。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"推进一章","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（已交给子代理完成。）");
        let names = tool_names.lock().unwrap().clone();
        assert!(names.contains(&"sub_agent".to_string()), "书会话注册：{names:?}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        let card = &execs[0];
        assert_eq!(card["tool"], "sub_agent");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        assert!(card["result"].as_str().unwrap().starts_with("Chapter written for \"b1\". Word count: "), "body: {parsed}");
        assert_eq!(card["details"]["kind"], "chapter_written");
        assert_eq!(card["details"]["bookId"], "b1");
        assert_eq!(card["details"]["chapterNumber"], 1);
        assert_eq!(card["details"]["status"], "ready-for-review");
        // 章节落盘。
        assert!(root.join("books/b1/chapters/0001_风起.md").exists());

        // ② auditor：审计第 1 章（审稿 mock PASS）。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"审一章","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "sub_agent");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        let audit_text = card["result"].as_str().unwrap();
        assert!(audit_text.starts_with("Audit chapter 1: "), "{audit_text}");
        assert!(audit_text.contains("issue(s)."), "{audit_text}");

        // ③ exporter：txt 导出落盘。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"导出全文","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "sub_agent");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        let export_text = card["result"].as_str().unwrap();
        assert!(export_text.starts_with("Exported \"b1\": "), "{export_text}");
        let output_path = export_text.split(" → ").nth(1).unwrap().to_string();
        assert!(std::path::Path::new(&output_path).is_file(), "导出落盘：{output_path}");
        let _ = Value::Null;
    }

    #[tokio::test]
    async fn chat_session_without_book_does_not_register_sub_agent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, tool_names) = mock_sub87_llm().await;
        let session_id = "1783007000002-s87b";
        let app = app87(rt87(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"推进一章","sessionId":"{session_id}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let names = tool_names.lock().unwrap().clone();
        assert!(!names.contains(&"sub_agent".to_string()), "无书 chat 不注册：{names:?}");
        assert!(names.contains(&"propose_action".to_string()), "{names:?}");
    }
}

mod sub88_e2e {
    //! 88 号：sub_agent reviser 聊天面——复用 47 号 /revise 审核环。
    //! 修稿 applied 全链（pre 有 warning → 修稿 → post 通过 → 落盘）+
    //! 门控拒绝（post 变差 → not-applied 诊断文本 + 原文保留）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt88(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app88(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    /// mock：修稿编辑（TAG 输出）+ 审计温控分流（pre 默认温 1 warning；
    /// post temp 0 由 `post_worse` 决定通过或变差）+ studio-agent 聊天
    /// 按指令发 sub_agent reviser 工具调用。
    async fn spawn_mock88(post_worse: bool) -> String {
        let post_content = if post_worse {
            r#"{"passed": false, "overallScore": 60, "summary": "变差了。", "issues": [{"severity": "warning", "category": "节奏", "description": "中段推进略缓。", "suggestion": "压缩。"}, {"severity": "warning", "category": "文风", "description": "形容词堆积。", "suggestion": "删减。"}]}"#
        } else {
            r#"{"passed": true, "overallScore": 90, "summary": "修订后连贯。", "issues": []}"#
        }
        .to_string();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let post_content = post_content.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                    let temperature = body["temperature"].as_f64();
                    let payload = if system.contains("修稿编辑") {
                        // 修稿提示含"审稿意见"——须先于审计分支。
                        serde_json::json!({ "choices": [{ "delta": { "content": "=== FIXED_ISSUES ===\n压缩了中段\n\n=== REVISED_CONTENT ===\n林动睁开双眼，灵气顺经脉游走。他攥紧拳头——屈辱自今日起讨回。\n\n=== UPDATED_STATE ===\n| 字段 | 值 |\n|---|---|\n| 当前章节 | 1 |\n\n=== UPDATED_HOOKS ===\n| hook_id | 状态 |\n|---|---|\n| H01 | progressing |\n" } }] })
                    } else if system.contains("审") || system.contains("连续") {
                        let content = if temperature == Some(0.0) {
                            post_content
                        } else {
                            r#"{"passed": false, "overallScore": 70, "summary": "有一处节奏问题。", "issues": [{"severity": "warning", "category": "节奏", "description": "中段推进略缓。", "suggestion": "压缩。"}]}"#.to_string()
                        };
                        serde_json::json!({ "choices": [{ "delta": { "content": content } }] })
                    } else if system.contains("创作助手") {
                        let last_user = messages
                            .iter()
                            .rev()
                            .find(|m| m["role"] == "user")
                            .and_then(|m| m["content"].as_str())
                            .unwrap_or("");
                        let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                        if has_tool_result {
                            serde_json::json!({ "choices": [{ "delta": { "content": "（已交给修订器处理。）" } }] })
                        } else if last_user.contains("修订一章") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_r1", "function": { "name": "sub_agent", "arguments": "{\"agent\":\"reviser\",\"instruction\":\"把第一章的节奏压紧\",\"chapterNumber\":1,\"mode\":\"polish\"}" } },
                            ] } }] })
                        } else {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_r2", "function": { "name": "sub_agent", "arguments": "{\"agent\":\"reviser\",\"instruction\":\"润色最新一章\"}" } },
                            ] } }] })
                        }
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    fn chapter_file(root: &std::path::Path) -> std::path::PathBuf {
        root.join("books").join("b1").join("chapters").join("0001_风起.md")
    }

    #[tokio::test]
    async fn book_session_sub_agent_reviser_applied_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        std::fs::write(chapter_file(&root), "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。").unwrap();
        let llm = spawn_mock88(false).await;
        let session_id = "1783007000003-s88a";
        let app = app88(rt88(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b1"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"修订一章","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        let card = &execs[0];
        assert_eq!(card["tool"], "sub_agent");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        assert_eq!(card["result"].as_str().unwrap(), "Revision (polish) complete for \"b1\" chapter 1.");
        let details = &card["details"];
        assert_eq!(details["kind"], "chapter_revision");
        assert_eq!(details["bookId"], "b1");
        assert_eq!(details["chapterNumber"], 1);
        assert_eq!(details["mode"], "polish");
        assert_eq!(details["applied"], true);
        assert_eq!(details["status"], "ready-for-review");
        assert!(details.get("skippedReason").is_none());
        // 章节文件被改写（标题保留 + 修订正文）。
        let saved = std::fs::read_to_string(chapter_file(&root)).unwrap();
        assert!(saved.starts_with("# 第1章 风起"), "{saved}");
        assert!(saved.contains("攥紧拳头"), "{saved}");
    }

    #[tokio::test]
    async fn book_session_sub_agent_reviser_gate_refusal_keeps_original() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let original = "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。";
        std::fs::write(chapter_file(&root), original).unwrap();
        let llm = spawn_mock88(true).await;
        let session_id = "1783007000004-s88b";
        let app = app88(rt88(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b1"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"润色一下这一章","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "sub_agent");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        let text = card["result"].as_str().unwrap();
        assert!(
            text.starts_with("Revision not applied for \"b1\" chapter 1: Manual revision kept original chapter: before blocking=1,"),
            "{text}"
        );
        assert!(text.contains("Revision gate:"), "{text}");
        assert!(text.contains("- Standard: A revision is applied only when"), "{text}");
        assert!(text.contains("- Before: blocking=1, critical=0, aiTell=0"), "{text}");
        assert!(text.contains("- After: blocking=2, critical=0, aiTell=0"), "{text}");
        assert!(text.contains("- Remaining issues:"), "{text}");
        assert!(text.contains("  - [warning] 节奏: 中段推进略缓。 (压缩。)"), "{text}");
        let details = &card["details"];
        assert_eq!(details["kind"], "chapter_revision");
        assert_eq!(details["applied"], false);
        assert_eq!(details["status"], "unchanged");
        assert_eq!(details["mode"], "spot-fix");
        assert!(details["skippedReason"].as_str().unwrap().starts_with("Manual revision kept original chapter"));
        let diagnostics = &details["revisionDiagnostics"];
        assert_eq!(diagnostics["before"]["blockingCount"], 1);
        assert_eq!(diagnostics["after"]["blockingCount"], 2);
        assert!(!diagnostics["remainingIssues"].as_array().unwrap().is_empty());
        // 门控拒绝：原章保留。
        assert_eq!(std::fs::read_to_string(chapter_file(&root)).unwrap(), original);
    }
}

mod sub89_e2e {
    //! 89 号：书会话确定性编辑工具族 + 注册矩阵对齐。
    //! patch（三级替换精确级）+ delete_latest（.trash + 状态回滚）全链
    //! 经聊天面 + book/edit 会话注册真值表。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt89(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app89(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    /// mock：studio-agent 聊天按指令发编辑工具调用（确定性五件，无 LLM 域链）。
    async fn spawn_mock89() -> (String, Arc<Mutex<Vec<String>>>) {
        let tool_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let tools_in = tool_names.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let tools_in = tools_in.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                    if let Some(tools) = body["tools"].as_array() {
                        let names = tools
                            .iter()
                            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
                            .collect::<Vec<_>>();
                        *tools_in.lock().unwrap() = names;
                    }
                    let payload = if system.contains("创作助手") || system.contains("Play 助手") {
                        let last_user = messages
                            .iter()
                            .rev()
                            .find(|m| m["role"] == "user")
                            .and_then(|m| m["content"].as_str())
                            .unwrap_or("");
                        let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                        if has_tool_result {
                            serde_json::json!({ "choices": [{ "delta": { "content": "（编辑已完成。）" } }] })
                        } else if last_user.contains("修改一处文字") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_p1", "function": { "name": "patch_chapter_text", "arguments": "{\"chapterNumber\":1,\"targetText\":\"灵气顺着经脉游走\",\"replacementText\":\"灵气在丹田盘旋\"}" } },
                            ] } }] })
                        } else if last_user.contains("删掉最新一章") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_d1", "function": { "name": "delete_latest_chapter", "arguments": "{}" } },
                            ] } }] })
                        } else {
                            serde_json::json!({ "choices": [{ "delta": { "content": "好的。" } }] })
                        }
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), tool_names)
    }

    fn fixture_two_chapters(root: &std::path::Path) {
        fixture_project(root);
        let book = root.join("books").join("b1");
        std::fs::write(
            book.join("chapters").join("0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
        )
        .unwrap();
        std::fs::write(
            book.join("chapters").join("0002_夜行.md"),
            "# 第2章 夜行\n\n夜色深沉，林动一路疾行。",
        )
        .unwrap();
        let snapshot = book.join("story").join("snapshots").join("1");
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::write(snapshot.join("current_state.md"), "# 状态\n主角在青阳镇。").unwrap();
        std::fs::write(snapshot.join("pending_hooks.md"), "# 伏笔\n- H01 祖符").unwrap();
    }

    #[tokio::test]
    async fn book_session_patch_and_delete_latest_full_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_two_chapters(&root);
        let (llm, tool_names) = spawn_mock89().await;
        let session_id = "1783007000005-s89a";
        let app = app89(rt89(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b1"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // ① patch_chapter_text：精确替换 + 复核标记。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"修改一处文字","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "patch_chapter_text");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        assert_eq!(card["result"].as_str().unwrap(), "Patched chapter 1 and marked it for review.");
        let saved = std::fs::read_to_string(root.join("books/b1/chapters/0001_风起.md")).unwrap();
        assert_eq!(saved, "# 第1章 风起\n\n林动睁开双眼，灵气在丹田盘旋。");

        // ② delete_latest_chapter：.trash 保留 + 状态回滚到第 1 章快照。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"删掉最新一章","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "delete_latest_chapter");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        assert_eq!(
            card["result"].as_str().unwrap(),
            "Deleted latest chapter 2 from \"b1\", preserved it in trash, and rolled story state back to chapter 1."
        );
        let details = &card["details"];
        assert_eq!(details["kind"], "chapter_deleted");
        assert_eq!(details["deletedChapter"], 2);
        assert_eq!(details["rolledBackTo"], 1);
        assert_eq!(details["title"], "夜行");
        let book = root.join("books").join("b1");
        assert!(!book.join("chapters").join("0002_夜行.md").exists());
        assert!(book.join("chapters").join(".trash").join("0002_夜行.md").is_file());
        assert!(book.join("chapters").join("0001_风起.md").is_file());
        // 回滚后的活状态来自第 1 章快照。
        let state = std::fs::read_to_string(book.join("story").join("current_state.md")).unwrap();
        assert_eq!(state, "# 状态\n主角在青阳镇。");

        // book 会话注册真值表：六件 + sub_agent + research + import；无 propose。
        let names = tool_names.lock().unwrap().clone();
        for expected in [
            "write_truth_file",
            "rename_entity",
            "patch_chapter_text",
            "replace_chapter_text",
            "delete_latest_chapter",
            "generate_cover",
            "sub_agent",
            "research_web",
            "import_chapters",
        ] {
            assert!(names.contains(&expected.to_string()), "缺 {expected}：{names:?}");
        }
        assert!(!names.contains(&"propose_action".to_string()), "book 会话不应注册 propose_action：{names:?}");
    }

    #[tokio::test]
    async fn edit_session_registers_deterministic_subset_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_two_chapters(&root);
        let (llm, tool_names) = spawn_mock89().await;
        let session_id = "1783007000006-s89b";
        let app = app89(rt89(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b1","sessionKind":"edit"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"随便聊聊","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let names = tool_names.lock().unwrap().clone();
        for expected in [
            "write_truth_file",
            "rename_entity",
            "patch_chapter_text",
            "replace_chapter_text",
            "delete_latest_chapter",
        ] {
            assert!(names.contains(&expected.to_string()), "edit 缺 {expected}：{names:?}");
        }
        for banned in [
            "sub_agent",
            "generate_cover",
            "research_web",
            "import_chapters",
            "propose_action",
        ] {
            assert!(!names.contains(&banned.to_string()), "edit 不应注册 {banned}：{names:?}");
        }
    }
}

mod sub90_e2e {
    //! 90 号：narrative forecast 三件套——create→get→select 全链经聊天面
    //! （mock 投影代理）+ 过期检出（stale 持久化）+ book/edit 注册面。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt90(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app90(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    /// mock：投影代理（叙事推演助手系统提示 → 2 分支 JSON）+ studio-agent
    /// 聊天按指令发 forecast 工具调用（forecastId 从指令正文中提取）。
    async fn spawn_mock90() -> (String, Arc<Mutex<Vec<String>>>) {
        let tool_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let tools_in = tool_names.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let tools_in = tools_in.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                    if let Some(tools) = body["tools"].as_array() {
                        let names = tools
                            .iter()
                            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
                            .collect::<Vec<_>>();
                        *tools_in.lock().unwrap() = names;
                    }
                    let forecast_json = r#"{"branches":[
                        {"title":"结盟","premise":"接受合作提议","beats":[{"chapter":2,"summary":"结成同盟"}],
                         "characterDecisions":[{"character":"主角","decision":"接受"}],
                         "projectedChanges":{"characters":["地位提升"],"relationships":[],"world":[],"hooks":["H01 推进"]},
                         "risks":[{"kind":"causality","description":"动机偏快"}],"uncertainties":["信任边界"],
                         "intentAlignment":{"score":88,"rationale":"贴合意图"}},
                        {"title":"翻脸","premise":"拒绝合作提议","beats":[{"chapter":2,"summary":"当场翻脸"}],
                         "characterDecisions":[],
                         "projectedChanges":{"characters":[],"relationships":[],"world":[],"hooks":[]},
                         "risks":[],"uncertainties":[],
                         "intentAlignment":{"score":72,"rationale":"冲突更强"}}
                    ]}"#;
                    let payload = if system.contains("叙事推演助手") || system.contains("narrative forecast assistant") {
                        serde_json::json!({ "choices": [{ "delta": { "content": forecast_json } }] })
                    } else if system.contains("创作助手") || system.contains("Play 助手") {
                        let last_user = messages
                            .iter()
                            .rev()
                            .find(|m| m["role"] == "user")
                            .and_then(|m| m["content"].as_str())
                            .unwrap_or("");
                        let forecast_id = last_user
                            .split_whitespace()
                            .find(|word| word.starts_with("fc-"))
                            .unwrap_or("fc-unknown");
                        let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                        if has_tool_result {
                            serde_json::json!({ "choices": [{ "delta": { "content": "（推演完成。）" } }] })
                        } else if last_user.contains("推演一下") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_f1", "function": { "name": "create_narrative_forecast", "arguments": "{\"divergence\":\"合作还是对抗\",\"branchCount\":2,\"horizon\":5}" } },
                            ] } }] })
                        } else if last_user.contains("核验推演") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_f2", "function": { "name": "get_narrative_forecast", "arguments": format!("{{\"forecastId\":\"{forecast_id}\"}}") } },
                            ] } }] })
                        } else if last_user.contains("选择分支") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_f3", "function": { "name": "select_narrative_branch", "arguments": format!("{{\"forecastId\":\"{forecast_id}\",\"branchId\":\"branch-1\"}}") } },
                            ] } }] })
                        } else {
                            serde_json::json!({ "choices": [{ "delta": { "content": "好的。" } }] })
                        }
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), tool_names)
    }

    #[tokio::test]
    async fn book_session_forecast_create_get_select_full_chain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        std::fs::write(
            root.join("books/b1/chapters/0001_风起.md"),
            "# 第1章 风起\n\n林动睁开双眼，灵气顺着经脉游走。",
        )
        .unwrap();
        let (llm, tool_names) = spawn_mock90().await;
        let session_id = "1783007000007-s90a";
        let app = app90(rt90(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b1"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // ① create：投影 2 分支 + 工件落盘。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"推演一下后续走向","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "create_narrative_forecast");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        let result_text = card["result"].as_str().unwrap();
        assert!(result_text.starts_with("Narrative forecast fc-"), "{result_text}");
        assert!(result_text.contains("created with 2 isolated branches."), "{result_text}");
        assert!(result_text.contains("branch-1 \"结盟\" — intent fit 88/100, 1 risk(s), premise: 接受合作提议"), "{result_text}");
        let details = &card["details"];
        assert_eq!(details["kind"], "narrative_forecast_created");
        let forecast_id = details["forecastId"].as_str().unwrap().to_string();
        let forecast_dir = root.join("books/b1/story/runtime/narrative-forecasts").join(&forecast_id);
        assert!(forecast_dir.join("forecast.json").is_file());
        assert!(forecast_dir.join("comparison.md").is_file());
        let comparison = std::fs::read_to_string(forecast_dir.join("comparison.md")).unwrap();
        assert!(comparison.contains("| branch-1 | 结盟 | 88 | 1 | 接受合作提议 |"));

        // ② get：正史未变 → active。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"核验推演 {forecast_id}","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "get_narrative_forecast");
        let result_text = card["result"].as_str().unwrap();
        assert!(result_text.contains(&format!("Forecast {forecast_id} (book b1) — status: active.")), "{result_text}");
        assert_eq!(card["details"]["stale"], false);

        // ③ select：只写 selected-branch-plan.md。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"选择分支 {forecast_id}","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "select_narrative_branch");
        let result_text = card["result"].as_str().unwrap();
        assert!(result_text.starts_with(&format!("Selected branch-1 \"结盟\" from forecast {forecast_id}.")), "{result_text}");
        let plan = std::fs::read_to_string(forecast_dir.join("selected-branch-plan.md")).unwrap();
        assert!(plan.starts_with("# 已选分支计划：结盟"));
        assert!(plan.contains("- 分支：branch-1"));

        // ④ 正史变化 → stale 持久化 + 警告行。
        std::fs::write(root.join("books/b1/chapters/0002_夜行.md"), "# 第2章").unwrap();
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"核验推演 {forecast_id}","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        let result_text = card["result"].as_str().unwrap();
        assert!(result_text.contains("— status: stale."), "{result_text}");
        assert!(result_text.contains("WARNING: canonical chapters or state changed"), "{result_text}");
        assert_eq!(card["details"]["stale"], true);

        // book 会话注册：forecast 三件齐备。
        let names = tool_names.lock().unwrap().clone();
        for expected in [
            "create_narrative_forecast",
            "get_narrative_forecast",
            "select_narrative_branch",
        ] {
            assert!(names.contains(&expected.to_string()), "缺 {expected}：{names:?}");
        }
    }

    #[tokio::test]
    async fn edit_session_does_not_register_forecast_tools() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, tool_names) = spawn_mock90().await;
        let session_id = "1783007000008-s90b";
        let app = app90(rt90(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b1","sessionKind":"edit"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"随便聊聊","sessionId":"{session_id}","activeBookId":"b1"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let names = tool_names.lock().unwrap().clone();
        for banned in [
            "create_narrative_forecast",
            "get_narrative_forecast",
            "select_narrative_branch",
        ] {
            assert!(!names.contains(&banned.to_string()), "edit 不应注册 {banned}：{names:?}");
        }
    }
}

mod sub91_e2e {
    //! 91 号：import_chapters resumeFrom 增量续放——既有书（1 章 + 既有地基）
    //! 续放第 2 章：跳过 Step 1（地基/索引不重置）、逐章回放同号替换、
    //! 文本 Resumed 分支 + importMode 直通。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    const ANALYZER_OUTPUT: &str = "\
=== CHAPTER_TITLE ===
续章

=== CHAPTER_CONTENT ===
夜色渐深。

=== PRE_WRITE_CHECK ===

=== POST_SETTLEMENT ===

=== UPDATED_STATE ===
| Field | Value |
| --- | --- |
| Current Chapter | 2 |

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

    fn rt91(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app91(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    async fn mock_resume_llm() -> String {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                let messages = body["messages"].as_array().cloned().unwrap_or_default();
                let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                let payload = if system.contains("连续性分析") || system.contains("continuity analyst") {
                    serde_json::json!({ "choices": [{ "delta": { "content": ANALYZER_OUTPUT } }] })
                } else if system.contains("创作助手") {
                    let last_user = messages
                        .iter()
                        .rev()
                        .find(|m| m["role"] == "user")
                        .and_then(|m| m["content"].as_str())
                        .unwrap_or("");
                    let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                    if has_tool_result {
                        serde_json::json!({ "choices": [{ "delta": { "content": "（续放完成。）" } }] })
                    } else if last_user.contains("续放") {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_res_1", "function": { "name": "import_chapters", "arguments": "{\"bookId\":\"b91\",\"sourcePath\":\"novel91.txt\",\"resumeFrom\":2}" } },
                        ] } }] })
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                    }
                } else {
                    serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                };
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn chat_resume_import_appends_chapter_two_keeps_foundation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
        let book = root.join("books").join("b91");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b91","title":"续放书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        // 既有第 1 章 + 既有地基（内容标记——续放不得覆盖）。
        std::fs::write(book.join("chapters").join("0001_风起.md"), "# 第一章 风起\n\n林动睁开双眼。").unwrap();
        std::fs::write(book.join("story").join("story_bible.md"), "# 既有地基（续放不得覆盖）\n\n旧内容。").unwrap();
        // 源文件两章（续放从第 2 章起回放）。
        std::fs::write(
            root.join("novel91.txt"),
            "# 第一章 风起\n\n林动睁开双眼，灵气涌动。\n\n# 第二章 云涌\n\n坊市喧闹，夜色渐深。",
        )
        .unwrap();

        let llm = mock_resume_llm().await;
        let session_id = "1783007000009-s91a";
        let app = app91(rt91(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b91"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"续放导入 novel91.txt 的后续章节","sessionId":"{session_id}","activeBookId":"b91"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        let card = &execs[0];
        assert_eq!(card["tool"], "import_chapters");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        let result_text = card["result"].as_str().unwrap();
        assert!(result_text.contains("Imported 1 chapter(s) into book \"b91\"."), "{result_text}");
        assert!(
            result_text.contains("Resumed replay from chapter 2; earlier chapters and the existing foundation were kept."),
            "{result_text}"
        );
        let details = &card["details"];
        assert_eq!(details["kind"], "chapters_imported");
        assert_eq!(details["importedCount"], 1);
        assert_eq!(details["importMode"], "continuation");

        // 既有地基未被动过（Step 1 被跳过——architect mock 若被调用会写新地基）。
        let bible = std::fs::read_to_string(book.join("story").join("story_bible.md")).unwrap();
        assert_eq!(bible, "# 既有地基（续放不得覆盖）\n\n旧内容。");
        // 第 1 章保留、第 2 章落盘（同号回放写 0002）。
        assert!(book.join("chapters").join("0001_风起.md").is_file());
        let entries: Vec<String> = std::fs::read_dir(book.join("chapters"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("0002"))
            .collect();
        assert_eq!(entries.len(), 1, "第 2 章应落盘：{entries:?}");
        // 索引：两章（1 保留 + 2 新增）。
        let index: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(book.join("chapters").join("index.json")).unwrap_or("[]".to_string()),
        )
        .unwrap_or(serde_json::json!([]));
        assert_eq!(index.as_array().map(Vec::len), Some(2), "index: {index}");
    }
}

mod sub92_e2e {
    //! 92 号：importMode=series 评审环——architect 首轮被拒 → 带反馈重生成
    //! （B 稿）→ 复审通过 → 落盘 B 稿地基。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    const ARCHITECT_FIRST: &str = r#"=== SECTION: story_frame ===
## 分岔点
开篇之前。

=== SECTION: volume_map ===
### 第一卷（1-20章）新程
独立冲突开启。

=== SECTION: roles ===
---ROLE---
tier: major
name: 林动
---CONTENT---
## 核心标签
坚忍。

=== SECTION: book_rules ===
## 导入模式
- series

=== SECTION: pending_hooks ===
| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H01 | 0 | 新谜 | open | 0 | 第2卷 | 慢烧 | 无 | 第2卷 | true |  | 开篇之谜 |
"#;

    const ARCHITECT_REVISED: &str = r#"=== SECTION: story_frame ===
## 分岔点
重写后的分岔。

=== SECTION: volume_map ===
### 第一卷（1-20章）系列新程
系列冲突强化。

=== SECTION: roles ===
---ROLE---
tier: major
name: 林震
---CONTENT---
## 核心标签
重写后的主角。

=== SECTION: book_rules ===
## 导入模式
- series

=== SECTION: pending_hooks ===
| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 备注 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H01 | 0 | 新谜 | open | 0 | 第2卷 | 慢烧 | 无 | 第2卷 | true |  | 开篇之谜 |
"#;

    const REVIEW_REJECT: &str = "\
=== DIMENSION: 1 ===
分数：65
意见：系列主线偏弱。

=== DIMENSION: 2 ===
分数：66
意见：承接不足。

=== DIMENSION: 3 ===
分数：64
意见：设定重复。

=== DIMENSION: 4 ===
分数：67
意见：角色扁平。

=== DIMENSION: 5 ===
分数：68
意见：节奏偏平。

=== OVERALL ===
总分：66
通过：否
总评：需要重写系列主线。";

    const REVIEW_PASS_SERIES: &str = "\
=== DIMENSION: 1 ===
分数：90
意见：系列主线清晰。

=== DIMENSION: 2 ===
分数：88
意见：承接有力。

=== DIMENSION: 3 ===
分数：85
意见：设定自洽。

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

    const ANALYZER_OUTPUT_92: &str = "\
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

    fn rt92(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app92(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    /// mock：architect 按反馈轮分流（消息含 "## 总评" → B 稿）；reviewer
    /// 计数（首审拒绝 → 之后通过）；analyzer 固定输出；studio 按指令发
    /// series 导入工具调用。
    async fn mock_series_llm() -> (String, Arc<Mutex<usize>>) {
        let review_calls: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let counter = review_calls.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let counter = counter.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                    let has_feedback = messages.iter().any(|m| {
                        m["content"].as_str().map(|c| c.contains("## 总评")).unwrap_or(false)
                    });
                    let payload = if system.contains("资深小说编辑") {
                        let calls = {
                            let mut c = counter.lock().unwrap();
                            *c += 1;
                            *c
                        };
                        let content = if calls == 1 { REVIEW_REJECT } else { REVIEW_PASS_SERIES };
                        serde_json::json!({ "choices": [{ "delta": { "content": content } }] })
                    } else if system.contains("总架构师") || system.contains("网络小说架构师") {
                        let content = if has_feedback { ARCHITECT_REVISED } else { ARCHITECT_FIRST };
                        serde_json::json!({ "choices": [{ "delta": { "content": content } }] })
                    } else if system.contains("连续性分析") || system.contains("continuity analyst") {
                        serde_json::json!({ "choices": [{ "delta": { "content": ANALYZER_OUTPUT_92 } }] })
                    } else if system.contains("创作助手") {
                        let last_user = messages
                            .iter()
                            .rev()
                            .find(|m| m["role"] == "user")
                            .and_then(|m| m["content"].as_str())
                            .unwrap_or("");
                        let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                        if has_tool_result {
                            serde_json::json!({ "choices": [{ "delta": { "content": "（系列导入完成。）" } }] })
                        } else if last_user.contains("系列导入") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_ser_1", "function": { "name": "import_chapters", "arguments": "{\"bookId\":\"b92\",\"sourcePath\":\"novel92.txt\",\"importMode\":\"series\"}" } },
                            ] } }] })
                        } else {
                            serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                        }
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), review_calls)
    }

    #[tokio::test]
    async fn chat_series_import_reviews_and_regenerates_foundation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
        let book = root.join("books").join("b92");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b92","title":"系列书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(root.join("novel92.txt"), "# 第一章 风起\n\n林动睁开双眼，灵气涌动。").unwrap();

        let (llm, review_calls) = mock_series_llm().await;
        let session_id = "1783007000010-s92a";
        let app = app92(rt92(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b92"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"系列导入 novel92.txt","sessionId":"{session_id}","activeBookId":"b92"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "import_chapters");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        let result_text = card["result"].as_str().unwrap();
        assert!(result_text.contains("Imported 1 chapter(s) into book \"b92\"."), "{result_text}");
        assert_eq!(card["details"]["importMode"], "series");

        // 评审环：首审拒绝 → 带反馈重生成 → 复审通过（恰两次评审）。
        assert_eq!(*review_calls.lock().unwrap(), 2, "评审次数");
        // 落盘地基来自 B 稿（反馈重生成轮）：系列强化的卷名 + 主角林震。
        let volume_map = std::fs::read_to_string(book.join("story").join("outline").join("volume_map.md"))
            .unwrap_or_default();
        assert!(volume_map.contains("系列新程"), "volume_map: {volume_map}");
        let roles_major = book.join("story").join("roles").join("主要角色");
        let role_names: Vec<String> = std::fs::read_dir(&roles_major)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(role_names.iter().any(|n| n.contains("林震")), "roles: {role_names:?}");
        // 章节回放照常。
        assert!(book.join("chapters").join("0001_风起.md").is_file());
    }
}

mod sub93_e2e {
    //! 93 号：/agent model 校验（非文本模型 400 双语）+ attachments 归一化
    //! （text/image 落盘 + 错误面）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt93(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app93(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    async fn mock_plain_llm() -> String {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|axum::Json(_body): axum::Json<serde_json::Value>| async move {
                let payload = serde_json::json!({ "choices": [{ "delta": { "content": "好的。" } }] });
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn agent_model_guard_and_attachment_normalization() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let llm = mock_plain_llm().await;
        let session_id = "1783007000011-s93a";
        let app = app93(rt93(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // ① 非文本模型 → 400 {error, response} 双语（zh 项目）。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"写点什么","sessionId":"{session_id}","model":"gemini-image-2.0"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        let message = parsed["error"].as_str().unwrap();
        assert!(message.starts_with("模型 gemini-image-2.0 不适合文本聊天/写作。"), "{message}");
        assert_eq!(parsed["response"].as_str().unwrap(), message);
        // 文本模型放行（后续附件用例复用）。
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"写点什么","sessionId":"{session_id}","model":"gemini-2.5-flash"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // ② attachments 归一化：text + image 两件 → 落盘 + 200。
        //    base64("参考资料内容") 与 PNG 头。
        let attachments = serde_json::json!([
            {"id": "att-1", "filename": "参考 notes.md", "dataUrl": "data:text/markdown;base64,5Y+C6ICD6LWE5paZ5YaF5a65"},
            {"filename": "shot.png", "mediaType": "image/png", "dataUrl": "data:image/png;base64,iVBORw0KGgo="},
        ]);
        let body = serde_json::json!({
            "instruction": "看看附件",
            "sessionId": session_id,
            "attachments": attachments,
        })
        .to_string();
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/agent", Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let upload_dir = root.join(".inkos").join("uploads").join(session_id);
        let mut names: Vec<String> = std::fs::read_dir(&upload_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 2, "上传落盘：{names:?}");
        assert!(names[0].ends_with("-1-参考 notes.md"), "{names:?}");
        assert!(names[1].ends_with("-2-shot.png"), "{names:?}");

        // ③ 错误面：非数组 → 400 INVALID_ATTACHMENTS；缺 dataUrl → 400。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"看看附件","sessionId":"{session_id}","attachments":"not-an-array"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "INVALID_ATTACHMENTS");
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"看看附件","sessionId":"{session_id}","attachments":[{{"filename":"a.md"}}]}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "INVALID_ATTACHMENT");
        assert!(parsed["error"]["message"].as_str().unwrap().contains("a.md is missing dataUrl"));
    }
}

mod sub95_e2e {
    //! 95 号：attachments 多模态注入——文本清单块拼入用户消息 + 图片经
    //! vision content 数组注入最后一条 user 消息（mock 捕获请求侧验证）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt95(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app95(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    /// mock：捕获 studio-agent 请求的 messages（最后一条 user 消息）。
    async fn mock_capture_llm() -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let captured: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = captured.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let sink = sink.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    if let Some(last_user) = messages.iter().rev().find(|m| m["role"] == "user") {
                        sink.lock().unwrap().push(last_user.clone());
                    }
                    let payload = serde_json::json!({ "choices": [{ "delta": { "content": "已查看附件。" } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), captured)
    }

    #[tokio::test]
    async fn agent_attachments_inject_text_block_and_vision_images() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, captured) = mock_capture_llm().await;
        let session_id = "1783007000012-s95a";
        let app = app95(rt95(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 文本附件（base64 "参考资料内容"）+ 图片附件（PNG 头字节）。
        let attachments = serde_json::json!([
            {"id": "att-1", "filename": "notes.md", "dataUrl": "data:text/markdown;base64,5Y+C6ICD6LWE5paZ5YaF5a65"},
            {"filename": "shot.png", "mediaType": "image/png", "dataUrl": "data:image/png;base64,iVBORw0KGgo="},
        ]);
        let body = serde_json::json!({
            "instruction": "看看这两个附件",
            "sessionId": session_id,
            "attachments": attachments,
        })
        .to_string();
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/agent", Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");

        let rounds = captured.lock().unwrap().clone();
        assert!(!rounds.is_empty(), "应捕获 studio-agent 请求");
        let last_user = &rounds[0];
        // ① 文本通道：用户消息为 vision 数组，text 段含指令 + 附件清单块
        //    （文本附件内容 + 图片标记行）。
        let content = &last_user["content"];
        assert!(content.is_array(), "content: {content}");
        assert_eq!(content[0]["type"], "text");
        let text = content[0]["text"].as_str().unwrap();
        assert!(text.starts_with("看看这两个附件"), "{text}");
        assert!(text.contains("## 用户上传文件（宿主已接收，用户授权本轮使用）"), "{text}");
        assert!(text.contains("### notes.md"), "{text}");
        assert!(text.contains("内容：\n```\n参考资料内容\n```"), "{text}");
        assert!(text.contains("### shot.png"), "{text}");
        assert!(text.contains("- 图片：已作为多模态输入附加"), "{text}");
        // ② 图片通道：两个 image_url 段（data URL 形态）。
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(content.as_array().unwrap().len(), 2, "text + 1 图：{content}");
        // 上传落盘（93 号行为保留）。
        let upload_dir = root.join(".inkos").join("uploads").join(session_id);
        let count = std::fs::read_dir(&upload_dir).unwrap().count();
        assert_eq!(count, 2);
    }
}

mod sub96_e2e {
    //! 96 号：/services/:service/test 深链——live /models 不可达时回退
    //! 最小 chat 探测（成功）；google 深链失败 → 四步检查清单诊断。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::service_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt96(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app96(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/services/:service/test",
                axum::routing::post(service_routes::test_service),
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

    /// mock 上游：/models 恒 404（逼深链）；/chat/completions 按
    /// `chat_ok` 分流（SSE OK / 400）。
    async fn mock_upstream(chat_ok: bool) -> String {
        let app = axum::Router::new()
            .route(
                "/models",
                axum::routing::get(|| async {
                    (StatusCode::NOT_FOUND, "no models".to_string())
                }),
            )
            .route(
                "/chat/completions",
                axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| async move {
                    if !chat_ok {
                        return axum::response::IntoResponse::into_response((StatusCode::BAD_REQUEST, "invalid key"));
                    }
                    // 108 号：非流式请求 → 整体 JSON（TS chatCompletion 非流式）。
                    if !body["stream"].as_bool().unwrap_or(false) {
                        return axum::response::IntoResponse::into_response(axum::Json(
                            serde_json::json!({
                                "choices": [{ "message": { "role": "assistant", "content": "OK" } }],
                                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
                            }),
                        ));
                    }
                    let payload = serde_json::json!({ "choices": [{ "delta": { "content": "OK" } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn deep_chat_probe_succeeds_when_models_unreachable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let upstream = mock_upstream(true).await;
        let app = app96(rt96(&root, &upstream));
        let body = serde_json::json!({
            "apiKey": "sk-test",
            "baseUrl": upstream,
            "model": "deepseek-chat-x",
            "apiFormat": "chat",
            "stream": false,
        })
        .to_string();
        let (status, parsed) = call(app, "POST", "/api/v1/services/deepseek/test", Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true, "body: {parsed}");
        assert_eq!(parsed["selectedModel"], "deepseek-chat-x");
        assert_eq!(parsed["detected"]["modelsSource"], "api");
        assert_eq!(parsed["probe"]["ok"], true);
    }

    #[tokio::test]
    async fn google_deep_probe_failure_returns_four_step_checklist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let upstream = mock_upstream(false).await;
        let app = app96(rt96(&root, &upstream));
        let body = serde_json::json!({
            "apiKey": "bad-key",
            "baseUrl": upstream,
            "model": "gemini-2.5-flash",
            "apiFormat": "chat",
            "stream": false,
        })
        .to_string();
        let (status, parsed) = call(app, "POST", "/api/v1/services/google/test", Some(&body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["ok"], false);
        let error = parsed["error"].as_str().unwrap();
        assert!(error.starts_with("Google Gemini 测试连接失败。"), "{error}");
        assert!(error.contains("服务商：Google Gemini"), "{error}");
        assert!(error.contains("测试模型：gemini-2.5-flash"), "{error}");
        assert!(error.contains("1. API Key 是否来自 Google AI Studio 的 Gemini API key"), "{error}");
        assert!(error.contains("4. 如果 key 曾经泄露，请在 AI Studio 重新生成后再保存。"), "{error}");
        assert_eq!(parsed["probe"]["ok"], false);
    }
}

mod sub97_e2e {
    //! 97 号：/agent 模型四层解析——层 1 命中（前端 service+model → per-request
    //! router 覆盖，代理请求携带前端模型）+ 层 1 无 key 400 双语。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt97(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app97(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    /// 覆盖目标 mock：捕获请求体（model 字段）。
    async fn mock_capture_model() -> (String, Arc<Mutex<Vec<String>>>) {
        let models: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = models.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let sink = sink.clone();
                async move {
                    if let Some(model) = body["model"].as_str() {
                        sink.lock().unwrap().push(model.to_string());
                    }
                    let payload = serde_json::json!({ "choices": [{ "delta": { "content": "好的，用了你选的模型。" } }] });
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), models)
    }

    #[tokio::test]
    async fn agent_layer1_override_routes_through_frontend_model() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, models) = mock_capture_model().await;
        // custom 服务条目指向覆盖 mock + secrets 有 key。
        std::fs::write(
            root.join("inkos.json"),
            format!(
                r#"{{"llm":{{"services":[{{"service":"custom:pick","baseUrl":"{llm}","apiFormat":"chat"}}]}}}}"#
            ),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{"services":{"custom:pick":{"apiKey":"sk-pick"}}}"#,
        )
        .unwrap();
        let session_id = "1783007000013-s97a";
        let app = app97(rt97(&root, "http://127.0.0.1:9"));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 层 1：service+model 显式 → 请求经覆盖端点（而非项目端点 127.0.0.1:9）。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"你好","sessionId":"{session_id}","service":"custom:pick","model":"front-model-x"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "好的，用了你选的模型。");
        let captured = models.lock().unwrap().clone();
        assert!(!captured.is_empty(), "覆盖端点应收到请求");
        assert!(
            captured.iter().all(|m| m == "front-model-x"),
            "代理请求携带前端模型：{captured:?}"
        );
    }

    #[tokio::test]
    async fn agent_layer1_missing_key_bilingual_400() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, _) = mock_capture_model().await;
        let session_id = "1783007000014-s97b";
        let app = app97(rt97(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // deepseek（非本地 preset）无 key → 400 双语。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"你好","sessionId":"{session_id}","service":"deepseek","model":"deepseek-chat"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {parsed}");
        assert_eq!(parsed["error"], "请先为 deepseek 配置 API Key");
        assert_eq!(parsed["response"], "请先在模型配置中为 deepseek 填写 API Key，然后再试。");
    }
}

mod sub99_e2e {
    //! 99 号：doctor transport 回退——models 不可达时 chat 深链，preferred
    //! stream 首探测空响应 → 回退非 stream（llmConnected 判定链）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::ops_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt99(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app99(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/doctor", axum::routing::get(ops_routes::get_doctor))
            .with_state(runtime)
    }

    async fn call(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let request = axum::http::Request::builder().uri(uri).body(axum::body::Body::empty()).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) };
        (status, parsed)
    }

    /// mock：/models 恒 404；/chat/completions 按请求体 stream 字段分流——
    /// stream=true 返回空内容（首传输空），stream=false 返回 "OK"（回退成功）。
    /// `always_empty` 时两路皆空（判定未连接）。
    async fn mock_doctor_upstream(always_empty: bool) -> (String, Arc<Mutex<Vec<bool>>>) {
        let streams: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = streams.clone();
        let app = axum::Router::new()
            .route("/models", axum::routing::get(|| async { (StatusCode::NOT_FOUND, "no models") }))
            .route(
                "/chat/completions",
                axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let sink = sink.clone();
                    async move {
                        let stream = body["stream"].as_bool().unwrap_or(false);
                        sink.lock().unwrap().push(stream);
                        let content = if always_empty || stream { "" } else { "OK" };
                        // 108 号：非流式请求 → 整体 JSON；流式 → SSE。
                        if !stream {
                            return axum::response::IntoResponse::into_response(axum::Json(
                                serde_json::json!({
                                    "choices": [{ "message": { "role": "assistant", "content": content } }],
                                    "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
                                }),
                            ));
                        }
                        let payload = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                        axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), streams)
    }

    #[tokio::test]
    async fn doctor_falls_back_to_non_stream_when_stream_probe_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        // 106 号起计划为 TS buildProbePlans 逐字：stream 偏好仅在 apiFormat
        // 有偏好时生效（apiFormat=chat + stream=true → [chat×true, chat×false]）。
        std::fs::write(root.join("inkos.json"), r#"{"llm":{"stream":true,"apiFormat":"chat"}}"#).unwrap();
        let (llm, streams) = mock_doctor_upstream(false).await;
        let (status, parsed) = call(app99(rt99(&root, &llm)), "/api/v1/doctor").await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["llmConnected"], true, "body: {parsed}");
        let calls = streams.lock().unwrap().clone();
        assert_eq!(calls, vec![true, false], "首传输 stream 空 → 回退非 stream：{calls:?}");
    }

    #[tokio::test]
    async fn doctor_reports_disconnected_when_all_transports_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        std::fs::write(root.join("inkos.json"), r#"{"llm":{"stream":true,"apiFormat":"chat"}}"#).unwrap();
        let (llm, streams) = mock_doctor_upstream(true).await;
        let (status, parsed) = call(app99(rt99(&root, &llm)), "/api/v1/doctor").await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["llmConnected"], false, "body: {parsed}");
        let calls = streams.lock().unwrap().clone();
        assert_eq!(calls, vec![true, false], "两计划都试过后判未连接：{calls:?}");
    }
}

mod sub100_e2e {
    //! 100 号：PDF 材料抽取经聊天面——ingest_material(file) 真抽取
    //! （kind pdf + 页数 + 错误文案）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt100(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    fn app100(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
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

    async fn mock_material_llm() -> String {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                let messages = body["messages"].as_array().cloned().unwrap_or_default();
                let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
                let payload = if system.contains("创作助手") {
                    let last_user = messages
                        .iter()
                        .rev()
                        .find(|m| m["role"] == "user")
                        .and_then(|m| m["content"].as_str())
                        .unwrap_or("");
                    let has_tool_result = messages.iter().any(|m| m["role"] == "tool");
                    if has_tool_result {
                        serde_json::json!({ "choices": [{ "delta": { "content": "（材料已归档。）" } }] })
                    } else if last_user.contains("归档 PDF") {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_pdf_1", "function": { "name": "ingest_material", "arguments": "{\"sourceKind\":\"file\",\"filePath\":\"doc.pdf\",\"purpose\":\"reference\"}" } },
                        ] } }] })
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                    }
                } else {
                    serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                };
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn chat_ingests_pdf_material_with_extracted_text() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.pdf"),
            root.join("doc.pdf"),
        )
        .unwrap();
        let llm = mock_material_llm().await;
        let session_id = "1783007000015-s100a";
        let app = app100(rt100(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"归档 PDF 材料","sessionId":"{session_id}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let card = &parsed["details"]["toolExecutions"][0];
        assert_eq!(card["tool"], "ingest_material");
        assert_eq!(card["status"], "completed", "body: {parsed}");
        let result_text = card["result"].as_str().unwrap();
        assert!(result_text.starts_with("Material ingested: "), "{result_text}");
        // manifest：kind pdf + 页数 + 抽取文本。
        let manifest_path = format!(".inkos/materials/{}.json", card["details"]["asset"]["id"].as_str().unwrap_or_default());
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(manifest_path)).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["kind"], "pdf");
        assert_eq!(manifest["totalPages"], 1);
        assert!(manifest["excerpt"].as_str().unwrap().contains("Chapter one reference material."), "excerpt: {manifest}");
    }
}

mod sub101_e2e {
    //! 101 号：单章写作中途截断——确认式 write_next 任务在草稿 LLM 生成期间
    //! 收到 abort → 链内检查点②（草稿后）命中 → 502 双语逐字错误 + 磁盘零
    //! 落盘 + 任务快照 error + tool:end isError 广播（TS throwIfOperationAborted
    //! 检查点语义的端到端验证）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::server::task_store::studio_task_snapshot_path;
    use inkos_engine::state::manager::StateManager;
    use std::sync::Mutex as StdMutex;

    const SID: &str = "1783099000001-ab01";

    /// 草稿分支延迟 mock：作家分支先标记 writer_seen、睡 delay_ms 再回包
    /// （把"用户在草稿生成中点击停止"钉死在检查点②之前，测试无时序抖动）。
    async fn spawn_slow_writer_mock(
        delay_ms: u64,
    ) -> (String, Arc<StdMutex<bool>>, tokio::task::JoinHandle<()>) {
        let writer_seen = Arc::new(StdMutex::new(false));
        let seen_for_server = writer_seen.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(
                move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let writer_seen = seen_for_server.clone();
                    async move {
                        let system = body["messages"][0]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        let content = if system.contains("创作总编") {
                            PLANNER_RESPONSE.to_string()
                        } else if system.contains("作家") || system.contains("写手") {
                            *writer_seen.lock().unwrap() = true;
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                            WRITER_RESPONSE.to_string()
                        } else if system.contains("审稿") {
                            "PASS\n95".to_string()
                        } else {
                            "PASS".to_string()
                        };
                        axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            sse_body(&content),
                        ))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), writer_seen, handle)
    }

    fn rt(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request =
            builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, parsed)
    }

    #[tokio::test]
    async fn confirmed_write_next_abort_mid_draft_stops_at_safe_point() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, writer_seen, _guard) = spawn_slow_writer_mock(600).await;
        let runtime = rt(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();
        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .route(
                "/api/v1/sessions/:sessionId/abort",
                axum::routing::post(session_routes::abort_session),
            )
            .with_state(runtime);

        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(
                r#"{{"sessionId":"{SID}","bookId":"b1","sessionKind":"book"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 确认式写作任务异步起跑（HTTP 响应要等链跑完才回）。
        let task_app = app.clone();
        let task = tokio::spawn(async move {
            call(
                task_app,
                "POST",
                "/api/v1/agent",
                Some(&format!(
                    r#"{{"instruction":"写下一章","sessionId":"{SID}","requestedIntent":"write_next"}}"#
                )),
            )
            .await
        });

        // 等到草稿 LLM 调用已在途（planner/composer 即刻完成，作家分支标记后睡 600ms）。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !*writer_seen.lock().unwrap() {
            assert!(
                std::time::Instant::now() < deadline,
                "草稿调用应在 10s 内出现"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // 草稿生成中触发中止（默认 scope=all 命中确认任务控制器）。
        let (status, parsed) = call(
            app.clone(),
            "POST",
            &format!("/api/v1/sessions/{SID}/abort"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["aborted"], true, "body: {parsed}");

        let (status, parsed) = task.await.unwrap();
        // TS formatAgentActionFailure 非忙错误面：502 AGENT_ACTION_FAILED。
        assert_eq!(status, StatusCode::BAD_GATEWAY, "body: {parsed}");
        assert_eq!(parsed["error"]["code"], "AGENT_ACTION_FAILED");
        assert_eq!(parsed["error"]["message"], "操作已中止：用户请求停止该任务。");
        assert_eq!(parsed["response"], "操作已中止：用户请求停止该任务。");

        // 安全点语义：草稿虽已生成，但检查点②先于任何落盘——章节目录、
        // 章索引、真相面全部保持原样。
        let chapters: usize = std::fs::read_dir(root.join("books").join("b1").join("chapters"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".md"))
            .count();
        assert_eq!(chapters, 0, "中止后不应有任何章节落盘");
        assert!(
            !root.join("books").join("b1").join("story").join("chapter_summaries.md").exists(),
            "真相面不应有章节摘要投影"
        );

        // 任务快照：error + 双语错误文本。
        let snapshot: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(studio_task_snapshot_path(&root, SID)).unwrap(),
        )
        .unwrap();
        assert_eq!(snapshot["requestedIntent"], "write_next");
        assert_eq!(snapshot["execution"]["status"], "error");
        assert_eq!(
            snapshot["execution"]["error"], "操作已中止：用户请求停止该任务。"
        );

        // 广播序：agent:start → tool:start(background) →（abort 端点的
        // agent:aborted 插队）→ tool:end(isError:true)。
        assert_eq!(subscriber.recv().await.unwrap().event, "agent:start");
        let tool_start = subscriber.recv().await.unwrap();
        assert_eq!(tool_start.event, "tool:start");
        let mut saw_aborted = false;
        let tool_end = loop {
            let event = subscriber.recv().await.unwrap();
            match event.event.as_str() {
                "agent:aborted" => saw_aborted = true,
                "tool:end" => break event,
                other => panic!("意外事件 {other}"),
            }
        };
        assert!(saw_aborted, "abort 端点应广播 agent:aborted");
        assert!(
            tool_end.data.contains("\"isError\":true"),
            "tool:end data: {}",
            tool_end.data
        );
    }
}

mod sub102_e2e {
    //! 102 号：import 链中途截断——既有书续放（第 2、3 章），第 2 章已完整
    //! 落盘后、第 3 章分析在途时触发 abort → 链内检查点③（分析后落盘前）命中
    //! → 聊天面工具卡 error（英文锚文本）+ 第 3 章零落盘 + 第 1/2 章与索引
    //! 完整保留（章粒度安全点——TS importChapters 检查点语义）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;
    use std::sync::Mutex as StdMutex;

    const ANALYZER_OUTPUT: &str = "\
=== CHAPTER_TITLE ===
续章

=== CHAPTER_CONTENT ===
夜色渐深。

=== PRE_WRITE_CHECK ===

=== POST_SETTLEMENT ===

=== UPDATED_STATE ===
| Field | Value |
| --- | --- |
| Current Chapter | 2 |

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

    fn rt(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request =
            builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, parsed)
    }

    /// 分析器两段节奏 mock：第 1 次分析（第 2 章）即刻回包；第 2 次分析
    /// （第 3 章）**到达时**标记 seen 后睡 600ms——标记时刻第 2 章必然已
    /// 完整落盘（回放循环已推进到第 3 章头），把"第 3 章分析在途时 abort"
    /// 钉死在检查点③之前，无时序抖动。
    async fn mock_two_phase_analyzer()
        -> (String, Arc<StdMutex<bool>>, tokio::task::JoinHandle<()>) {
        let second_seen = Arc::new(StdMutex::new(false));
        let seen_for_server = second_seen.clone();
        let analyzer_calls = Arc::new(StdMutex::new(0u32));
        let calls_for_server = analyzer_calls.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let second_seen = seen_for_server.clone();
                let analyzer_calls = calls_for_server.clone();
                async move {
                    let messages = body["messages"].as_array().cloned().unwrap_or_default();
                    let system =
                        messages.first().and_then(|m| m["content"].as_str()).unwrap_or("");
                    let payload = if system.contains("连续性分析") || system.contains("continuity analyst") {
                        let call_index = {
                            let mut calls = analyzer_calls.lock().unwrap();
                            *calls += 1;
                            *calls
                        };
                        if call_index >= 2 {
                            // 第 3 章分析到达：此刻第 2 章已完整落盘（循环头已
                            // 推进）；在途回包（检查点③尚未到达）。
                            *second_seen.lock().unwrap() = true;
                            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                        }
                        serde_json::json!({ "choices": [{ "delta": { "content": ANALYZER_OUTPUT } }] })
                    } else if system.contains("创作助手") {
                        let last_user = messages
                            .iter()
                            .rev()
                            .find(|m| m["role"] == "user")
                            .and_then(|m| m["content"].as_str())
                            .unwrap_or("");
                        if last_user.contains("续放") {
                            serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                                { "index": 0, "id": "call_res102", "function": { "name": "import_chapters", "arguments": "{\"bookId\":\"b102\",\"sourcePath\":\"novel102.txt\",\"resumeFrom\":2}" } },
                            ] } }] })
                        } else {
                            serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                        }
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                    };
                    let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                    ))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), second_seen, handle)
    }

    #[tokio::test]
    async fn chat_import_abort_mid_replay_keeps_landed_chapters_and_drops_current() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
        let book = root.join("books").join("b102");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b102","title":"截断书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(book.join("chapters").join("0001_风起.md"), "# 第一章 风起\n\n林动睁开双眼。").unwrap();
        std::fs::write(book.join("story").join("story_bible.md"), "# 既有地基（截断不得覆盖）\n\n旧内容。").unwrap();
        // 源文件三章：续放回放第 2、3 章。
        std::fs::write(
            root.join("novel102.txt"),
            "# 第一章 风起\n\n林动睁开双眼，灵气涌动。\n\n# 第二章 云涌\n\n坊市喧闹，夜色渐深。\n\n# 第三章 潮生\n\n潮声在远方滚动。",
        )
        .unwrap();

        let (llm, second_seen, _guard) = mock_two_phase_analyzer().await;
        let session_id = "1783099000002-imp1";
        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
            .route(
                "/api/v1/sessions/:sessionId/abort",
                axum::routing::post(session_routes::abort_session),
            )
            .with_state(rt(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session_id}","bookId":"b102"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 聊天请求异步起跑（工具执行期间回不了响应）。
        let task_app = app.clone();
        let task = tokio::spawn(async move {
            call(
                task_app,
                "POST",
                "/api/v1/agent",
                Some(&format!(
                    r#"{{"instruction":"续放导入 novel102.txt 的后续章节","sessionId":"{session_id}","activeBookId":"b102"}}"#
                )),
            )
            .await
        });

        // 等到第 3 章分析在途（第 2 章此刻应已完整落盘）。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !*second_seen.lock().unwrap() {
            assert!(std::time::Instant::now() < deadline, "第 3 章分析应在 10s 内出现");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        // 第 2 章已落盘是章粒度安全点的前提（此刻断言而非事后，锁死语义）。
        assert!(
            std::fs::read_dir(book.join("chapters"))
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with("0002")),
            "第 2 章应已落盘"
        );

        let (status, parsed) = call(
            app.clone(),
            "POST",
            &format!("/api/v1/sessions/{session_id}/abort"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["aborted"], true, "body: {parsed}");

        let (status, parsed) = task.await.unwrap();
        // 聊天面现行中止契约（66 号）：200 + 空回复占位 + 工具卡。
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "（无回复内容）");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        let card = &execs[0];
        assert_eq!(card["tool"], "import_chapters");
        assert_eq!(card["status"], "error", "body: {parsed}");
        assert_eq!(
            card["error"], "Operation aborted: the user requested to stop this task."
        );

        // 章粒度安全点：第 1/2 章保留、第 3 章零落盘、索引两章、地基未动。
        assert!(book.join("chapters").join("0001_风起.md").is_file());
        assert!(
            std::fs::read_dir(book.join("chapters"))
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with("0002"))
        );
        assert!(
            !std::fs::read_dir(book.join("chapters"))
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with("0003")),
            "第 3 章不应落盘"
        );
        let index: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(book.join("chapters").join("index.json"))
                .unwrap_or("[]".to_string()),
        )
        .unwrap_or(serde_json::json!([]));
        assert_eq!(index.as_array().map(Vec::len), Some(2), "index: {index}");
        let bible = std::fs::read_to_string(book.join("story").join("story_bible.md")).unwrap();
        assert_eq!(bible, "# 既有地基（截断不得覆盖）\n\n旧内容。");
    }
}

mod sub105_e2e {
    //! 105 号：文件三件书会话化（TS createReadTool/createLsTool/createGrepTool
    //! 逐字）——books/ 作用域 + book/edit 会话独占注册 + grep story/chapters
    //! 前缀行号形态 + ls bytes 形态 + 空命中文案。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;
    use std::sync::Mutex as StdMutex;

    fn rt(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request =
            builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, parsed)
    }

    /// 工具名捕获 mock：按指令关键词发 grep（书会话）或纯文本（chat）。
    async fn mock_capture_tools() -> (String, Arc<StdMutex<Vec<String>>>) {
        let names = Arc::new(StdMutex::new(Vec::<String>::new()));
        let names_for_server = names.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let names = names_for_server.clone();
                async move {
                    if let Some(tools) = body["tools"].as_array() {
                        let mut batch: Vec<String> = tools
                            .iter()
                            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
                            .collect();
                        batch.sort();
                        // 按请求批次落一条（逗号连接）——注册矩阵按批次断言。
                        names.lock().unwrap().push(batch.join(","));
                    }
                    let chunk = if body["messages"].as_array().map(Vec::len).unwrap_or(0) <= 2 {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_grep_1", "function": { "name": "grep", "arguments": "{\"bookId\":\"b105\",\"pattern\":\"LANTERN\"}" } },
                        ] } }] })
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "搜到了。" } }] })
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
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), names)
    }

    #[tokio::test]
    async fn book_file_tools_scoped_grep_and_registration_matrix() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\n---\n正文指导\n",
        )
        .unwrap();
        let book = root.join("books").join("b105");
        std::fs::create_dir_all(book.join("story")).unwrap();
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b105","title":"文件书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        // story 命中小写 lantern（大小写不敏感）+ chapters 命中 + 项目根文件不参与。
        std::fs::write(book.join("story").join("current_state.md"), "灯是 lantern 亮着。").unwrap();
        std::fs::write(book.join("chapters").join("0001_风起.md"), "他提起 Lantern 走进夜色。").unwrap();
        std::fs::write(root.join("outside.md"), "lantern 不该被搜到").unwrap();

        let (llm, names) = mock_capture_tools().await;
        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route("/api/v1/sessions", axum::routing::post(session_routes::create_session))
            .with_state(rt(&root, &llm));

        // 书会话：grep（books/ 作用域 + story/chapters 前缀 + 行号 + 原行）。
        let session = "1783099000003-f105";
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(
                r#"{{"sessionId":"{session}","bookId":"b105","sessionKind":"book"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app.clone(),
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"搜一下灯笼线索","sessionId":"{session}","activeBookId":"b105"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0]["tool"], "grep");
        assert_eq!(execs[0]["status"], "completed", "body: {parsed}");
        let result = execs[0]["result"].as_str().unwrap();
        assert!(
            result.contains("story/current_state.md:1: 灯是 lantern 亮着。"),
            "result: {result}"
        );
        assert!(
            result.contains("chapters/0001_风起.md:1: 他提起 Lantern 走进夜色。"),
            "result: {result}"
        );
        assert!(!result.contains("outside.md"), "books/ 外不参与：{result}");
        // 书会话注册面：文件三件在批次内（含 sub_agent/material 等）。
        let registered = names.lock().unwrap().clone();
        let book_batch = registered
            .iter()
            .find(|batch| batch.contains("sub_agent"))
            .unwrap_or_else(|| panic!("书会话批次缺失：{registered:?}"));
        for tool in ["grep", "read", "ls", "sub_agent", "ingest_material"] {
            assert!(
                registered
                    .iter()
                    .any(|batch| batch.split(',').any(|name| name == tool)),
                "{tool} 应在书会话批次：{registered:?}"
            );
        }
        let _ = book_batch;

        // chat 会话：文件三件不注册（TS 矩阵）。
        let chat = "1783099000004-c105";
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{chat}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"纯聊天","sessionId":"{chat}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let registered = names.lock().unwrap().clone();
        // 最后一批 = chat 会话：无 read/ls/grep，有 propose_action + material。
        let chat_batch = registered.last().unwrap();
        assert!(
            chat_batch.contains("propose_action") && chat_batch.contains("ingest_material"),
            "chat 批次：{chat_batch}"
        );
        for tool in ["read", "ls", "grep", "sub_agent"] {
            assert!(
                !chat_batch.split(',').any(|name| name == tool),
                "chat 会话不应注册 {tool}：{chat_batch}"
            );
        }
    }
}

mod sub106_e2e {
    //! 106 号：responses 协议传输——探测计划维度（chat 失败回退 /responses）+
    //! 聊天面全链（inkos.json 服务项 apiFormat=responses → 97 层覆盖端点 →
    //! studio-agent 走 /responses：input/instructions/store/max_output_tokens
    //! 请求形态 + delta/completed 事件流解析）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::service_routes;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;
    use std::sync::Mutex as StdMutex;

    fn rt(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request =
            builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, parsed)
    }

    /// 三端点 mock：/models 404；/chat/completions 404；/responses 按请求
    /// stream 字段分流（非流式 JSON / 流式 responses SSE 事件），捕获请求体。
    async fn mock_responses_only() -> (String, Arc<StdMutex<Vec<serde_json::Value>>>) {
        let bodies = Arc::new(StdMutex::new(Vec::<serde_json::Value>::new()));
        let bodies_for_server = bodies.clone();
        let app = axum::Router::new()
            .route(
                "/models",
                axum::routing::get(|| async { StatusCode::NOT_FOUND }),
            )
            .route(
                "/chat/completions",
                axum::routing::post(|| async { StatusCode::NOT_FOUND }),
            )
            .route(
                "/responses",
                axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let bodies = bodies_for_server.clone();
                    async move {
                        bodies.lock().unwrap().push(body.clone());
                        if body["stream"].as_bool().unwrap_or(false) {
                            let events = [
                                serde_json::json!({ "type": "response.output_text.delta", "delta": "直接回答。" }),
                                serde_json::json!({ "type": "response.completed", "response": { "usage": { "input_tokens": 9, "output_tokens": 7, "total_tokens": 16 } } }),
                            ];
                            let stream: String = events
                                .iter()
                                .map(|event| format!("data: {event}\n\n"))
                                .collect();
                            axum::response::IntoResponse::into_response((
                                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                                stream,
                            ))
                        } else {
                            axum::response::IntoResponse::into_response(axum::Json(
                                serde_json::json!({
                                    "output": [{ "content": [{ "type": "output_text", "text": "OK" }] }],
                                    "usage": { "input_tokens": 1, "output_tokens": 1, "total_tokens": 2 },
                                }),
                            ))
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), bodies)
    }

    #[tokio::test]
    async fn test_service_falls_back_to_responses_probe_when_chat_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, bodies) = mock_responses_only().await;
        let app = axum::Router::new()
            .route(
                "/api/v1/services/:service/test",
                axum::routing::post(service_routes::test_service),
            )
            .with_state(rt(&root, &llm));

        // 无 apiFormat 偏好 → 计划 [chat 非流式, responses 非流式]：chat 404
        // → responses 探测成功（96 号"一律 chat"备案闭合）。
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/services/svc-r/test",
            Some(&format!(
                r#"{{"apiKey":"k","baseUrl":"{llm}","model":"resp-model"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["ok"], true, "body: {parsed}");
        assert_eq!(parsed["detected"]["apiFormat"], "responses", "body: {parsed}");
        assert_eq!(parsed["detected"]["stream"], false, "body: {parsed}");
        assert_eq!(parsed["selectedModel"], "resp-model");

        // /responses 探测请求形态（TS chatCompletion 经 responses 传输）。
        let bodies = bodies.lock().unwrap();
        let probe = bodies.last().unwrap();
        assert_eq!(probe["model"], "resp-model");
        assert_eq!(probe["store"], false);
        assert_eq!(probe["max_output_tokens"], 16);
        assert_eq!(probe["stream"], false);
        assert_eq!(
            probe["input"][0]["content"][0]["text"], "Reply with OK only."
        );
    }

    #[tokio::test]
    async fn agent_chat_uses_responses_transport_for_configured_service() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, bodies) = mock_responses_only().await;
        // 服务项 apiFormat=responses + secrets key → 97 层 1 显式命中覆盖端点。
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join("inkos.json"),
            format!(
                r#"{{"llm":{{"services":{{"custom:Resp":{{"service":"custom","name":"Resp","baseUrl":"{llm}","apiFormat":"responses"}}}}}}}}"#
            ),
        )
        .unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{"services":{"custom:Resp":{"apiKey":"sk-r"}}}"#,
        )
        .unwrap();

        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .with_state(rt(&root, &llm));
        let session = "1783099000005-r106";
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"打个招呼","sessionId":"{session}","service":"custom:Resp","model":"resp-model"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["response"], "直接回答。", "body: {parsed}");

        // 请求侧：/responses 形态（input + instructions + store + 流式）。
        let bodies = bodies.lock().unwrap();
        let request = bodies.last().expect("应命中 /responses");
        assert_eq!(request["model"], "resp-model");
        assert_eq!(request["stream"], true);
        assert_eq!(request["store"], false);
        assert!(
            request["instructions"].as_str().is_some_and(|s| !s.is_empty()),
            "system 提示应进 instructions：{request}"
        );
        let input = request["input"].as_array().unwrap();
        let last = input.last().unwrap();
        assert_eq!(last["role"], "user");
        assert!(
            last["content"][0]["text"]
                .as_str()
                .is_some_and(|text| text.contains("打个招呼")),
            "input: {input:?}"
        );
        assert!(
            input.iter().all(|item| item["role"] != "system"),
            "system 不进 input：{input:?}"
        );
    }
}

mod sub108_e2e {
    //! 108 号：stream 传输维度生产面——服务项 stream:false → 97 层覆盖端点
    //! 全非流式调用（TS client.stream）：chat 请求体 stream=false + 非流式
    //! JSON 响应路径（不支持 SSE 的端点兼容）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;
    use std::sync::Mutex as StdMutex;

    fn rt(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request =
            builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, parsed)
    }

    /// chat 双形态 mock：请求 stream=false → 非流式 JSON；true → SSE。捕获请求体。
    async fn mock_dual_mode_chat() -> (String, Arc<StdMutex<Vec<serde_json::Value>>>) {
        let bodies = Arc::new(StdMutex::new(Vec::<serde_json::Value>::new()));
        let bodies_for_server = bodies.clone();
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let bodies = bodies_for_server.clone();
                async move {
                    bodies.lock().unwrap().push(body.clone());
                    if body["stream"].as_bool().unwrap_or(false) {
                        let chunk =
                            serde_json::json!({ "choices": [{ "delta": { "content": "流式回复。" } }] });
                        let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
                        axum::response::IntoResponse::into_response((
                            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                            format!("data: {chunk}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                        ))
                    } else {
                        axum::response::IntoResponse::into_response(axum::Json(
                            serde_json::json!({
                                "choices": [{ "message": { "role": "assistant", "content": "非流式回复。" } }],
                                "usage": { "prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7 },
                            }),
                        ))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), bodies)
    }

    #[tokio::test]
    async fn service_stream_false_makes_agent_chat_non_streaming() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (llm, bodies) = mock_dual_mode_chat().await;
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join("inkos.json"),
            format!(
                r#"{{"llm":{{"services":{{"custom:Quiet":{{"service":"custom","name":"Quiet","baseUrl":"{llm}","stream":false}}}}}}}}"#
            ),
        )
        .unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{"services":{"custom:Quiet":{"apiKey":"sk-q"}}}"#,
        )
        .unwrap();

        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .with_state(rt(&root, &llm));
        let session = "1783099000006-s108";
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"打个招呼","sessionId":"{session}","service":"custom:Quiet","model":"quiet-model"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        // 非流式路径：响应来自 choices[0].message.content。
        assert_eq!(parsed["response"], "非流式回复。", "body: {parsed}");

        let bodies = bodies.lock().unwrap();
        let request = bodies.last().expect("应命中 /chat/completions");
        assert_eq!(request["stream"], false, "request: {request}");
        assert_eq!(request["model"], "quiet-model");
    }
}

mod sub109_e2e {
    //! 109 号：非 agent 面运行时配置装配——启动 router 指向死端点，inkos.json
    //! 服务项（custom:Cfg + secrets key）指向 mock；radar 扫描经 effective
    //! router 命中 mock。第二步改写 inkos.json 指回死端点 → mtime 失效 →
    //! 扫描失败（热解析与配置写入即时生效的完整证明）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::ops_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt(root: &std::path::Path, dead: &str) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root.to_path_buf())),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: dead.into(),
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

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request =
            builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, parsed)
    }

    /// 雷达 mock：市场分析师 system → 榜单 JSON。
    async fn mock_radar_llm() -> String {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|axum::Json(_body): axum::Json<serde_json::Value>| async move {
                let content = serde_json::json!({
                    "recommendations": [
                        { "platform": "番茄小说", "genre": "都市脑洞", "concept": "外卖员觉醒系统", "confidence": 0.82 }
                    ],
                    "marketSummary": "热配置端点生效。"
                })
                .to_string();
                let chunk = serde_json::json!({ "choices": [{ "delta": { "content": content } }] });
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
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

    #[tokio::test]
    async fn non_agent_face_resolves_router_from_inkos_json_hot() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let mock = mock_radar_llm().await;
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(
            root.join("inkos.json"),
            format!(
                r#"{{"llm":{{"services":{{"custom:Cfg":{{"service":"custom","name":"Cfg","baseUrl":"{mock}","defaultModelHint":true}}}},"defaultModel":"radar-model"}}}}"#
            ),
        )
        .unwrap();
        std::fs::write(
            root.join(".inkos").join("secrets.json"),
            r#"{"services":{"custom:Cfg":{"apiKey":"sk-c"}}}"#,
        )
        .unwrap();

        // 启动 router 指向死端点——radar 若走启动态必然失败。
        let runtime = rt(&root, "http://127.0.0.1:9");
        let app = axum::Router::new()
            .route("/api/v1/radar/scan", axum::routing::post(ops_routes::post_radar_scan))
            .with_state(runtime);
        let (status, parsed) = call(app.clone(), "POST", "/api/v1/radar/scan", None).await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        assert_eq!(parsed["marketSummary"], "热配置端点生效。", "body: {parsed}");

        // 配置改写指回死端点 → mtime 失效 → 热解析回落失败。
        std::fs::write(
            root.join("inkos.json"),
            r#"{"llm":{"services":{"custom:Cfg":{"service":"custom","name":"Cfg","baseUrl":"http://127.0.0.1:9"}},"defaultModel":"radar-model"}}"#,
        )
        .unwrap();
        let (status, parsed) = call(app, "POST", "/api/v1/radar/scan", None).await;
        assert_ne!(status, StatusCode::OK, "mtime 失效应使扫描失败：{parsed}");
    }
}

mod sub111_e2e {
    //! 111 号：Scheduler 精简面收口——① 确认式 write_next 章完通知（webhook
    //! 通道双发：notification-as-pipeline-complete + emitWebhook pipeline-complete，
    //! HMAC 签名头）；② daemon 写循环检测自动改写环（detect 不过 → anti-detect
    //! 重写 → 重测过 → detection_history.json 落盘）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::ops_routes;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;
    use std::sync::Mutex as StdMutex;

    fn rt(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request =
            builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, parsed)
    }

    /// webhook 捕获 mock（任意 POST → 200，记 headers+body）。
    async fn mock_webhook_sink() -> (String, Arc<StdMutex<Vec<(serde_json::Value, String, String)>>>) {
        let hits = Arc::new(StdMutex::new(Vec::new()));
        let hits_for_server = hits.clone();
        let app = axum::Router::new().route(
            "/hook",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, body: String| {
                    let hits = hits_for_server.clone();
                    async move {
                        let signature = headers
                            .get("X-InkOS-Signature")
                            .map(|value| value.to_str().unwrap_or_default().to_string())
                            .unwrap_or_default();
                        let raw = body.clone();
                        hits.lock().unwrap().push((
                            serde_json::from_str(&body).unwrap_or(serde_json::Value::Null),
                            signature,
                            raw,
                        ));
                        StatusCode::OK
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}/hook"), hits)
    }

    #[tokio::test]
    async fn confirmed_write_dispatches_notification_and_pipeline_complete_webhooks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (sink, hits) = mock_webhook_sink().await;
        let (llm, _calls, _guard) = spawn_mock_llm().await;
        std::fs::write(
            root.join("inkos.json"),
            format!(
                r#"{{"notify":[{{"type":"webhook","url":"{sink}","secret":"s3cr3t","events":[]}}]}}"#
            ),
        )
        .unwrap();

        let session = "1783099000007-n111";
        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .with_state(rt(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(
                r#"{{"sessionId":"{session}","bookId":"b1","sessionKind":"book"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"写下一章","sessionId":"{session}","requestedIntent":"write_next"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");

        // 双发：① 通知（title/body/format 进 data）② pipeline-complete
        // （bookId/chapterNumber/wordCount/passed/revised/status）。
        let hits = hits.lock().unwrap().clone();
        assert!(hits.len() >= 2, "webhook hits: {hits:?}");
        let notification = hits
            .iter()
            .find(|(body, _, _)| body["data"]["body"].is_string())
            .unwrap_or_else(|| panic!("通知事件缺失：{hits:?}"));
        assert!(
            notification.0["data"]["title"]
                .as_str()
                .is_some_and(|title| title.contains("测试书") && title.contains("第1章")),
            "title: {notification:?}"
        );
        assert!(
            notification.0["data"]["body"]
                .as_str()
                .is_some_and(|body| body.contains("风起")),
            "body: {notification:?}"
        );
        let complete = hits
            .iter()
            .find(|(body, _, _)| body["data"]["wordCount"].is_number())
            .unwrap_or_else(|| panic!("pipeline-complete 事件缺失：{hits:?}"));
        assert_eq!(complete.0["event"], "pipeline-complete");
        assert_eq!(complete.0["bookId"], "b1");
        assert_eq!(complete.0["chapterNumber"], 1);
        assert_eq!(complete.0["data"]["status"], "ready-for-review");
        // HMAC 签名头（sha256=hex 且与原始报文重算一致）。
        for (_body, signature, raw) in &hits {
            assert!(signature.starts_with("sha256="), "signature: {signature}");
            let expected = inkos_engine::notify::dispatcher::hmac_sha256_hex(b"s3cr3t", raw.as_bytes());
            assert_eq!(signature, &format!("sha256={expected}"));
        }
    }

    /// 全链 mock：写作链（创作总编/作家/审稿/PASS）+ 修稿编辑 → 改写正文 +
    /// /detect 检测（首测 0.9 → 重测 0.3）。
    async fn mock_detect_loop_llm() -> (String, Arc<StdMutex<u32>>) {
        let detect_calls = Arc::new(StdMutex::new(0u32));
        let detect_for_server = detect_calls.clone();
        let app = axum::Router::new()
            .route(
                "/chat/completions",
                axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                    let system = body["messages"][0]["content"].as_str().unwrap_or("").to_string();
                    let content = if system.contains("创作总编") {
                        PLANNER_RESPONSE.to_string()
                    } else if system.contains("修稿编辑") {
                        "改写后的章节内容：口语更强，长短句交错， AI 痕迹更少。".to_string()
                    } else if system.contains("作家") || system.contains("写手") {
                        WRITER_RESPONSE.to_string()
                    } else if system.contains("审稿") {
                        "PASS\n95".to_string()
                    } else {
                        "PASS".to_string()
                    };
                    axum::response::IntoResponse::into_response((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        sse_body(&content),
                    ))
                }),
            )
            .route(
                "/detect",
                axum::routing::post(move |axum::Json(_body): axum::Json<serde_json::Value>| {
                    let detect_calls = detect_for_server.clone();
                    async move {
                        let calls = {
                            let mut counter = detect_calls.lock().unwrap();
                            *counter += 1;
                            *counter
                        };
                        // 前两次超标（调度首测 + 环首测 → 触发改写）；重测过阈。
                        let score = if calls <= 2 { 0.9 } else { 0.3 };
                        axum::Json(serde_json::json!({ "score": score }))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{addr}"), detect_calls)
    }

    #[tokio::test]
    async fn daemon_detection_loop_rewrites_and_records_history() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        let (llm, detect_calls) = mock_detect_loop_llm().await;
        std::env::set_var("INKOS_TEST_DETECT_KEY_111", "dk");
        std::fs::write(
            root.join("inkos.json"),
            format!(
                r#"{{"detection":{{"provider":"custom","apiUrl":"{llm}/detect","apiKeyEnv":"INKOS_TEST_DETECT_KEY_111","threshold":0.5,"enabled":true,"autoRewrite":true,"maxRetries":2}}}}"#
            ),
        )
        .unwrap();

        let app = axum::Router::new()
            .route("/api/v1/daemon/start", axum::routing::post(ops_routes::post_daemon_start))
            .route("/api/v1/daemon/stop", axum::routing::post(ops_routes::post_daemon_stop))
            .with_state(rt(&root, &llm));
        // daemon 为进程单例——并行测试（如 sub104 daemon 用例）可能正持有，
        // 400 时等待重试（另一测试 stop 后放行）。
        let start_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let (status, parsed) = call(app.clone(), "POST", "/api/v1/daemon/start", None).await;
            if status == StatusCode::OK {
                break;
            }
            assert!(
                std::time::Instant::now() < start_deadline,
                "daemon start 一直被占用：{parsed}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        // 检测环完成：三次 detect（调度首测 0.9 → 环首测 0.9 → 改写 → 重测 0.3）。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while *detect_calls.lock().unwrap() < 3 {
            assert!(
                std::time::Instant::now() < deadline,
                "检测环未完成：{}",
                detect_calls.lock().unwrap()
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        // history：一条 rewrite 记录（attempt 1，score 0.3，过阈）。
        let history_path = root
            .join("books")
            .join("b1")
            .join("story")
            .join("detection_history.json");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if history_path.is_file() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "history 未落盘");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let history: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&history_path).unwrap(),
        )
        .unwrap();
        let entries = history.as_array().unwrap();
        assert_eq!(entries.len(), 1, "history: {history}");
        assert_eq!(entries[0]["action"], "rewrite");
        assert_eq!(entries[0]["attempt"], 1);
        assert_eq!(entries[0]["score"], 0.3);
        assert_eq!(entries[0]["provider"], "custom");

        let (status, _) = call(app, "POST", "/api/v1/daemon/stop", None).await;
        assert_eq!(status, StatusCode::OK);
    }
}

mod sub112_e2e {
    //! 112 号：P3 尾量清账——① writing.reviewMode=manual 配置位经 from_project
    //! 流入确认式 write_next（manual 写完即停 → audit-failed + 需复核文案）；
    //! ② import 回放治理输入（TS prepareWriteInput 同构——逐章 plan 持久化）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request =
            builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, parsed)
    }

    #[tokio::test]
    async fn writing_review_mode_manual_config_reaches_confirmed_write() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fixture_project(&root);
        std::fs::write(
            root.join("inkos.json"),
            r#"{"writing":{"reviewMode":"manual"}}"#,
        )
        .unwrap();
        let (llm, _calls, _guard) = spawn_mock_llm().await;
        let session = "1783099000008-m112";
        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .with_state(rt(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(
                r#"{{"sessionId":"{session}","bookId":"b1","sessionKind":"book"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"写下一章","sessionId":"{session}","requestedIntent":"write_next"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        // manual 写完即停：passed=false → audit-failed → 需复核文案。
        let response = parsed["response"].as_str().unwrap_or_default();
        assert!(response.contains("审稿未通过"), "response: {response}");
        assert!(response.contains("audit-failed"), "response: {response}");
        let exec = &parsed["details"]["toolExecutions"][0];
        assert_eq!(exec["details"]["status"], "audit-failed", "body: {parsed}");
    }

    /// 导入回放 mock：创作总编（plan）+ 连续性分析（analyzer）+ 创作助手（工具调用）。
    async fn mock_replay_governed() -> String {
        const ANALYZER_MIN: &str = "=== CHAPTER_TITLE ===\n续章\n\n=== CHAPTER_CONTENT ===\n夜色渐深。\n\n=== PRE_WRITE_CHECK ===\n\n=== POST_SETTLEMENT ===\n\n=== UPDATED_STATE ===\n| Field | Value |\n| --- | --- |\n| Current Chapter | 2 |\n\n=== UPDATED_LEDGER ===\n\n=== UPDATED_HOOKS ===\n| hook_id | start_chapter | type | status | last_advanced_chapter | expected_payoff | payoff_timing | notes |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n\n=== CHAPTER_SUMMARY ===\n| Chapter | Title | Characters | Key Events | State Changes | Hook Activity | Mood | Chapter Type |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n\n=== UPDATED_SUBPLOTS ===\n\n=== UPDATED_EMOTIONAL_ARCS ===\n\n=== UPDATED_CHARACTER_MATRIX ===\n## 林动\n- **Role**: protagonist\n";
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| async move {
                let messages = body["messages"].as_array().cloned().unwrap_or_default();
                let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("");
                let payload = if system.contains("连续性分析") {
                    serde_json::json!({ "choices": [{ "delta": { "content": ANALYZER_MIN } }] })
                } else if system.contains("创作助手") {
                    let last_user = messages
                        .iter()
                        .rev()
                        .find(|m| m["role"] == "user")
                        .and_then(|m| m["content"].as_str())
                        .unwrap_or("");
                    if last_user.contains("续放") {
                        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                            { "index": 0, "id": "call_g112", "function": { "name": "import_chapters", "arguments": "{\"bookId\":\"b112\",\"sourcePath\":\"novel112.txt\",\"resumeFrom\":2}" } },
                        ] } }] })
                    } else {
                        serde_json::json!({ "choices": [{ "delta": { "content": "（续放完成。）" } }] })
                    }
                } else if system.contains("创作总编") {
                    serde_json::json!({ "choices": [{ "delta": { "content": PLANNER_RESPONSE } }] })
                } else {
                    serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                };
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn import_replay_persists_governed_plan_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
        let book = root.join("books").join("b112");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story").join("runtime")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b112","title":"回放书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(book.join("chapters").join("0001_风起.md"), "# 第一章 风起\n\n林动睁开双眼。").unwrap();
        std::fs::write(book.join("story").join("story_bible.md"), "# 既有地基\n\n旧内容。").unwrap();
        std::fs::write(
            root.join("novel112.txt"),
            "# 第一章 风起\n\n林动睁开双眼。\n\n# 第二章 云涌\n\n坊市喧闹。",
        )
        .unwrap();

        let llm = mock_replay_governed().await;
        let session = "1783099000009-g112";
        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .with_state(rt(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session}","bookId":"b112"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"续放导入 novel112.txt 的后续章节","sessionId":"{session}","activeBookId":"b112"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        // 治理输入工件：回放章（第 2 章）的 plan 持久化（TS prepareWriteInput 同构）。
        assert!(
            book.join("story")
                .join("runtime")
                .join("chapter-0002.plan.md")
                .is_file(),
            "回放章 plan 工件应落盘：{}",
            book.join("story").join("runtime").display()
        );
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs[0]["tool"], "import_chapters");
        assert_eq!(execs[0]["status"], "completed", "body: {parsed}");
    }
}

mod sub114_e2e {
    //! 114 号：① 导入回放回写 state/*.json 四件套（TS
    //! syncLegacyStructuredStateFromMarkdown——磁盘格式对齐）；② 聊天面
    //! tool:end 结构化（result.content 数组 + 顶层 details——86 号备案闭合）。
    use super::*;
    use axum::http::StatusCode;
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::agent_route;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::session_routes;
    use inkos_engine::state::manager::StateManager;

    fn rt(root: &std::path::Path, llm: &str) -> BooksRuntime {
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

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request =
            builder.body(axum::body::Body::from(body.unwrap_or("").to_string())).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let parsed = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, parsed)
    }

    /// 导入回放 mock（复用 sub112 形态：创作总编/连续性分析/创作助手）。
    async fn mock_replay() -> String {
        const ANALYZER_MIN: &str = "=== CHAPTER_TITLE ===\n续章\n\n=== CHAPTER_CONTENT ===\n夜色渐深。\n\n=== PRE_WRITE_CHECK ===\n\n=== POST_SETTLEMENT ===\n\n=== UPDATED_STATE ===\n| Field | Value |\n| --- | --- |\n| 当前章节 | 2 |\n\n=== UPDATED_LEDGER ===\n\n=== UPDATED_HOOKS ===\n| hook_id | start_chapter | type | status | last_advanced_chapter | expected_payoff | payoff_timing | notes |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n\n=== CHAPTER_SUMMARY ===\n| Chapter | Title | Characters | Key Events | State Changes | Hook Activity | Mood | Chapter Type |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n| 2 | 续章 | 林动 | 夜探 | 无 | 无 | 紧张 | 推进章 |\n\n=== UPDATED_SUBPLOTS ===\n\n=== UPDATED_EMOTIONAL_ARCS ===\n\n=== UPDATED_CHARACTER_MATRIX ===\n## 林动\n- **Role**: protagonist\n";
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| async move {
                let messages = body["messages"].as_array().cloned().unwrap_or_default();
                let system = messages.first().and_then(|m| m["content"].as_str()).unwrap_or("");
                let payload = if system.contains("连续性分析") {
                    serde_json::json!({ "choices": [{ "delta": { "content": ANALYZER_MIN } }] })
                } else if system.contains("创作助手") {
                    serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                        { "index": 0, "id": "call_c114", "function": { "name": "import_chapters", "arguments": "{\"bookId\":\"b114\",\"sourcePath\":\"novel114.txt\",\"resumeFrom\":2}" } },
                    ] } }] })
                } else if system.contains("创作总编") {
                    serde_json::json!({ "choices": [{ "delta": { "content": PLANNER_RESPONSE } }] })
                } else {
                    serde_json::json!({ "choices": [{ "delta": { "content": "PASS" } }] })
                };
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn import_replay_rewrites_structured_state_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("assets").join("genres")).unwrap();
        std::fs::write(
            root.join("assets").join("genres").join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .unwrap();
        let book = root.join("books").join("b114");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::create_dir_all(book.join("story")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b114","title":"结构态书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(book.join("chapters").join("0001_风起.md"), "# 第一章 风起\n\n林动。").unwrap();
        std::fs::write(book.join("story").join("story_bible.md"), "# 既有地基\n\n旧。").unwrap();
        std::fs::write(
            root.join("novel114.txt"),
            "# 第一章 风起\n\n林动。\n\n# 第二章 续章\n\n夜色渐深。",
        )
        .unwrap();

        let llm = mock_replay().await;
        let session = "1783099000010-s114";
        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .with_state(rt(&root, &llm));
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session}","bookId":"b114"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"续放导入 novel114.txt","sessionId":"{session}","activeBookId":"b114"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");
        let execs = parsed["details"]["toolExecutions"].as_array().unwrap();
        assert_eq!(execs[0]["status"], "completed", "body: {parsed}");

        // 四件套（TS 导入路径磁盘格式）。
        let state = book.join("story").join("state");
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(state.join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["schemaVersion"], 2);
        assert_eq!(manifest["language"], "zh");
        assert_eq!(manifest["lastAppliedChapter"], 2, "回放后进度 2：{manifest}");
        let current: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(state.join("current_state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(current["chapter"], 2, "{current}");
        let summaries: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(state.join("chapter_summaries.json")).unwrap(),
        )
        .unwrap();
        assert!(
            summaries["rows"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row["chapter"] == 2)),
            "{summaries}"
        );
        let hooks: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(state.join("hooks.json")).unwrap(),
        )
        .unwrap();
        assert!(hooks["hooks"].is_array(), "{hooks}");
    }

    /// material 工具聊天 mock：首轮发 ingest_material 工具调用。
    async fn mock_material_chat() -> String {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                let count = body["messages"].as_array().map(Vec::len).unwrap_or(0);
                let payload = if count <= 2 {
                    serde_json::json!({ "choices": [{ "delta": { "tool_calls": [
                        { "index": 0, "id": "call_m114", "function": { "name": "ingest_material", "arguments": "{\"sourceKind\":\"file\",\"filePath\":\"设定.md\",\"filename\":\"设定.md\"}" } },
                    ] } }] })
                } else {
                    serde_json::json!({ "choices": [{ "delta": { "content": "已归档。" } }] })
                };
                let usage = serde_json::json!({ "choices": [], "usage": { "prompt_tokens": 9, "completion_tokens": 7, "total_tokens": 16 } });
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    format!("data: {payload}\n\ndata: {usage}\n\ndata: [DONE]\n\n"),
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn chat_tool_end_sse_carries_content_array_and_details() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("设定.md"), "主角设定草稿。").unwrap();
        let llm = mock_material_chat().await;
        let runtime = rt(&root, &llm);
        let mut subscriber = runtime.hub.subscribe();
        let app = axum::Router::new()
            .route("/api/v1/agent", axum::routing::post(agent_route::post_agent))
            .route(
                "/api/v1/sessions",
                axum::routing::post(session_routes::create_session),
            )
            .with_state(runtime);
        let session = "1783099000011-e114";
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/v1/sessions",
            Some(&format!(r#"{{"sessionId":"{session}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, parsed) = call(
            app,
            "POST",
            "/api/v1/agent",
            Some(&format!(
                r#"{{"instruction":"归档设定材料","sessionId":"{session}"}}"#
            )),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {parsed}");

        // SSE tool:end：result.content 为 [{type:"text",text}] + 顶层 details。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let event = tokio::time::timeout(std::time::Duration::from_secs(10), subscriber.recv())
                .await
                .expect("等待事件超时")
                .unwrap();
            if event.event == "tool:end" {
                let payload: serde_json::Value = serde_json::from_str(&event.data).unwrap();
                assert_eq!(payload["tool"], "ingest_material", "{payload}");
                assert!(
                    payload["result"]["content"][0]["text"]
                        .as_str()
                        .is_some_and(|text| text.starts_with("Material ingested: ")),
                    "{payload}"
                );
                assert_eq!(payload["result"]["content"][0]["type"], "text");
                assert_eq!(payload["details"]["kind"], "material_ingested", "{payload}");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "未收到 tool:end：{}",
                event.event
            );
        }
    }
}
