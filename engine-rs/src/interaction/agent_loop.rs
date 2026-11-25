//! agent 多轮 tool-use 循环（pi-agent loop 核心子集）。
//!
//! 移植自 `packages/core/src/interaction/runtime.ts` 的 loop 骨架：
//! LLM ↔ 工具调用多轮循环（上限 12 轮 + 回合守卫）、SSE 增量事件回调（draft:delta /
//! tool:start|end）、abort 中途截断、工具执行卡收集。历史回放与完整事件面
//! 的其余部分随 67 号（restore.ts 后半 + 生产工具）。

use std::collections::{hash_map::DefaultHasher, VecDeque};
use std::hash::Hasher;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::llm::provider::{LLMMessage, LLMRole};

/// 循环事件回调（SSE 广播的桥）。
pub trait LoopEvents: Send + Sync {
    /// 文本增量（每轮聚合文本，draft:delta）。
    fn on_delta(&self, _text: &str) {}
    /// 工具开始（tool:start）。
    fn on_tool_start(&self, _id: &str, _tool: &str, _args: &Value) {}
    /// 工具结束（tool:end）。
    fn on_tool_end(
        &self,
        _id: &str,
        _tool: &str,
        _result_text: &str,
        _details: Option<&Value>,
        _is_error: bool,
    ) {
    }
}

/// 无操作回调。
pub struct NoopEvents;
impl LoopEvents for NoopEvents {}

/// abort 句柄（65 号注册表的条目）。
pub type AbortHandle = Arc<Mutex<bool>>;

/// 单次工具执行卡（对齐 TS CollectedToolExec：结构化 details 原样携带，
/// 未提供时序列化省略键）。R39 观测四字段（tookMs/attempt/timedOut/
/// errorKind）为 Rust 超集加法（health backend 先例）：执行器未接管管线
/// 时 None，卡片投影缺键；TS 面不消费（未知键忽略）。
#[derive(Debug, Clone)]
pub struct LoopToolExecution {
    pub id: String,
    pub tool: String,
    pub args: Value,
    pub status: &'static str,
    pub result: Option<String>,
    pub error: Option<String>,
    pub details: Option<Value>,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub took_ms: Option<u64>,
    pub attempt: Option<u32>,
    pub timed_out: Option<bool>,
    pub error_kind: Option<&'static str>,
}

/// G8a/333 号 AI 实况：token 用量（多轮累加，上游 usage 权威值；字段名对齐
/// TS 响应面 `usage: { input, output, totalTokens }`）。
/// R41（563 号）：`model` 供计量锚点身份（仅计量面消费；响应面 skip 缺省
/// 不出键，卡片投影零漂移）。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct LoopUsage {
    pub input: u64,
    pub output: u64,
    #[serde(rename = "totalTokens")]
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// G8a/333 号 AI 实况：首包/总耗时（毫秒；首包=起点到首个文本输出）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopTimings {
    pub first_token_ms: u64,
    pub total_ms: u64,
}

/// 循环结果。
pub struct LoopOutcome {
    pub response_text: String,
    pub tool_executions: Vec<LoopToolExecution>,
    pub aborted: bool,
    pub usage: LoopUsage,
    pub timings: LoopTimings,
}

/// 单轮 LLM 调用的抽象（测试注入 + AgentRouter 适配）。
#[async_trait::async_trait]
pub trait LoopChat: Send + Sync {
    /// 返回 (content, tool_calls[(id, name, arguments_json)], usage)。
    async fn chat(
        &self,
        messages: &[LLMMessage],
        tools: Option<&Value>,
    ) -> Result<(String, Vec<(String, String, String)>, LoopUsage), String>;
}

/// 工具执行抽象（80 号：play 聊天工具需要会话/路由上下文，文件工具只需
/// 项目根——trait 让 loop 与具体工具面解耦）。
#[async_trait::async_trait]
pub trait LoopToolExecutor: Send + Sync {
    async fn execute(
        &self,
        name: &str,
        args: &Value,
    ) -> crate::interaction::project_tools::ToolResult;

    /// 最近一次 [`Self::execute`] 的管线正交观测（R39）。缺省 None——
    /// 未接管三段管线的执行器（文件工具兜底链等）零成本适配；接管方
    /// （ChatToolRouter）在 execute 内存取。单轮内 execute 串行 await，
    /// 「最近一次」在读取点无歧义。
    fn last_observation(&self) -> Option<crate::interaction::pipeline::PipelineObservation> {
        None
    }
}

/// agent 循环：system + 历史回放 + user 起始，工具调用逐轮执行回填，直至
/// 模型给出无工具的最终文本、达到轮次上限，或触发回合守卫（连续若干轮只调不存在
/// 的工具 / 连续若干轮原地重复，见 `run_agent_loop` 内的常量）。abort 句柄每轮前轮询。
/// `initial_history`（68 号）：transcript 回放的 summary/对话/boundary 消息，
/// 插在 system 与本轮 user 之间（pi-agent initialState.messages 的等价位置）。
/// `tool_exec`（80 号）：工具执行面（文件工具 / play 聊天工具）。
/// `meter`（R41/563 号）：token 计量器（Some 时开局入表 system+历史+指令，
/// 每轮 chat 后 usage 入锚 + assistant/工具面追加——表语义 = 当前请求面；
/// None 零行为）。
#[allow(clippy::too_many_arguments)]
pub async fn run_agent_loop(
    chat: &dyn LoopChat,
    tool_exec: &dyn LoopToolExecutor,
    system_prompt: &str,
    initial_history: Vec<LLMMessage>,
    instruction: &str,
    tools: Option<&Value>,
    abort: Option<&AbortHandle>,
    events: &dyn LoopEvents,
    meter: Option<&crate::utils::token_meter::TokenMeter>,
) -> Result<LoopOutcome, String> {
    const MAX_ROUNDS: usize = 12;
    // 回合守卫：到阈值就提前收场，不再打满 MAX_ROUNDS（结局与轮次上限相同：空回复
    // + 工具错误明细）。与 Node 腿同值（packages/studio/src/__tests__/golden/
    // agent-loop-guards.json）。两层：
    //   ① 连续若干轮整轮只调了不存在的工具——什么都不会执行。只认「整轮全是」，
    //      夹着已知工具的轮次归零。
    //   ② 连续若干轮与本回合最近 REPEAT_LOOKBACK_ROUNDS 轮里的某一轮完全相同
    //      （工具名、参数、结果文本都一样）——同样的动作拿到同样的结果，再来一次
    //      也不会不同。参数变了或结果变了都是新的一轮。
    // 窗口每回合从空开始（本函数每次调用只管一个回合）。
    // Node 腿还有「整轮的工具结果全是错误」一层，这里不做：文件工具没有精确的
    // 失败标记（只有文本启发式），不用启发式做停止判定；轮次上限已经兜底。
    const MAX_CONSECUTIVE_UNKNOWN_TOOL_ROUNDS: usize = 5;
    const MAX_REPEATED_ROUNDS: usize = 6;
    const REPEAT_LOOKBACK_ROUNDS: usize = 8;
    let mut unknown_tool_rounds: usize = 0;
    let mut repeated_rounds: usize = 0;
    let mut recent_rounds: VecDeque<u64> = VecDeque::with_capacity(REPEAT_LOOKBACK_ROUNDS);
    let is_aborted = || {
        abort
            .and_then(|handle| {
                let guard = handle.lock().ok()?;
                Some(*guard)
            })
            .unwrap_or(false)
    };

    let mut messages = vec![LLMMessage {
        role: LLMRole::System,
        content: system_prompt.to_string(),
        tool_calls: None,
        tool_call_id: None,
    }];
    // R41/563 号：计量开局入表（当前请求面 = system + 历史 + 指令）。
    if let Some(m) = meter {
        m.append(system_prompt);
    }
    for message in initial_history {
        if let Some(m) = meter {
            m.append(&message.content);
        }
        messages.push(message);
    }
    if let Some(m) = meter {
        m.append(instruction);
    }
    messages.push(LLMMessage {
        role: LLMRole::User,
        content: instruction.to_string(),
        tool_calls: None,
        tool_call_id: None,
    });
    let mut executions: Vec<LoopToolExecution> = Vec::new();
    // G8a/333 号：AI 实况——多轮 usage 累加 + 首包（首个文本输出）计时。
    let started_ms = crate::interaction::session::utc_now_ms();
    let mut first_token_ms: u64 = 0;
    let mut total_usage = LoopUsage::default();

    for _round in 0..MAX_ROUNDS {
        if is_aborted() {
            return Ok(LoopOutcome {
                response_text: String::new(),
                tool_executions: executions,
                aborted: true,
                usage: total_usage,
                timings: LoopTimings {
                    first_token_ms,
                    total_ms: crate::interaction::session::utc_now_ms() - started_ms,
                },
            });
        }
        // 624 号：abort 不能只在轮间检查——单轮 LLM 调用可能挂起或长流，
        // 在飞期间用户点停止/换向时引擎会白烧整个调用并误 commit（真机走查
        // 实锤：中止轮照常 200 返回且落盘）。TS 侧 signal 直通 streamFunction
        //（在飞即断），此处 select 对齐双端语义：旗标置位即取消本次调用，
        // 按 aborted 轮出约（217 号契约：request_failed 不落盘、500 返回）。
        let (content, tool_calls, usage) = {
            let abort_waiter = async {
                loop {
                    if is_aborted() {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            };
            tokio::select! {
                result = chat.chat(&messages, tools) => result?,
                _ = abort_waiter, if abort.is_some() => {
                    return Ok(LoopOutcome {
                        response_text: String::new(),
                        tool_executions: executions,
                        aborted: true,
                        usage: total_usage,
                        timings: LoopTimings { first_token_ms, total_ms: crate::interaction::session::utc_now_ms() - started_ms },
                    });
                }
            }
        };
        total_usage.input += usage.input;
        total_usage.output += usage.output;
        total_usage.total_tokens += usage.total_tokens;
        // R41/563 号：usage 入锚（权威值校准比）；assistant 面入表（内容 +
        // tool_calls 参数串 = 下一请求将新增的面）。
        if let Some(m) = meter {
            m.note_usage(
                usage.model.as_deref(),
                usage.input,
                usage.output,
                usage.total_tokens,
            );
            m.append(&content);
            for (_, _, arguments_json) in &tool_calls {
                m.append(arguments_json);
            }
        }
        if !content.trim().is_empty() {
            if first_token_ms == 0 {
                first_token_ms = crate::interaction::session::utc_now_ms() - started_ms;
            }
            events.on_delta(content.trim());
        }
        if tool_calls.is_empty() {
            return Ok(LoopOutcome {
                response_text: content.trim().to_string(),
                tool_executions: executions,
                aborted: false,
                usage: total_usage,
                timings: LoopTimings {
                    first_token_ms,
                    total_ms: crate::interaction::session::utc_now_ms() - started_ms,
                },
            });
        }

        // assistant 轮（带 tool_calls 数组，OpenAI 形态）
        let tool_calls_json: Vec<Value> = tool_calls
            .iter()
            .map(|(id, name, arguments)| {
                json!({ "id": id, "type": "function", "function": { "name": name, "arguments": arguments } })
            })
            .collect();
        messages.push(LLMMessage {
            role: LLMRole::Assistant,
            content: content.clone(),
            tool_calls: Some(json!(tool_calls_json)),
            tool_call_id: None,
        });

        let mut round_calls = 0usize;
        let mut round_unknown_calls = 0usize;
        // 本轮指纹：工具名、参数、结果文本逐段喂给哈希器，不拼大字符串。
        let mut round_hasher = DefaultHasher::new();
        for (id, name, arguments_json) in &tool_calls {
            let args: Value = serde_json::from_str(arguments_json).unwrap_or(json!({}));
            events.on_tool_start(id, name, &args);
            let started = crate::interaction::session::utc_now_ms();
            let result = tool_exec.execute(name, &args).await;
            let completed = crate::interaction::session::utc_now_ms();
            let observation = tool_exec.last_observation();
            let unknown_tool = result.text.starts_with("Unknown tool");
            let is_error = result.is_error
                || (result.details.is_none()
                    && result
                        .text
                        .starts_with(|c: char| !c.is_ascii_alphanumeric())
                    && result.text.contains("escapes"))
                || result.text.contains("failed:")
                || unknown_tool;
            round_calls += 1;
            if unknown_tool {
                round_unknown_calls += 1;
            }
            round_hasher.write(name.as_bytes());
            round_hasher.write_u8(0);
            round_hasher.write(arguments_json.as_bytes());
            round_hasher.write_u8(1);
            round_hasher.write(result.text.as_bytes());
            round_hasher.write_u8(2);
            events.on_tool_end(
                id,
                name,
                &result.text,
                (!is_error).then_some(result.details.as_ref()).flatten(),
                is_error,
            );
            executions.push(LoopToolExecution {
                id: id.clone(),
                tool: name.clone(),
                args: args.clone(),
                status: if is_error { "error" } else { "completed" },
                result: (!is_error).then(|| result.text.chars().take(2000).collect::<String>()),
                error: is_error.then(|| result.text.chars().take(500).collect::<String>()),
                details: (!is_error).then(|| result.details.clone()).flatten(),
                started_at: started,
                completed_at: Some(completed),
                took_ms: observation.map(|o| o.took_ms),
                attempt: observation.map(|o| o.attempt),
                timed_out: observation.map(|o| o.timed_out),
                error_kind: observation.and_then(|o| o.error_kind),
            });
            messages.push(LLMMessage {
                role: LLMRole::Tool,
                content: result.text.chars().take(8000).collect::<String>(),
                tool_calls: None,
                tool_call_id: Some(id.clone()),
            });
            // R41/563 号：工具结果入表（截断与请求面同口径 8000）。
            if let Some(m) = meter {
                m.append(&result.text.chars().take(8000).collect::<String>());
            }
        }
        if round_unknown_calls == round_calls {
            unknown_tool_rounds += 1;
        } else {
            unknown_tool_rounds = 0;
        }
        let digest = round_hasher.finish();
        repeated_rounds = if recent_rounds.contains(&digest) {
            repeated_rounds + 1
        } else {
            0
        };
        recent_rounds.push_back(digest);
        if recent_rounds.len() > REPEAT_LOOKBACK_ROUNDS {
            recent_rounds.pop_front();
        }
        if unknown_tool_rounds >= MAX_CONSECUTIVE_UNKNOWN_TOOL_ROUNDS
            || repeated_rounds >= MAX_REPEATED_ROUNDS
        {
            break;
        }
    }

    Ok(LoopOutcome {
        response_text: String::new(),
        tool_executions: executions,
        aborted: false,
        usage: total_usage,
        timings: LoopTimings {
            first_token_ms,
            total_ms: crate::interaction::session::utc_now_ms() - started_ms,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ScriptedChat {
        rounds: Vec<ScriptedRound>,
        calls: Mutex<Vec<usize>>,
    }

    type ScriptedRound = (String, Vec<(String, String, String)>, LoopUsage);

    #[async_trait::async_trait]
    impl LoopChat for ScriptedChat {
        async fn chat(
            &self,
            _messages: &[LLMMessage],
            _tools: Option<&Value>,
        ) -> Result<(String, Vec<(String, String, String)>, LoopUsage), String> {
            let index = {
                let mut calls = self
                    .calls
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let index = calls.len();
                calls.push(index);
                index
            };
            self.rounds
                .get(index)
                .cloned()
                .ok_or_else(|| "no more rounds".to_string())
        }
    }

    #[tokio::test]
    async fn loop_runs_tool_then_finishes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "hello agent").unwrap();
        let chat = ScriptedChat {
            rounds: vec![
                (
                    "让我看看文件".into(),
                    vec![("call_1".into(), "read".into(), r#"{"path":"a.md"}"#.into())],
                    LoopUsage {
                        input: 100,
                        output: 20,
                        total_tokens: 120,
                        model: None,
                    },
                ),
                (
                    "文件内容是 hello agent。".into(),
                    vec![],
                    LoopUsage {
                        input: 150,
                        output: 30,
                        total_tokens: 180,
                        model: None,
                    },
                ),
            ],
            calls: Mutex::new(vec![]),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let outcome = run_agent_loop(
            &chat,
            &executor,
            "sys",
            Vec::new(),
            "读一下",
            Some(&crate::interaction::project_tools::tools_payload()),
            None,
            &NoopEvents,
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.response_text, "文件内容是 hello agent。");
        assert_eq!(outcome.tool_executions.len(), 1);
        assert_eq!(outcome.tool_executions[0].tool, "read");
        assert_eq!(outcome.tool_executions[0].status, "completed");
        assert!(outcome.tool_executions[0]
            .result
            .as_deref()
            .unwrap()
            .contains("hello agent"));
        // G8a/333 号：多轮 usage 累加 + 计时非负。
        assert_eq!(
            outcome.usage,
            LoopUsage {
                input: 250,
                output: 50,
                total_tokens: 300,
                model: None
            }
        );
        assert!(outcome.timings.total_ms < u64::MAX);
    }

    #[tokio::test]
    async fn abort_stops_before_first_round() {
        let dir = tempfile::tempdir().unwrap();
        let chat = ScriptedChat {
            rounds: vec![("x".into(), vec![], LoopUsage::default())],
            calls: Mutex::new(vec![]),
        };
        let abort: AbortHandle = Arc::new(Mutex::new(true));
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let outcome = run_agent_loop(
            &chat,
            &executor,
            "sys",
            Vec::new(),
            "hi",
            None,
            Some(&abort),
            &NoopEvents,
            None,
        )
        .await
        .unwrap();
        assert!(outcome.aborted);
        assert!(outcome.response_text.is_empty());
        assert_eq!(outcome.usage, LoopUsage::default(), "abort 后无 usage 累加");
        assert_eq!(
            chat.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            0,
            "abort 后未发起 LLM 调用"
        );
    }

    /// 624 号：模拟在飞 LLM 调用——chat() 内部置位 abort（用户点停止）后
    /// 挂起永不返回；select 必须取消本次调用并按 aborted 轮出约。
    struct InflightAbortChat {
        abort: AbortHandle,
    }

    #[async_trait::async_trait]
    impl LoopChat for InflightAbortChat {
        async fn chat(
            &self,
            _messages: &[LLMMessage],
            _tools: Option<&Value>,
        ) -> Result<(String, Vec<(String, String, String)>, LoopUsage), String> {
            *self
                .abort
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
            std::future::pending::<()>().await;
            unreachable!("pending future 不应解析")
        }
    }

    #[tokio::test]
    async fn abort_interrupts_inflight_llm_call() {
        let dir = tempfile::tempdir().unwrap();
        let abort: AbortHandle = Arc::new(Mutex::new(false));
        let chat = InflightAbortChat {
            abort: abort.clone(),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        // 旧代码（轮间才检查）会挂死整个测试，timeout 保证可证伪地转红。
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run_agent_loop(
                &chat,
                &executor,
                "sys",
                Vec::new(),
                "hi",
                None,
                Some(&abort),
                &NoopEvents,
                None,
            ),
        )
        .await
        .expect("在飞中止必须取消 LLM 调用，而非等到调用自然结束")
        .unwrap();
        assert!(outcome.aborted);
        assert!(outcome.response_text.is_empty());
        assert_eq!(
            outcome.usage,
            LoopUsage::default(),
            "在飞中止后无 usage 累加"
        );
    }

    #[tokio::test]
    async fn unknown_tool_is_error_card() {
        let dir = tempfile::tempdir().unwrap();
        let chat = ScriptedChat {
            rounds: vec![
                (
                    "".into(),
                    vec![("call_1".into(), "nope".into(), "{}".into())],
                    LoopUsage::default(),
                ),
                ("done".into(), vec![], LoopUsage::default()),
            ],
            calls: Mutex::new(vec![]),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let outcome = run_agent_loop(
            &chat,
            &executor,
            "sys",
            Vec::new(),
            "hi",
            None,
            None,
            &NoopEvents,
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.tool_executions[0].status, "error");
        assert!(outcome.tool_executions[0]
            .error
            .as_deref()
            .unwrap()
            .contains("Unknown tool"));
    }

    /// 回合守卫的阈值：两腿共用对照表（Node 腿 agent-turn-guard.flow.test.ts 读同一
    /// 文件）。改这些数必须两腿一起改——下面的用例都按它们排脚本。
    fn loop_guard(key: &str) -> usize {
        let golden: Value = serde_json::from_str(include_str!(
            "../../../packages/studio/src/__tests__/golden/agent-loop-guards.json"
        ))
        .unwrap();
        golden[key]
            .as_u64()
            .unwrap_or_else(|| panic!("golden 须有 {key}")) as usize
    }

    fn unknown_tool_guard() -> usize {
        loop_guard("maxConsecutiveUnknownToolRounds")
    }

    /// 一轮只读一个文件（真实工具、成功；同路径同内容 = 同一轮）。
    fn read_round(path: &str) -> ScriptedRound {
        (
            String::new(),
            vec![(
                "call_read".into(),
                "read".into(),
                format!(r#"{{"path":"{path}"}}"#),
            )],
            LoopUsage::default(),
        )
    }

    fn unknown_tool_round() -> ScriptedRound {
        (
            String::new(),
            vec![("call".into(), "nope".into(), "{}".into())],
            LoopUsage::default(),
        )
    }

    fn call_count(chat: &ScriptedChat) -> usize {
        chat.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    async fn run_rounds(
        root: &std::path::Path,
        rounds: Vec<ScriptedRound>,
    ) -> (LoopOutcome, usize) {
        let chat = ScriptedChat {
            rounds,
            calls: Mutex::new(vec![]),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root };
        let outcome = run_agent_loop(
            &chat,
            &executor,
            "sys",
            Vec::new(),
            "hi",
            None,
            None,
            &NoopEvents,
            None,
        )
        .await
        .unwrap();
        (outcome, call_count(&chat))
    }

    #[tokio::test]
    async fn consecutive_unknown_tool_rounds_stop_before_the_round_cap() {
        let dir = tempfile::tempdir().unwrap();
        let guard = unknown_tool_guard();
        // 12 = MAX_ROUNDS：守卫必须比轮次上限小，这条守卫才有意义。
        assert!(guard < 12, "守卫上限须小于轮次上限");
        let chat = ScriptedChat {
            // 多给几轮：没有守卫会一直打满轮次上限。
            rounds: (0..12).map(|_| unknown_tool_round()).collect(),
            calls: Mutex::new(vec![]),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let outcome = run_agent_loop(
            &chat,
            &executor,
            "sys",
            Vec::new(),
            "hi",
            None,
            None,
            &NoopEvents,
            None,
        )
        .await
        .unwrap();
        assert_eq!(call_count(&chat), guard, "连续未知轮到上限即停");
        assert_eq!(outcome.tool_executions.len(), guard);
        assert!(outcome
            .tool_executions
            .iter()
            .all(|execution| execution.status == "error"));
        // 结局与轮次上限相同：空回复、未中止。
        assert!(outcome.response_text.is_empty());
        assert!(!outcome.aborted);
    }

    #[tokio::test]
    async fn a_round_with_a_real_tool_resets_the_unknown_tool_count() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "hello agent").unwrap();
        let guard = unknown_tool_guard();
        let mut rounds: Vec<ScriptedRound> = (0..2).map(|_| unknown_tool_round()).collect();
        // 同一轮里夹着一次真实读取：这轮不算未知轮，计数归零。
        rounds.push((
            String::new(),
            vec![
                ("call_unknown".into(), "nope".into(), "{}".into()),
                (
                    "call_read".into(),
                    "read".into(),
                    r#"{"path":"a.md"}"#.into(),
                ),
            ],
            LoopUsage::default(),
        ));
        // 归零之后再连续 guard-1 轮未知：差一轮到上限，模型改口后照常收尾。
        rounds.extend((0..guard - 1).map(|_| unknown_tool_round()));
        rounds.push(("done".into(), vec![], LoopUsage::default()));
        let chat = ScriptedChat {
            rounds,
            calls: Mutex::new(vec![]),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let outcome = run_agent_loop(
            &chat,
            &executor,
            "sys",
            Vec::new(),
            "hi",
            None,
            None,
            &NoopEvents,
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.response_text, "done");
        assert_eq!(call_count(&chat), 2 + 1 + (guard - 1) + 1);
        assert_eq!(outcome.tool_executions.len(), 2 + 2 + (guard - 1));
    }

    /// 每次被调用前先改写目标文件：同一个读取调用、每轮结果都不同。
    struct RewritingChat {
        root: std::path::PathBuf,
        calls: Mutex<usize>,
    }

    #[async_trait::async_trait]
    impl LoopChat for RewritingChat {
        async fn chat(
            &self,
            _messages: &[LLMMessage],
            _tools: Option<&Value>,
        ) -> Result<(String, Vec<(String, String, String)>, LoopUsage), String> {
            let index = {
                let mut calls = self
                    .calls
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *calls += 1;
                *calls
            };
            if index > 12 {
                return Err("no more rounds".to_string());
            }
            std::fs::write(self.root.join("a.md"), format!("revision {index}")).unwrap();
            Ok((
                String::new(),
                vec![("call".into(), "read".into(), r#"{"path":"a.md"}"#.into())],
                LoopUsage::default(),
            ))
        }
    }

    #[tokio::test]
    async fn repeated_identical_rounds_stop_before_the_round_cap() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "hello agent").unwrap();
        let repeat = loop_guard("maxRepeatedRounds");
        assert!(repeat < 12, "重复阈值加原样那一轮须不超过轮次上限");
        let (outcome, calls) =
            run_rounds(dir.path(), (0..12).map(|_| read_round("a.md")).collect()).await;
        // 第 1 轮是原样，之后连续 repeat 轮都是重复即停。
        assert_eq!(calls, repeat + 1, "原样重复到阈值即停");
        assert_eq!(outcome.tool_executions.len(), repeat + 1);
        assert!(outcome
            .tool_executions
            .iter()
            .all(|execution| execution.status == "completed"));
        // 结局与轮次上限相同：空回复、未中止。
        assert!(outcome.response_text.is_empty());
        assert!(!outcome.aborted);
    }

    #[tokio::test]
    async fn a_two_step_loop_of_two_different_reads_stops() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "hello agent").unwrap();
        std::fs::write(dir.path().join("b.md"), "goodbye agent").unwrap();
        let repeat = loop_guard("maxRepeatedRounds");
        let rounds = (0..12)
            .map(|index| read_round(if index % 2 == 0 { "a.md" } else { "b.md" }))
            .collect();
        let (_outcome, calls) = run_rounds(dir.path(), rounds).await;
        // A、B 两轮是原样，从第 3 轮起每一轮都是重复。
        assert_eq!(calls, repeat + 2, "周期 2 的循环：两轮原样后重复到阈值即停");
    }

    #[tokio::test]
    async fn different_arguments_are_a_different_round_even_when_the_result_is_the_same() {
        let dir = tempfile::tempdir().unwrap();
        // 两个文件内容相同：调用参数不同、结果文本相同。按两步循环处理；
        // 若参数不进指纹，两轮会被当成同一轮，提前一轮停。
        std::fs::write(dir.path().join("a.md"), "same text").unwrap();
        std::fs::write(dir.path().join("b.md"), "same text").unwrap();
        let repeat = loop_guard("maxRepeatedRounds");
        let rounds = (0..12)
            .map(|index| read_round(if index % 2 == 0 { "a.md" } else { "b.md" }))
            .collect();
        let (_outcome, calls) = run_rounds(dir.path(), rounds).await;
        assert_eq!(calls, repeat + 2);
    }

    #[tokio::test]
    async fn identical_calls_with_changing_results_are_not_repeats() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "revision 0").unwrap();
        let chat = RewritingChat {
            root: dir.path().to_path_buf(),
            calls: Mutex::new(0),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let outcome = run_agent_loop(
            &chat,
            &executor,
            "sys",
            Vec::new(),
            "hi",
            None,
            None,
            &NoopEvents,
            None,
        )
        .await
        .unwrap();
        // 每轮都拿到新内容，不是原地重复：守卫不触发，一直打到轮次上限（12）。
        // 若结果不进指纹，会在第 repeat+1 轮被当成重复而提前停。
        let calls = *chat
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls, 12);
        assert!(outcome.response_text.is_empty());
    }

    #[tokio::test]
    async fn a_round_that_has_not_happened_before_resets_the_repeat_count() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "hello agent").unwrap();
        std::fs::write(dir.path().join("b.md"), "goodbye agent").unwrap();
        let repeat = loop_guard("maxRepeatedRounds");
        // 12 = MAX_ROUNDS：A ×(repeat-1)、B、A ×(repeat-1)，第 12 轮收尾。
        assert_eq!(2 * repeat, 12, "脚本须恰好排在轮次上限内收尾");
        let mut rounds: Vec<ScriptedRound> = (0..repeat - 1).map(|_| read_round("a.md")).collect();
        rounds.push(read_round("b.md"));
        rounds.extend((0..repeat - 1).map(|_| read_round("a.md")));
        rounds.push(("done".into(), vec![], LoopUsage::default()));
        let (outcome, calls) = run_rounds(dir.path(), rounds).await;
        assert_eq!(outcome.response_text, "done");
        assert_eq!(calls, 12);
    }

    /// R41/563 号：计量挂线——开局入表（system+历史+指令）、每轮 usage 入锚、
    /// assistant/工具参数/工具结果追加；表语义 = 当前请求面。
    #[tokio::test]
    async fn meter_folds_request_surface_across_rounds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "hello agent").unwrap();
        let chat = ScriptedChat {
            rounds: vec![
                (
                    "让我看看文件".into(),
                    vec![("call_1".into(), "read".into(), r#"{"path":"a.md"}"#.into())],
                    LoopUsage {
                        input: 100,
                        output: 20,
                        total_tokens: 120,
                        model: Some("m1".into()),
                    },
                ),
                (
                    "文件内容是 hello agent。".into(),
                    vec![],
                    LoopUsage {
                        input: 150,
                        output: 30,
                        total_tokens: 180,
                        model: None,
                    },
                ),
            ],
            calls: Mutex::new(vec![]),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let meter = crate::utils::token_meter::TokenMeter::new(100_000);
        let history = vec![LLMMessage {
            role: LLMRole::User,
            content: "上一轮摘要".into(),
            tool_calls: None,
            tool_call_id: None,
        }];
        let outcome = run_agent_loop(
            &chat,
            &executor,
            "sys",
            history,
            "读一下",
            None,
            None,
            &NoopEvents,
            Some(&meter),
        )
        .await
        .unwrap();
        assert!(!outcome.aborted);
        let snap = meter.measure();
        // 两轮 usage 都入过锚：最后 accepted 的是第二轮（usage.model None 仍成立）。
        assert!(snap.anchor_valid);
        assert!(snap.model.is_none(), "第二轮 model None → 锚点模型面 null");
        // 覆盖率 < 1：锚点（第二轮入账时）未覆盖第一轮已入表面。
        assert!(snap.coverage > 0.0 && snap.coverage < 1.0);
        // 表面节点：system+history+instruction(3) + 轮1(内容+参数串=2) + 工具结果(1)
        // + 轮2(内容=1) = 7。
        assert_eq!(snap.surface_nodes, 7);
        // 启发式总量 = usage 权威累加量的同域估算，非零且窗口内。
        assert!(snap.tokens > 0 && !snap.over_window);
    }
}
