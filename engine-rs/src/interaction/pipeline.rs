//! 工具执行三段管线（R39，556 号）——dsh 三态调度器形态的本仓落地：
//! `pre（守卫）→ around（超时+重试+计时）→ body → post（钩子派链）`，
//! 全注册表分发单点 [`crate::interaction::registry::execute_routed`] 内接入，
//! 注册表内无旁路。
//!
//! - **守卫单调语义**（dsh「只 deny/abstain、无 allow、不可重排」照搬）：
//!   守卫只可拒绝，判定序=装配序，`Ask` 本轮归一为 `Deny("审批未启用")`
//!   （闸位预留，人审通道立项后启用）。
//! - **正交结果独立上报**：耗时/尝试序/超时/错误类别经
//!   [`PipelineObservation`] 与业务 [`ToolResult`] 分离——超时与业务失败
//!   互不吞没（defensive-patterns 规范条目）。
//! - **重试面裁剪**：仅只读工具（`MutationKind::ReadOnly`）对传输类瞬时
//!   错误重试（幂等安全）；写工具副作用非幂等，任何错误不重试。超时
//!   同理不重试。工具内重试与 R25 模型链 failover 计数天然正交（两条
//!   独立重试面，互不计入）。
//! - **panic 隔离**：工具体与 post 钩子均以 `catch_unwind` 包裹——钩子
//!   panic 记日志返回原结果，工具体 panic 转错误结果，不炸 agent 循环。

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::interaction::project_tools::{error_result, ToolResult};
use crate::interaction::registry::{MutationKind, ToolCtx, ToolRegistry};

// ── 守卫段 ────────────────────────────────────────────────────────────────

/// 守卫判定（dsh 单调语义：只 deny / 预留 ask，无 allow 通道）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardVerdict {
    Allow,
    /// 拒绝执行，文本进错误结果（"Blocked by guard: …"）。
    Deny(String),
    /// 人审闸位（本轮未启用，管线归一为 [`GuardVerdict::Deny`]）。
    Ask,
}

pub trait ToolGuard: Sync + Send {
    fn check(&self, name: &str, args: &Value, ctx: &ToolCtx<'_>) -> GuardVerdict;
}

/// 文件面纵深守卫（201 号 segment_guard 的工具面同族语义）：带 `path`
/// 参数的文件工具，`..` 段（`/` 或 `\` 任一分隔）在进工具体前拒绝——
/// 工具内 `safe_child_path` 仍是权威边界，本层为纵深预检（非 HTTP 入口的
/// 内部调用同样过管线，与中间件互补构成纵深，segment_guard 头注同语义）。
/// 绝对路径不拦——`read` 在系统读开启（env）下合法，守卫不可见 env 不越权。
pub struct PathTraversalGuard;

const PATH_ARG_TOOLS: [&str; 4] = ["read", "edit", "write", "write_truth_file"];

fn args_path_traverses(args: &Value) -> bool {
    let Some(path) = args.get("path").and_then(Value::as_str) else {
        return false;
    };
    path.split(['/', '\\']).any(|seg| seg == "..")
}

impl ToolGuard for PathTraversalGuard {
    fn check(&self, name: &str, args: &Value, _ctx: &ToolCtx<'_>) -> GuardVerdict {
        if PATH_ARG_TOOLS.contains(&name) && args_path_traverses(args) {
            return GuardVerdict::Deny(format!("path argument must not contain '..' segments: {name}"));
        }
        GuardVerdict::Allow
    }
}

/// 缺省守卫装配（声明序=判定序；OnceLock 静态零泄漏，registry::global 同法）。
pub fn default_guards() -> &'static [&'static dyn ToolGuard] {
    static GUARDS: std::sync::OnceLock<Vec<&'static dyn ToolGuard>> = std::sync::OnceLock::new();
    GUARDS.get_or_init(|| vec![&PathTraversalGuard])
}

// ── post 段 ───────────────────────────────────────────────────────────────

/// post 钩子（派链序 = 注册序；ctx 参数供落盘类钩子取项目根/会话标识——
/// R42 SpillHook 首消费）。
pub trait PostHook: Sync + Send {
    fn post<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        res: ToolResult,
    ) -> futures_util::future::BoxFuture<'a, ToolResult>;
}

/// 缺省 post 钩子装配（声明序 = 派链序；OnceLock 静态零泄漏）。
/// R42：SpillHook（超阈值结果头尾保留+全文落盘）。
pub fn default_post_hooks() -> &'static [Box<dyn PostHook>] {
    static HOOKS: std::sync::OnceLock<Vec<Box<dyn PostHook>>> = std::sync::OnceLock::new();
    HOOKS.get_or_init(|| vec![Box::new(crate::interaction::spill::SpillHook)])
}

// ── 观测段 ────────────────────────────────────────────────────────────────

/// 管线正交观测（与业务 [`ToolResult`] 独立上报；defensive-patterns 规范条目）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineObservation {
    /// body 净耗时（管线内计时段，非含守卫段的墙钟）。
    pub took_ms: u64,
    /// body 实际执行次数：0 = 未执行（unknown/suppress 分发面拒绝）；
    /// ≥1 = 执行过（含重试后的最终一次）。
    pub attempt: u32,
    /// 最后一次执行是否超时。
    pub timed_out: bool,
    /// 错误类别：None=成功或分发面拒绝；guard=被守卫拒；timeout=超时；
    /// transient=瞬时错误重试耗尽；error=业务错误 / panic。
    pub error_kind: Option<&'static str>,
}

/// 工具级超时缺省（按 MutationKind 定性映射，无新增配置面）：
/// 只读快操作 30s；项目写 300s（research_web 含多次外呼同水位）；
/// 生产变更面（sub_agent 等长 LLM 链）600s。
pub fn pipeline_timeout(kind: MutationKind) -> Duration {
    match kind {
        MutationKind::ReadOnly => Duration::from_secs(30),
        MutationKind::ProjectWrite => Duration::from_secs(300),
        MutationKind::ProductionMutation => Duration::from_secs(600),
    }
}

/// 工具面瞬时错误分类（传输类瞬时才值得重试；与 LLM 链
/// [`crate::llm::provider::is_retryable_llm_error`] 同族但文本面不同——
/// 工具错误是 reqwest/HTTP 文本形态，非 chat 链错误形态）。
pub fn is_retryable_tool_error(text: &str) -> bool {
    if crate::llm::provider::is_transient_llm_http_error(text) {
        return true;
    }
    const TOOL_TRANSIENT_PHRASES: [&str; 6] = [
        "error sending request",
        "connection refused",
        "connection reset",
        "broken pipe",
        "dns error",
        "operation timed out",
    ];
    let lower = text.to_lowercase();
    TOOL_TRANSIENT_PHRASES.iter().any(|p| lower.contains(p))
}

// ── 管线本体 ──────────────────────────────────────────────────────────────

/// 管线执行（分发单点内部形态）：
/// `lookup→available→suppress→guards(Deny 短路)→around{timeout+retry+计时}→body→post 派链`。
///
/// 返回 (业务结果, 观测)；[`crate::interaction::registry::execute_routed`]
/// 为丢观测薄壳（既有调用面零破坏）。
pub async fn run_pipeline(reg: &ToolRegistry, ctx: &ToolCtx<'_>, name: &str, args: &Value) -> (ToolResult, PipelineObservation) {
    // 超时按变更面定性映射（未知工具名取 ReadOnly 兜底——分发失败路径
    // 不受超时值影响）。
    let timeout = reg
        .mutation_kind(name)
        .map(pipeline_timeout)
        .unwrap_or(pipeline_timeout(MutationKind::ReadOnly));
    run_pipeline_with_timeout(reg, ctx, name, args, timeout).await
}

/// 超时可注入形态（测试载体；生产走 [`run_pipeline`] 定性映射）。
pub async fn run_pipeline_with_timeout(
    reg: &ToolRegistry,
    ctx: &ToolCtx<'_>,
    name: &str,
    args: &Value,
    timeout: Duration,
) -> (ToolResult, PipelineObservation) {
    let started = Instant::now();
    // ── 分发段（R38b 语义原样：available 门控 + 生产面抑制）──
    let Some(def) = reg.find(name, ctx) else {
        return (error_result(format!("Unknown tool: {name}")), PipelineObservation::default());
    };
    if ctx.suppress_production && def.mutation_kind() == MutationKind::ProductionMutation {
        return (error_result(format!("Unknown tool: {name}")), PipelineObservation::default());
    }

    // ── pre 段：守卫判定（装配序，Deny 短路；Ask 归一 Deny）──
    for guard in default_guards() {
        match guard.check(name, args, ctx) {
            GuardVerdict::Allow => {}
            GuardVerdict::Deny(reason) => {
                let observation = PipelineObservation {
                    took_ms: started.elapsed().as_millis() as u64,
                    attempt: 0,
                    timed_out: false,
                    error_kind: Some("guard"),
                };
                return (error_result(format!("Blocked by guard: {reason}")), observation);
            }
            // 闸位预留：人审通道未启用，Ask 归一拒绝（施工图 §3.1）。
            GuardVerdict::Ask => {
                let observation = PipelineObservation {
                    took_ms: started.elapsed().as_millis() as u64,
                    attempt: 0,
                    timed_out: false,
                    error_kind: Some("guard"),
                };
                return (error_result("Blocked by guard: approval flow not enabled".to_string()), observation);
            }
        }
    }

    // ── around 段：超时 + 重试 + 计时（loop 直接产出结果三元组——
    // 每条 break 路径终态自定，无中途占位）──
    let retryable = def.mutation_kind() == MutationKind::ReadOnly;
    const MAX_RETRIES: u32 = 2;
    let mut attempt: u32 = 0;
    let (mut result, timed_out, error_kind) = loop {
        attempt += 1;
        let body = def.execute(ctx, args);
        let guarded = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(body));
        match tokio::time::timeout(timeout, guarded).await {
            Err(_elapsed) => {
                // 超时不重试（写工具副作用非幂等；只读超时多为环境性，
                // 立即失败并如实标注）。
                break (
                    error_result(format!("Tool '{name}' timed out after {}s", timeout.as_secs())),
                    true,
                    Some("timeout"),
                );
            }
            Ok(Err(panic_payload)) => {
                // 工具体 panic：转错误结果，不炸 agent 循环（隔离不吞观测）。
                break (
                    error_result(format!("Tool '{name}' panicked: {}", panic_message(&panic_payload))),
                    false,
                    Some("error"),
                );
            }
            Ok(Ok(res)) => {
                if !res.is_error {
                    break (res, false, None);
                }
                if retryable && is_retryable_tool_error(&res.text) {
                    if attempt <= MAX_RETRIES {
                        continue;
                    }
                    // 瞬时错误重试耗尽：区别于业务错误的最终失败。
                    break (res, false, Some("transient"));
                }
                // 业务错误，或写工具的瞬时面错误（政策性不重试）→ 终态 error。
                break (res, false, Some("error"));
            }
        }
    };

    // ── post 段：钩子派链（注册序；panic 隔离记日志返回原结果）──
    // 原结果先克隆留底：钩子 future 独占持有入参，panic 即随栈展开丢弃。
    for hook in default_post_hooks() {
        let original = result.clone();
        let chained = std::panic::AssertUnwindSafe(hook.post(ctx, name, result));
        result = match futures_util::FutureExt::catch_unwind(chained).await {
            Ok(res) => res,
            Err(panic_payload) => {
                tracing::warn!(tool = name, panic = %panic_message(&panic_payload), "post hook panicked; returning original result");
                original
            }
        };
    }

    let observation = PipelineObservation {
        took_ms: started.elapsed().as_millis() as u64,
        attempt,
        timed_out,
        error_kind,
    };
    (result, observation)
}

/// panic 载荷消息提取（&str / String / 未知类型三态）。
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::registry::ToolDef;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    fn ctx() -> ToolCtx<'static> {
        // root 指向不存在的临时路径即可——守卫/分发面不触盘。
        let root: &'static std::path::Path =
            Box::leak(Box::new(std::path::PathBuf::from("/tmp/inkos-pipeline-test")));
        ToolCtx::root_only(root)
    }

    /// 可编程 mock 工具定义（管线行为测试载体）。
    struct MockDef {
        name: &'static str,
        kind: MutationKind,
        /// 每次执行的脚本（按调用序弹出；None = 返回 ok）。
        script: Mutex<Vec<MockOutcome>>,
        calls: AtomicU32,
    }

    enum MockOutcome {
        Ok(&'static str),
        Err(&'static str),
        Sleep(Duration),
        Panic,
    }

    impl MockDef {
        fn new(name: &'static str, kind: MutationKind, script: Vec<MockOutcome>) -> Box<Self> {
            Box::new(Self { name, kind, script: Mutex::new(script), calls: AtomicU32::new(0) })
        }
    }

    #[async_trait::async_trait]
    impl ToolDef for MockDef {
        fn name(&self) -> &'static str {
            self.name
        }
        fn description(&self) -> String {
            "mock".into()
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
        fn scope(&self) -> crate::interaction::registry::ToolScope {
            crate::interaction::registry::ToolScope::Project
        }
        fn mutation_kind(&self) -> MutationKind {
            self.kind
        }
        fn available(&self, _ctx: &ToolCtx<'_>) -> bool {
            true
        }
        async fn execute(&self, _ctx: &ToolCtx<'_>, _args: &Value) -> ToolResult {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let outcome = self.script.lock().unwrap().remove(0);
            match outcome {
                MockOutcome::Ok(text) => ToolResult { text: text.into(), details: None, is_error: false },
                MockOutcome::Err(text) => ToolResult { text: text.into(), details: None, is_error: true },
                MockOutcome::Sleep(d) => {
                    tokio::time::sleep(d).await;
                    ToolResult { text: "slept".into(), details: None, is_error: false }
                }
                MockOutcome::Panic => panic!("mock panic"),
            }
        }
    }

    /// 测试注册表：mock 件挂名，registry::global 不涉——管线函数接收注册表
    /// 参数正是为可测性（生产薄壳传 global()）。
    fn reg_with(def: Box<dyn ToolDef>) -> ToolRegistry {
        ToolRegistry::from_defs(vec![def])
    }

    #[tokio::test]
    async fn unknown_tool_short_circuits_without_attempt() {
        let registry = ToolRegistry::from_defs(vec![]);
        let (res, obs) = run_pipeline(&registry, &ctx(), "no_such_tool", &json!({})).await;
        assert!(res.is_error);
        assert!(res.text.contains("Unknown tool"));
        assert_eq!(obs.attempt, 0);
        assert_eq!(obs.error_kind, None);
    }

    #[tokio::test]
    async fn guard_denies_path_traversal_before_body() {
        let def = MockDef::new("read", MutationKind::ReadOnly, vec![]);
        let registry = reg_with(def);
        for evil in ["/../evil.txt", "a/../../b.txt", "..\\..\\secret", "ok/.."] {
            let (res, obs) = run_pipeline(&registry, &ctx(), "read", &json!({ "path": evil })).await;
            assert!(res.is_error, "evil={evil}");
            assert!(res.text.starts_with("Blocked by guard"), "evil={evil}");
            assert_eq!(obs.error_kind, Some("guard"), "evil={evil}");
            assert_eq!(obs.attempt, 0, "守卫拒绝不进 body");
        }
        // 非文件工具不适用该守卫；正常路径放行进 body。
        let def = MockDef::new(
            "research_web",
            MutationKind::ProjectWrite,
            vec![MockOutcome::Ok("searched")],
        );
        let registry = reg_with(def);
        let (res, obs) = run_pipeline(&registry, &ctx(), "research_web", &json!({ "path": "../x" })).await;
        assert!(!res.is_error);
        assert_eq!(obs.attempt, 1);
        assert_eq!(obs.error_kind, None);
    }

    #[tokio::test]
    async fn readonly_tool_retries_transient_error() {
        let def = MockDef::new(
            "read",
            MutationKind::ReadOnly,
            vec![
                MockOutcome::Err("Fetch failed: 503 service unavailable"),
                MockOutcome::Err("Fetch failed: error sending request for url"),
                MockOutcome::Ok("finally"),
            ],
        );
        let registry = reg_with(def);
        let (res, obs) = run_pipeline(&registry, &ctx(), "read", &json!({})).await;
        assert!(!res.is_error);
        assert_eq!(res.text, "finally");
        assert_eq!(obs.attempt, 3, "初试 + 2 重试");
        assert_eq!(obs.error_kind, None, "最终成功");
    }

    #[tokio::test]
    async fn readonly_tool_retries_exhausted_reports_transient() {
        let def = MockDef::new(
            "read",
            MutationKind::ReadOnly,
            vec![
                MockOutcome::Err("Fetch failed: 503"),
                MockOutcome::Err("Fetch failed: 503"),
                MockOutcome::Err("Fetch failed: 503"),
            ],
        );
        let registry = reg_with(def);
        let (res, obs) = run_pipeline(&registry, &ctx(), "read", &json!({})).await;
        assert!(res.is_error);
        assert_eq!(obs.attempt, 3);
        assert_eq!(obs.error_kind, Some("transient"), "瞬时重试耗尽如实标注");
    }

    #[tokio::test]
    async fn write_tool_never_retries() {
        // 写工具即使瞬时传输类错误也不重试（副作用非幂等）。
        let def = MockDef::new(
            "write_truth_file",
            MutationKind::ProductionMutation,
            vec![MockOutcome::Err("Fetch failed: 503")],
        );
        let registry = reg_with(def);
        let (res, obs) = run_pipeline(&registry, &ctx(), "write_truth_file", &json!({})).await;
        assert!(res.is_error);
        assert_eq!(obs.attempt, 1, "写工具零重试");
        assert_eq!(obs.error_kind, Some("error"));
    }

    #[tokio::test]
    async fn business_error_is_not_retried() {
        let def = MockDef::new(
            "read",
            MutationKind::ReadOnly,
            vec![MockOutcome::Err("read failed: No such file")],
        );
        let registry = reg_with(def);
        let (res, obs) = run_pipeline(&registry, &ctx(), "read", &json!({})).await;
        assert!(res.is_error);
        assert_eq!(obs.attempt, 1, "非瞬时业务错误不重试");
        assert_eq!(obs.error_kind, Some("error"));
    }

    #[tokio::test]
    async fn timeout_marks_observation_and_does_not_retry() {
        let def = MockDef::new(
            "read",
            MutationKind::ReadOnly,
            vec![MockOutcome::Sleep(Duration::from_millis(500))],
        );
        let registry = reg_with(def);
        // 超时可注入形态（run_pipeline_with_timeout）：body sleep 500ms ≫
        // 50ms 上限 → around 段截断；超时不重试且观测如实标注。
        let (res, obs) = run_pipeline_with_timeout(
            &registry,
            &ctx(),
            "read",
            &json!({}),
            Duration::from_millis(50),
        )
        .await;
        assert!(res.is_error);
        assert!(res.text.contains("timed out"), "res={}", res.text);
        assert!(obs.timed_out);
        assert_eq!(obs.error_kind, Some("timeout"));
        assert_eq!(obs.attempt, 1, "超时不重试");
    }

    #[tokio::test]
    async fn timeout_mapping_follows_mutation_kind() {
        assert_eq!(pipeline_timeout(MutationKind::ReadOnly), Duration::from_secs(30));
        assert_eq!(pipeline_timeout(MutationKind::ProjectWrite), Duration::from_secs(300));
        assert_eq!(pipeline_timeout(MutationKind::ProductionMutation), Duration::from_secs(600));
    }

    #[tokio::test]
    async fn body_panic_is_contained_as_error() {
        let def = MockDef::new("read", MutationKind::ReadOnly, vec![MockOutcome::Panic]);
        let registry = reg_with(def);
        let (res, obs) = run_pipeline(&registry, &ctx(), "read", &json!({})).await;
        assert!(res.is_error);
        assert!(res.text.contains("panicked"));
        assert_eq!(obs.error_kind, Some("error"));
        assert_eq!(obs.attempt, 1);
    }

    #[tokio::test]
    async fn post_hooks_chain_in_order_and_isolate_panics() {
        struct AppendHook(&'static str);
        impl PostHook for AppendHook {
            fn post<'a>(
                &'a self,
                _ctx: &'a ToolCtx<'a>,
                _name: &'a str,
                mut res: ToolResult,
            ) -> futures_util::future::BoxFuture<'a, ToolResult> {
                Box::pin(async move {
                    res.text.push_str(self.0);
                    res
                })
            }
        }
        struct PanicHook;
        impl PostHook for PanicHook {
            fn post<'a>(
                &'a self,
                _ctx: &'a ToolCtx<'a>,
                _name: &'a str,
                res: ToolResult,
            ) -> futures_util::future::BoxFuture<'a, ToolResult> {
                Box::pin(async move {
                    let _ = res;
                    panic!("hook boom");
                })
            }
        }
        let def = MockDef::new("read", MutationKind::ReadOnly, vec![MockOutcome::Ok("body")]);
        let registry = reg_with(def);
        // 派链序与 panic 隔离经显式 hook 列表验证——SpillHook 行为由 spill.rs
        // 测试覆盖，此处仅验证派链机制本身。
        let h1: Box<dyn PostHook> = Box::new(PanicHook);
        let h2: Box<dyn PostHook> = Box::new(AppendHook("+h2"));
        let hooks: Vec<Box<dyn PostHook>> = vec![h1, h2];
        let ctx = ctx();
        let initial = ToolResult { text: "body".into(), details: None, is_error: false };
        let mut res = initial;
        for hook in &hooks {
            let original = res.clone();
            let chained = std::panic::AssertUnwindSafe(hook.post(&ctx, "read", res));
            res = match futures_util::FutureExt::catch_unwind(chained).await {
                Ok(res) => res,
                Err(payload) => {
                    assert!(panic_message(&payload).contains("hook boom"));
                    original
                }
            };
        }
        assert_eq!(res.text, "body+h2", "panic 钩子跳过、后续钩子继续");
        let _ = registry;
    }

    #[tokio::test]
    async fn transient_classifier_faces() {
        assert!(is_retryable_tool_error("Fetch failed: 429 rate limited"));
        assert!(is_retryable_tool_error("Fetch failed: error sending request for url (http://x)"));
        assert!(is_retryable_tool_error("connection refused"));
        assert!(is_retryable_tool_error("operation timed out"));
        assert!(!is_retryable_tool_error("read failed: No such file or directory"));
        assert!(!is_retryable_tool_error("research_web requires a topic argument"));
        assert!(!is_retryable_tool_error("Invalid research_web.purpose: x"));
    }
}
