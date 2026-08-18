//! agent 多轮 tool-use 循环（pi-agent loop 核心子集）。
//!
//! 移植自 `packages/core/src/interaction/runtime.ts` 的 loop 骨架：
//! LLM ↔ 工具调用多轮循环（上限 12 轮）、SSE 增量事件回调（draft:delta /
//! tool:start|end）、abort 中途截断、工具执行卡收集。历史回放与完整事件面
//! 的其余部分随 67 号（restore.ts 后半 + 生产工具）。

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
    fn on_tool_end(&self, _id: &str, _tool: &str, _result_text: &str, _details: Option<&Value>, _is_error: bool) {}
}

/// 无操作回调。
pub struct NoopEvents;
impl LoopEvents for NoopEvents {}

/// abort 句柄（65 号注册表的条目）。
pub type AbortHandle = Arc<Mutex<bool>>;

/// 单次工具执行卡（对齐 TS CollectedToolExec：结构化 details 原样携带，
/// 未提供时序列化省略键）。
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
}

/// 循环结果。
pub struct LoopOutcome {
    pub response_text: String,
    pub tool_executions: Vec<LoopToolExecution>,
    pub aborted: bool,
}

/// 单轮 LLM 调用的抽象（测试注入 + AgentRouter 适配）。
#[async_trait::async_trait]
pub trait LoopChat: Send + Sync {
    /// 返回 (content, tool_calls[(id, name, arguments_json)])。
    async fn chat(&self, messages: &[LLMMessage], tools: Option<&Value>) -> Result<(String, Vec<(String, String, String)>), String>;
}

/// 工具执行抽象（80 号：play 聊天工具需要会话/路由上下文，文件工具只需
/// 项目根——trait 让 loop 与具体工具面解耦）。
#[async_trait::async_trait]
pub trait LoopToolExecutor: Send + Sync {
    async fn execute(&self, name: &str, args: &Value) -> crate::interaction::project_tools::ToolResult;
}

/// agent 循环：system + 历史回放 + user 起始，工具调用逐轮执行回填，直至
/// 模型给出无工具的最终文本或达到轮次上限。abort 句柄每轮前轮询。
/// `initial_history`（68 号）：transcript 回放的 summary/对话/boundary 消息，
/// 插在 system 与本轮 user 之间（pi-agent initialState.messages 的等价位置）。
/// `tool_exec`（80 号）：工具执行面（文件工具 / play 聊天工具）。
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
) -> Result<LoopOutcome, String> {
    const MAX_ROUNDS: usize = 12;
    let is_aborted = || {
        abort
            .and_then(|handle| {
                let guard = handle.lock().ok()?;
                Some(*guard)
            })
            .unwrap_or(false)
    };

    let mut messages = vec![
        LLMMessage { role: LLMRole::System, content: system_prompt.to_string(), tool_calls: None, tool_call_id: None },
    ];
    messages.extend(initial_history);
    messages.push(LLMMessage { role: LLMRole::User, content: instruction.to_string(), tool_calls: None, tool_call_id: None });
    let mut executions: Vec<LoopToolExecution> = Vec::new();

    for _round in 0..MAX_ROUNDS {
        if is_aborted() {
            return Ok(LoopOutcome {
                response_text: String::new(),
                tool_executions: executions,
                aborted: true,
            });
        }
        let (content, tool_calls) = chat.chat(&messages, tools).await?;
        if !content.trim().is_empty() {
            events.on_delta(content.trim());
        }
        if tool_calls.is_empty() {
            return Ok(LoopOutcome {
                response_text: content.trim().to_string(),
                tool_executions: executions,
                aborted: false,
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

        for (id, name, arguments_json) in &tool_calls {
            let args: Value = serde_json::from_str(arguments_json).unwrap_or(json!({}));
            events.on_tool_start(id, name, &args);
            let started = crate::interaction::session::utc_now_ms();
            let result = tool_exec.execute(name, &args).await;
            let completed = crate::interaction::session::utc_now_ms();
            let is_error = result.is_error
                || (result.details.is_none()
                    && result.text.starts_with(|c: char| !c.is_ascii_alphanumeric())
                    && result.text.contains("escapes"))
                || result.text.contains("failed:")
                || result.text.starts_with("Unknown tool");
            events.on_tool_end(id, name, &result.text, (!is_error).then_some(result.details.as_ref()).flatten(), is_error);
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
            });
            messages.push(LLMMessage {
                role: LLMRole::Tool,
                content: result.text.chars().take(8000).collect::<String>(),
                tool_calls: None,
                tool_call_id: Some(id.clone()),
            });
        }
    }

    Ok(LoopOutcome {
        response_text: String::new(),
        tool_executions: executions,
        aborted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ScriptedChat {
        rounds: Vec<ScriptedRound>,
        calls: Mutex<Vec<usize>>,
    }

    type ScriptedRound = (String, Vec<(String, String, String)>);

    #[async_trait::async_trait]
    impl LoopChat for ScriptedChat {
        async fn chat(&self, _messages: &[LLMMessage], _tools: Option<&Value>) -> Result<(String, Vec<(String, String, String)>), String> {
            let index = {
                let mut calls = self.calls.lock().unwrap();
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
                ),
                ("文件内容是 hello agent。".into(), vec![]),
            ],
            calls: Mutex::new(vec![]),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let outcome = run_agent_loop(&chat, &executor, "sys", Vec::new(), "读一下", Some(&crate::interaction::project_tools::tools_payload()), None, &NoopEvents)
            .await
            .unwrap();
        assert_eq!(outcome.response_text, "文件内容是 hello agent。");
        assert_eq!(outcome.tool_executions.len(), 1);
        assert_eq!(outcome.tool_executions[0].tool, "read");
        assert_eq!(outcome.tool_executions[0].status, "completed");
        assert!(outcome.tool_executions[0].result.as_deref().unwrap().contains("hello agent"));
    }

    #[tokio::test]
    async fn abort_stops_before_first_round() {
        let dir = tempfile::tempdir().unwrap();
        let chat = ScriptedChat { rounds: vec![("x".into(), vec![])], calls: Mutex::new(vec![]) };
        let abort: AbortHandle = Arc::new(Mutex::new(true));
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let outcome = run_agent_loop(&chat, &executor, "sys", Vec::new(), "hi", None, Some(&abort), &NoopEvents)
            .await
            .unwrap();
        assert!(outcome.aborted);
        assert!(outcome.response_text.is_empty());
        assert_eq!(chat.calls.lock().unwrap().len(), 0, "abort 后未发起 LLM 调用");
    }

    #[tokio::test]
    async fn unknown_tool_is_error_card() {
        let dir = tempfile::tempdir().unwrap();
        let chat = ScriptedChat {
            rounds: vec![
                ("".into(), vec![("call_1".into(), "nope".into(), "{}".into())]),
                ("done".into(), vec![]),
            ],
            calls: Mutex::new(vec![]),
        };
        let executor = crate::interaction::project_tools::ProjectToolExecutor { root: dir.path() };
        let outcome = run_agent_loop(&chat, &executor, "sys", Vec::new(), "hi", None, None, &NoopEvents)
            .await
            .unwrap();
        assert_eq!(outcome.tool_executions[0].status, "error");
        assert!(outcome.tool_executions[0].error.as_deref().unwrap().contains("Unknown tool"));
    }
}
