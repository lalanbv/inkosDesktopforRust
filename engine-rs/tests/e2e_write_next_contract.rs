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
        // 46 号起为审核环结果（ReviseChainResult 形状）。
        assert_eq!(parsed["applied"], true, "body: {parsed}");
        assert_eq!(parsed["status"], "revised");
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
        // 审核环：pre 有 warning → 修稿 → applied。
        assert_eq!(parsed["applied"], true, "body: {parsed}");
        assert_eq!(parsed["status"], "revised");
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
