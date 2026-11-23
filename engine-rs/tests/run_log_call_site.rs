//! 1666 号：R26 run-log **写入点口径**活体锁——`AgentRouter::chat` 对 flaky mock。
//!
//! 背景：1663 号复座 TS 腿写入点时备案「Rust 逐次 `chat_once` 记」，据此把 TS
//! 定为每个瞬态重试轮各记一条、命中轮记 `tookOver`。实读 `agent_router.rs`：
//! 记录点在 `chat` 的 (chain_idx, round) 层，`chat_transient` 内环（同模型
//! 429/5xx 退避重试，最多 2 次）**不记**——同模型瞬态重试折叠进一条，耗时
//! 覆盖全部尝试；`took_over` 只在链层接管（chain_idx > 0）或 R25 轮 > 1 时成立。
//!
//! 此前 Rust 侧只有纯函数 golden（`golden_run_log_diff.rs`），写入点零测试——
//! 口径只存在于源码行间。本测把它钉成运行时事实，TS 腿 `run-log-writer.test`
//! 的折叠断言以此为参照（双端同一工作负载 → 同一组条目）。
//!
//! 运行：`cargo test --test run_log_call_site`（进程级环形缓冲按 agent/model
//! 标签过滤，用例间互不干扰；瞬态退避为真实 sleep，全程约 10s）。

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
use inkos_engine::llm::provider::{LLMMessage, LLMRole};
use inkos_engine::models::task_routing::{TaskModelOverride, TaskModelRouting};
use inkos_engine::utils::run_log::{global_run_log, RunLogEntry};

/// 测试键惯用形：env 注入优先，回退非凭据形态特征占位（855/1015 号）。
fn test_llm_key() -> String {
    std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key-placeholder".to_string())
}

fn endpoint(base_url: String, model: &str) -> LlmEndpointConfig {
    LlmEndpointConfig {
        context_window_tokens: 128_000,
        base_url,
        api_key: test_llm_key(),
        model: model.to_string(),
        max_tokens: 8192,
        extra_headers: HashMap::new(),
    }
}

fn user_message() -> Vec<LLMMessage> {
    vec![LLMMessage {
        role: LLMRole::User,
        content: "Reply with OK only.".to_string(),
        tool_calls: None,
        tool_call_id: None,
    }]
}

/// mock 上游对一次请求的应答裁决。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reply {
    /// 429 = 瞬态（进 `chat_transient` 退避重试）。
    Transient,
    /// 401 = 非瞬态（立即失败不重试不切换）。
    Fatal,
    Ok,
}

/// 应答裁决器：`(命中序号, 请求模型) → 应答`（经 Arc 入 axum handler 闭包）。
type Decide = Arc<dyn Fn(usize, &str) -> Reply + Send + Sync>;

fn ok_response(stream: bool) -> axum::response::Response {
    if stream {
        let chunk = json!({ "choices": [{ "delta": { "content": "OK" }, "finish_reason": null }] });
        let finish = json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] });
        let usage = json!({ "choices": [], "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 } });
        return axum::response::Response::builder()
            .status(200)
            .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
            .body(axum::body::Body::from(format!(
                "data: {chunk}\n\ndata: {finish}\n\ndata: {usage}\n\ndata: [DONE]\n\n"
            )))
            .unwrap();
    }
    axum::response::Response::builder()
        .status(200)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({
                "choices": [{ "message": { "role": "assistant", "content": "OK" }, "finish_reason": "stop" }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
            })
            .to_string(),
        ))
        .unwrap()
}

fn error_response(reply: Reply) -> axum::response::Response {
    let (status, message) = match reply {
        Reply::Transient => (429, "rate limited (429)"),
        // 不含任何瞬态短语/状态码（429|502|503|504）——走 Fatal 分支。
        Reply::Fatal => (401, "invalid api key"),
        Reply::Ok => unreachable!("Ok 由 ok_response 应答"),
    };
    axum::response::Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({ "error": { "message": message } }).to_string(),
        ))
        .unwrap()
}

/// mock 上游：`decide(命中序号, 请求模型)` 裁决每次应答。返回 (baseUrl, 命中计数)。
async fn spawn_mock_llm(
    decide: impl Fn(usize, &str) -> Reply + Send + Sync + 'static,
) -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_in = hits.clone();
    // handler 闭包须 Clone——裁决器经 Arc<dyn Fn> 入捕获（裸 impl Fn 不满足）。
    let decide: Decide = Arc::new(decide);
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
            let hits = hits_in.clone();
            let decide = decide.clone();
            async move {
                let index = hits.fetch_add(1, Ordering::SeqCst);
                let model = body["model"].as_str().unwrap_or_default().to_string();
                match decide(index, &model) {
                    Reply::Ok => ok_response(body["stream"].as_bool().unwrap_or(false)),
                    other => error_response(other),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), hits)
}

/// 前 `failures` 次瞬态失败、其后成功。
fn scripted(failures: usize) -> impl Fn(usize, &str) -> Reply + Send + Sync + 'static {
    move |index, _model| {
        if index < failures {
            Reply::Transient
        } else {
            Reply::Ok
        }
    }
}

/// `failing` 里的模型恒瞬态失败，其余恒成功。
fn failing_models(
    failing: &'static [&'static str],
) -> impl Fn(usize, &str) -> Reply + Send + Sync + 'static {
    move |_index, model| {
        if failing.contains(&model) {
            Reply::Transient
        } else {
            Reply::Ok
        }
    }
}

fn entries_for(agent: &str) -> Vec<RunLogEntry> {
    global_run_log()
        .lock()
        .expect("run log poisoned")
        .snapshot()
        .entries
        .into_iter()
        .filter(|entry| entry.agent == agent)
        .collect()
}

/// 剥离易变字段（ts/duration/agent/model）后的口径元组：
/// (ok, attempt_index, round, took_over, error_kind)。
fn gauge(entries: &[RunLogEntry]) -> Vec<(bool, i64, i64, bool, Option<&str>)> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.ok,
                entry.attempt_index,
                entry.round,
                entry.took_over,
                entry.error_kind.as_deref(),
            )
        })
        .collect()
}

/// 一次性成功：一条 ok、primary 轮、不接管。
#[tokio::test]
async fn plain_success_is_one_ok_entry() {
    let (base, hits) = spawn_mock_llm(scripted(0)).await;
    let router = AgentRouter::new(endpoint(base, "rl-plain"), HashMap::new());
    let outcome = router
        .chat("rl-plain-agent", user_message(), 0.2, None)
        .await;
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let entries = entries_for("rl-plain-agent");
    assert_eq!(gauge(&entries), vec![(true, 0, 1, false, None)]);
    assert_eq!(entries[0].model, "rl-plain");
    assert!(entries[0].duration_ms < 60_000);
}

/// 同模型瞬态重试（429 → 退避 → 429 → 退避 → 200）：**三次上线折叠成一条
/// ok**，round=1、不接管——瞬态重试既不是接管，也不是独立的一轮。
#[tokio::test]
async fn same_model_transient_retries_fold_into_one_ok_entry() {
    let (base, hits) = spawn_mock_llm(scripted(2)).await;
    let router = AgentRouter::new(endpoint(base, "rl-fold"), HashMap::new());
    let outcome = router
        .chat("rl-fold-agent", user_message(), 0.2, None)
        .await;
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(hits.load(Ordering::SeqCst), 3, "2 次瞬态失败后第 3 次命中");

    let entries = entries_for("rl-fold-agent");
    assert_eq!(gauge(&entries), vec![(true, 0, 1, false, None)]);
}

/// 瞬态耗尽（3 次 429）：**一条** transient 失败，不切换、不接管。
#[tokio::test]
async fn transient_exhaustion_is_one_transient_entry() {
    let (base, hits) = spawn_mock_llm(scripted(usize::MAX)).await;
    let router = AgentRouter::new(endpoint(base, "rl-exhaust"), HashMap::new());
    let outcome = router
        .chat("rl-exhaust-agent", user_message(), 0.2, None)
        .await;
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(hits.load(Ordering::SeqCst), 3, "1 次 + 2 次重试预算");

    let entries = entries_for("rl-exhaust-agent");
    assert_eq!(
        gauge(&entries),
        vec![(false, 0, 1, false, Some("transient"))]
    );
}

/// 非瞬态失败（401）：一条 fatal，立即失败不重试。
#[tokio::test]
async fn fatal_failure_is_one_fatal_entry_without_retry() {
    let (base, hits) = spawn_mock_llm(|_index, _model| Reply::Fatal).await;
    let router = AgentRouter::new(endpoint(base, "rl-fatal"), HashMap::new());
    let outcome = router
        .chat("rl-fatal-agent", user_message(), 0.2, None)
        .await;
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let entries = entries_for("rl-fatal-agent");
    assert_eq!(gauge(&entries), vec![(false, 0, 1, false, Some("fatal"))]);
}

/// 链层接管（R25）：primary 瞬态耗尽折叠成一条 transient，备用模型命中另记
/// 一条 ok——`attempt_index=1`、`took_over=true`。**接管只发生在链层。**
#[tokio::test]
async fn chain_takeover_is_a_separate_entry_with_took_over() {
    let (base, hits) = spawn_mock_llm(failing_models(&["rl-primary"])).await;
    let routing = TaskModelRouting {
        defaults: None,
        tasks: Some(BTreeMap::from([(
            "writing".to_string(),
            TaskModelOverride {
                model: Some("rl-primary".to_string()),
                retry_count: Some(1),
                backup_models: Some(vec!["rl-backup".to_string()]),
                ..Default::default()
            },
        )])),
    };
    let router = AgentRouter::new(endpoint(base, "rl-primary"), HashMap::new())
        .with_task_routing(Some(routing));
    let outcome = router.chat("writer", user_message(), 0.2, None).await;
    assert!(outcome.is_ok(), "{outcome:?}");
    // primary 3 次（瞬态耗尽）+ backup 1 次。
    assert_eq!(hits.load(Ordering::SeqCst), 4);

    // agent 名 "writer" 是任务映射键，不能换成唯一标签——按模型前缀过滤。
    let mut entries = entries_for("writer");
    entries.retain(|entry| entry.model.starts_with("rl-"));
    let models: Vec<&str> = entries.iter().map(|entry| entry.model.as_str()).collect();
    assert_eq!(models, vec!["rl-primary", "rl-backup"]);
    assert_eq!(
        gauge(&entries),
        vec![
            (false, 0, 1, false, Some("transient")),
            (true, 1, 1, true, None),
        ]
    );
}
