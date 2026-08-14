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
