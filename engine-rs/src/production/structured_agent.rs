//! 结构化 worker 基座（888 号 M3b）：`packages/core/src/agent/worker-agent.ts`
//! `runWorkerAgentTool` 忠实移植——单结果工具环（validate 重试 + supporting
//! 工具 + 预算耗尽码面 + usage 聚合），通用件（M3c 四代理与 M4 复用）。
//!
//! ## 与 TS 的对齐锚（逐字金标准串）
//! - finish-by-calling system 追加文本、no-call 补轮文本、READBACK/ALREADY_
//!   FINALIZED/预算说明 JSON——全部与 worker-agent.ts 逐字共用（本模块
//!   `mod tests` + TS `__tests__/worker-agent.test.ts` 双端锁）。
//! - `tool_choice:"required"` 经 `ChatCompletionParams.extra` 顶层透传
//!   （build_chat_completion_request 的 extra 合并非保留字段）。
//!
//! ## 差异备案
//! TS 的 TypeBox `toolArgumentIssues` 字段级校验由调用方闭包承担（serde typed
//! 反序列化在 validate/execute 内做）——码面 WORKER_SCHEMA_INVALID 同、明细
//! 形态备案不逐字。LLM 传输经 [`ChatTurnFn`] 注入：生产实现包
//! streaming_client（M3c 接线），测试注入 fake（mock 双腿形态对齐）。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use crate::llm::provider::{LLMMessage, LLMRole};

// ── 金标准串（与 TS worker-agent.ts 逐字共用）────────────────────────

/// TS：`Finish by calling ${resultTool.name} exactly once. …`。
pub fn finish_by_calling_text(tool_name: &str) -> String {
    format!("Finish by calling {tool_name} exactly once. Do not print the result as prose or JSON.")
}

/// TS：`You did not call ${resultTool.name}. Call it now …`（no-call 补轮）。
pub fn no_call_prompt(tool_name: &str) -> String {
    format!("You did not call {tool_name}. Call it now with the complete result.")
}

/// TS：result 工具执行成功回喂的固定文本。
pub const STRUCTURED_RESULT_RECEIVED: &str = "Structured result received by the host.";

/// TS：supporting 非空时每轮 systemPrompt 追加的预算说明前缀。
pub const BUDGET_PREAMBLE: &str = "Complete the task within the response budget below. Batch independent supporting calls and leave a later response to read their results and submit the final tool. Do not claim incomplete work is complete.";

/// TS：workerBudget JSON（response=当前轮 1 基，maxResponses/remainingResponses）。
pub fn budget_payload(response: u32, max_responses: u32, final_tool: &str) -> Value {
    serde_json::json!({
        "workerBudget": {
            "response": response,
            "maxResponses": max_responses,
            "remainingResponses": max_responses - response,
            "finalTool": final_tool,
        }
    })
}

// ── 错误与规格 ───────────────────────────────────────────────────────

/// TS `Object.assign(new Error(…), {code, …})` 的 Rust 形态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredToolError {
    pub code: Option<String>,
    pub message: String,
}

impl StructuredToolError {
    pub fn plain(message: impl Into<String>) -> Self {
        Self { code: None, message: message.into() }
    }

    pub fn coded(code: &str, message: impl Into<String>) -> Self {
        Self { code: Some(code.to_string()), message: message.into() }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredAgentError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_tool: Option<String>,
}

/// 结果工具规格（TS `WorkerResultTool` 去 validate/execute 后的数据面）。
#[derive(Debug, Clone)]
pub struct WorkerToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema 形态（与 TS TypeBox 序列化同形——请求 tools 数组元素）。
    pub parameters_schema: Value,
    /// TS `resultTool.maxTurns ?? 3`——模型轮预算。
    pub max_turns: Option<u32>,
}

/// supporting 工具规格 + 执行闭包（TS supportingTools 的 execute 面）。
pub struct SupportingToolSpec {
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
    pub execute: StructuredToolCallback,
}

/// 异步工具回调形态（execute/validate 共用——clippy type_complexity 拆型）。
pub type StructuredToolCallback = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value, StructuredToolError>> + Send>>
        + Send
        + Sync,
>;

/// validate 回调（TS `validate`：typed 校验，Err 即回喂重试）。
pub type ValidateFn = StructuredToolCallback;

/// 一轮 LLM 调用的注入面（生产=streaming_client 包裹；测试=fake）。
pub struct ChatTurnRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [LLMMessage],
    /// tools 数组（result tool 在首位——TS `tools: [tool, ...supporting]` 同序）。
    pub tools: &'a Value,
    /// TS `toolChoice: "required"`（恒 Some("required")——基座环固定；生产适配
    /// 器写入请求顶层字段，测试捕获断言形态）。
    pub tool_choice: Option<&'a str>,
    pub temperature: f64,
    pub max_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct ChatTurnToolCall {
    pub id: String,
    pub name: String,
    /// 原始 JSON 文本（OpenAI arguments 串）。
    pub arguments: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ChatTurnUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ChatTurnResponse {
    pub content: String,
    pub tool_calls: Vec<ChatTurnToolCall>,
    /// OpenAI finish_reason（None=传输未提供；"length" → MODEL_OUTPUT_LIMIT 面）。
    pub finish_reason: Option<String>,
    pub usage: ChatTurnUsage,
}

#[derive(Debug, Clone)]
pub struct ChatTurnError {
    pub message: String,
    /// TS stopReason "length" 分流面（streaming_client 的 output-limit 守卫）。
    pub output_limit: bool,
}

pub type ChatTurnFn = Arc<
    dyn Fn(ChatTurnRequest) -> Pin<Box<dyn Future<Output = Result<ChatTurnResponse, ChatTurnError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, Default)]
pub struct StructuredDefaults {
    pub temperature: f64,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Default)]
pub struct StructuredRunOptions {
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct StructuredRunOutcome {
    pub result: Value,
    pub usage: ChatTurnUsage,
    pub turns: u32,
}

// ── 环 ───────────────────────────────────────────────────────────────

/// TS `runWorkerAgentTool` 等价环。`validate=None` = 提交即定稿。
#[allow(clippy::too_many_arguments)]
pub async fn run_structured_tool(
    chat: ChatTurnFn,
    model: &str,
    messages: &[LLMMessage],
    defaults: StructuredDefaults,
    result_tool: WorkerToolSpec,
    validate: Option<ValidateFn>,
    supporting: Vec<SupportingToolSpec>,
    options: StructuredRunOptions,
) -> Result<StructuredRunOutcome, StructuredAgentError> {
    let max_turns = result_tool.max_turns.unwrap_or(3);
    let temperature = options.temperature.unwrap_or(defaults.temperature);
    let max_tokens = options.max_tokens.unwrap_or(defaults.max_tokens);
    let result_name = result_tool.name.clone();

    // system 合并 + finish-by-calling 追加（TS join("\n\n") 同序）。
    let mut system_parts: Vec<String> = messages
        .iter()
        .filter(|message| message.role == LLMRole::System)
        .map(|message| message.content.clone())
        .collect();
    system_parts.push(finish_by_calling_text(&result_name));
    let base_system = system_parts.join("\n\n");
    // TS：promptMessages 非空守卫（system 不计入）。
    if !messages.iter().any(|message| message.role != LLMRole::System) {
        return Err(StructuredAgentError {
            code: "WORKER_EMPTY_PROMPT".into(),
            message: "Structured Worker Agent requires at least one non-system message".into(),
            attempts: None,
            result_tool: Some(result_name),
        });
    }

    // tools 数组：result tool 首位 + supporting（TS tools: [tool, ...supporting]）。
    let mut tool_specs = vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": result_tool.name,
            "description": result_tool.description,
            "parameters": result_tool.parameters_schema,
        },
    })];
    for tool in &supporting {
        tool_specs.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters_schema,
            },
        }));
    }
    let tools = Value::Array(tool_specs);

    let mut conversation: Vec<LLMMessage> = messages
        .iter()
        .filter(|message| message.role != LLMRole::System)
        .cloned()
        .collect();

    let mut submitted: Option<Value> = None;
    let mut model_turns: u32 = 0;
    let mut last_supporting_turn: Option<u32> = None;
    let mut attempts_exhausted = false;
    let mut last_validation: Option<StructuredToolError> = None;
    let mut no_call_retried = false;
    let mut usage = ChatTurnUsage::default();
    let mut last_finish_reason: Option<String> = None;

    loop {
        if submitted.is_some() {
            break; // TS：submitted → localStopStream（环立即终止）。
        }
        if model_turns >= max_turns {
            attempts_exhausted = true;
            break; // TS：streamFn 预算闸 + resultAttemptsExhausted。
        }
        model_turns += 1;

        // TS：supporting 非空时每轮 system 追加预算说明（workerBudget 递增）。
        let system_content = if supporting.is_empty() {
            base_system.clone()
        } else {
            format!(
                "{base_system}\n\n{BUDGET_PREAMBLE}\n\n{}",
                budget_payload(model_turns, max_turns, &result_name)
            )
        };
        let mut request_messages = vec![LLMMessage {
            role: LLMRole::System,
            content: system_content,
            tool_calls: None,
            tool_call_id: None,
        }];
        request_messages.extend(conversation.iter().cloned());

        let response = chat(ChatTurnRequest {
            model,
            messages: &request_messages,
            tools: &tools,
            tool_choice: Some("required"),
            temperature,
            max_tokens,
        })
        .await
        .map_err(|error: ChatTurnError| {
            if error.output_limit {
                StructuredAgentError {
                    code: "MODEL_OUTPUT_LIMIT".into(),
                    message: "Worker output reached the configured model output limit".into(),
                    attempts: Some(model_turns),
                    result_tool: Some(result_name.clone()),
                }
            } else {
                StructuredAgentError {
                    code: "WORKER_MODEL_ERROR".into(),
                    message: error.message,
                    attempts: Some(model_turns),
                    result_tool: Some(result_name.clone()),
                }
            }
        })?;
        usage.prompt_tokens += response.usage.prompt_tokens;
        usage.completion_tokens += response.usage.completion_tokens;
        usage.total_tokens += response.usage.total_tokens;
        last_finish_reason = response.finish_reason.clone();

        if response.tool_calls.is_empty() {
            // 模型纯文本未调用：回喂 assistant 文本 + 补轮 prompt（TS agent.prompt
            // 结束后的一次性补召）。
            conversation.push(LLMMessage {
                role: LLMRole::Assistant,
                content: response.content.clone(),
                tool_calls: None,
                tool_call_id: None,
            });
            if !no_call_retried {
                no_call_retried = true;
                conversation.push(LLMMessage {
                    role: LLMRole::User,
                    content: no_call_prompt(&result_name),
                    tool_calls: None,
                    tool_call_id: None,
                });
                continue;
            }
            // 补轮后仍未调用——按 TS 收尾分流（finish_reason 面优先）。
            break;
        }

        // assistant 轮回喂（OpenAI 形态：tool_calls 数组 + 逐个 tool 结果）。
        let assistant_tool_calls: Vec<Value> = response
            .tool_calls
            .iter()
            .map(|call| {
                serde_json::json!({
                    "id": call.id,
                    "type": "function",
                    "function": { "name": call.name, "arguments": call.arguments },
                })
            })
            .collect();
        let assistant_message = LLMMessage {
            role: LLMRole::Assistant,
            content: response.content.clone(),
            tool_calls: Some(Value::Array(assistant_tool_calls)),
            tool_call_id: None,
        };
        let mut tool_results: Vec<(String, String, bool)> = Vec::new(); // (id, content, is_error)

        for call in &response.tool_calls {
            let arguments: Value = match serde_json::from_str(&call.arguments) {
                Ok(args) => args,
                Err(error) => {
                    // TS prepareArguments：schema 面失败 → WORKER_SCHEMA_INVALID 回喂。
                    let failure = StructuredToolError::coded(
                        "WORKER_SCHEMA_INVALID",
                        format!(
                            "{{\"code\":\"WORKER_SCHEMA_INVALID\",\"resultTool\":\"{call_name}\",\"issues\":\"{error}\"}}",
                            call_name = call.name
                        ),
                    );
                    if call.name == result_name {
                        last_validation = Some(failure.clone());
                    }
                    tool_results.push((call.id.clone(), failure.message, true));
                    continue;
                }
            };
            if call.name == result_name {
                // TS execute：已定稿后后续 result 调用拒绝。
                if submitted.is_some() {
                    let error = StructuredToolError::coded(
                        "WORKER_RESULT_ALREADY_FINALIZED",
                        "The final result is already recorded; later tool calls cannot change it.",
                    );
                    tool_results.push((call.id.clone(), error.message, true));
                    continue;
                }
                // TS READBACK 规则：本轮已有 supporting 调用 → 禁止同轮定稿。
                if last_supporting_turn == Some(model_turns) {
                    let error = StructuredToolError::coded(
                        "WORKER_RESULT_READBACK_REQUIRED",
                        "Read the supporting tool results before finalizing in a later response. Correct or omit unsupported findings, then submit the final result.",
                    );
                    last_validation = Some(error.clone());
                    tool_results.push((call.id.clone(), error.message, true));
                    continue;
                }
                match validate.as_ref() {
                    Some(validate) => {
                        match validate(arguments).await {
                            Ok(value) => {
                                submitted = Some(value);
                                tool_results.push((
                                    call.id.clone(),
                                    STRUCTURED_RESULT_RECEIVED.to_string(),
                                    false,
                                ));
                            }
                            Err(error) => {
                                last_validation = Some(error.clone());
                                tool_results.push((call.id.clone(), error.message, true));
                            }
                        }
                    }
                    None => {
                        submitted = Some(arguments);
                        tool_results.push((
                            call.id.clone(),
                            STRUCTURED_RESULT_RECEIVED.to_string(),
                            false,
                        ));
                    }
                }
            } else if let Some(index) = supporting.iter().position(|tool| tool.name == call.name) {
                // TS supporting execute：已定稿后拒绝；成功后记录 lastSupportingTurn。
                if submitted.is_some() {
                    let error = StructuredToolError::coded(
                        "WORKER_RESULT_ALREADY_FINALIZED",
                        "The final result is already recorded; later tool calls cannot change it.",
                    );
                    tool_results.push((call.id.clone(), error.message, true));
                    continue;
                }
                let tool = &supporting[index];
                match (tool.execute)(arguments).await {
                    Ok(result) => {
                        last_supporting_turn = Some(model_turns);
                        tool_results.push((
                            call.id.clone(),
                            serde_json::to_string(&result).unwrap_or_else(|_| "null".into()),
                            false,
                        ));
                    }
                    Err(error) => {
                        tool_results.push((call.id.clone(), error.message, true));
                    }
                }
            } else {
                // 未知工具名（模型幻觉）——schema 面失败同族回喂。
                let failure = StructuredToolError::coded(
                    "WORKER_SCHEMA_INVALID",
                    format!("Unknown tool: {}", call.name),
                );
                tool_results.push((call.id.clone(), failure.message, true));
            }
        }
        conversation.push(assistant_message);
        for (id, content, _is_error) in &tool_results {
            conversation.push(LLMMessage {
                role: LLMRole::Tool,
                content: content.clone(),
                tool_calls: None,
                tool_call_id: Some(id.clone()),
            });
        }
    }

    if submitted.is_none() {
        // TS 收尾分流：exhausted → WORKER_RESULT_INVALID(lastValidation)；length →
        // MODEL_OUTPUT_LIMIT；其余 → WORKER_RESULT_MISSING。
        if attempts_exhausted {
            return Err(StructuredAgentError {
                code: last_validation
                    .as_ref()
                    .and_then(|error| error.code.clone())
                    .unwrap_or_else(|| "WORKER_RESULT_INVALID".into()),
                message: last_validation
                    .as_ref()
                    .map(|error| error.message.clone())
                    .unwrap_or_else(|| {
                        "Structured result remained invalid after bounded correction attempts"
                            .into()
                    }),
                attempts: Some(model_turns),
                result_tool: Some(result_name),
            });
        }
        if last_finish_reason.as_deref() == Some("length") {
            return Err(StructuredAgentError {
                code: "MODEL_OUTPUT_LIMIT".into(),
                message: "Worker output reached the configured model output limit".into(),
                attempts: Some(model_turns),
                result_tool: Some(result_name),
            });
        }
        return Err(StructuredAgentError {
            code: "WORKER_RESULT_MISSING".into(),
            message: format!("Worker Agent completed without calling {result_name}"),
            attempts: Some(model_turns),
            result_tool: Some(result_name),
        });
    }

    if usage.total_tokens > 0 {
        // TS onUsage 面由调用方经 outcome.usage 消费（等价）。
    }
    Ok(StructuredRunOutcome {
        result: submitted.unwrap_or(Value::Null),
        usage,
        turns: model_turns,
    })
}

// ── 生产适配器（AgentRouter/streaming_client → ChatTurnFn）───────────

/// `ChatTurnError` 的生产映射（StreamError 分类：OutputLimit→output_limit，
/// 其余→WORKER_MODEL_ERROR 面）。
pub fn chat_turn_error_from_stream(error: crate::llm::streaming_client::StreamError) -> ChatTurnError {
    let output_limit = matches!(
        error,
        crate::llm::streaming_client::StreamError::OutputLimit(_)
    );
    ChatTurnError { message: error.to_string(), output_limit }
}

/// 生产 ChatTurnFn：AgentRouter 按 `agent` 角色路由（resolve→client 缓存），
/// streaming_client 非流式收集（环轮次短；tool_choice 走 extra 顶层透传，
/// stream=false 对齐 TS guardedPiNonStreaming 分支的确定性收集）。
pub fn router_chat_turn_fn(
    router: Arc<crate::llm::agent_router::AgentRouter>,
    agent: &'static str,
) -> ChatTurnFn {
    Arc::new(move |request: ChatTurnRequest| {
        let router = Arc::clone(&router);
        // 借用面克隆出所有权（Box<dyn Future> 无生命周期泛型——888 号同款）。
        let model = request.model.to_string();
        let messages = request.messages.to_vec();
        let tools = request.tools.clone();
        let tool_choice = request.tool_choice.map(str::to_string);
        let temperature = request.temperature;
        let max_tokens = request.max_tokens;
        Box::pin(async move {
            // client 取该角色路由端点（base_url/api_key 面）；model 以
            // request.model 为准（调用方经 defaults 装配已解析的模型——TS
            // workerModel(client, modelId) 的 modelId 单参换模型同构）。
            let endpoint = router.resolve(agent);
            let client = router.client_for_public(&endpoint).await;
            let mut extra = std::collections::HashMap::new();
            if let Some(choice) = &tool_choice {
                extra.insert("tool_choice".to_string(), Value::String(choice.clone()));
            }
            let llm_messages: Vec<crate::llm::provider::LLMMessage> = messages;
            let params = crate::llm::streaming_client::ChatCompletionParams {
                model: &model,
                messages: &llm_messages,
                temperature,
                max_tokens,
                stream: false,
                api_format: router.api_format(),
                extra: Some(&extra),
                tools: Some(&tools),
                images: None,
                progress: None,
                deadline: crate::llm::streaming_client::StreamDeadlineSpec::PIPELINE,
                trajectory: None,
            };
            let completion = client
                .stream_chat(&params)
                .await
                .map_err(chat_turn_error_from_stream)?;
            Ok(ChatTurnResponse {
                content: completion.content,
                tool_calls: completion
                    .tool_calls
                    .into_iter()
                    .map(|call| ChatTurnToolCall {
                        id: call.id,
                        name: call.name,
                        arguments: call.arguments,
                    })
                    .collect(),
                finish_reason: completion.finish_reason,
                usage: ChatTurnUsage {
                    prompt_tokens: completion.prompt_tokens.unwrap_or(0),
                    completion_tokens: completion.completion_tokens.unwrap_or(0),
                    total_tokens: completion.total_tokens.unwrap_or(0),
                },
            })
        })
    })
}

// ── 测试（mock 注入：形态金标准 + 变异必红）─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const RESULT_TOOL: &str = "submit_state";
    const RESULT_SCHEMA: &str =
        r#"{"type":"object","properties":{"label":{"type":"string"},"status":{"type":"string"}},"required":["label","status"],"additionalProperties":false}"#;

    fn llm(role: LLMRole, content: &str) -> LLMMessage {
        LLMMessage { role, content: content.into(), tool_calls: None, tool_call_id: None }
    }

    fn call(id: &str, name: &str, arguments: &str) -> ChatTurnToolCall {
        ChatTurnToolCall { id: id.into(), name: name.into(), arguments: arguments.into() }
    }

    fn response(tool_calls: Vec<ChatTurnToolCall>, tokens: u64) -> ChatTurnResponse {
        let finish_reason = tool_calls.is_empty().then(|| "stop".to_string());
        ChatTurnResponse {
            content: String::new(),
            tool_calls,
            finish_reason,
            usage: ChatTurnUsage {
                prompt_tokens: tokens,
                completion_tokens: tokens,
                total_tokens: tokens * 2,
            },
        }
    }

    fn text_response(content: &str) -> ChatTurnResponse {
        ChatTurnResponse {
            content: content.into(),
            tool_calls: Vec::new(),
            finish_reason: Some("stop".to_string()),
            usage: ChatTurnUsage::default(),
        }
    }

    /// 脚本化 fake：逐轮弹出响应（按脚本顺序消费）+ 捕获请求（形态断言面）。
    struct FakeChat {
        script: Mutex<std::collections::VecDeque<ChatTurnResponse>>,
        errors: Mutex<Vec<ChatTurnError>>,
        captured: Mutex<Vec<CapturedRequest>>,
    }

    #[derive(Clone)]
    struct CapturedRequest {
        system_content: String,
        tools: Value,
        tool_choice: Option<String>,
        user_contents: Vec<String>,
    }

    impl FakeChat {
        fn new(script: Vec<ChatTurnResponse>) -> Arc<Self> {
            Arc::new(Self {
                script: Mutex::new(script.into()),
                errors: Mutex::new(Vec::new()),
                captured: Mutex::new(Vec::new()),
            })
        }

        fn chat_fn(self: &Arc<Self>) -> ChatTurnFn {
            let me = Arc::clone(self);
            Arc::new(move |request: ChatTurnRequest| {
                let me = Arc::clone(&me);
                // 借用面在 async 块前克隆出所有权（Box<dyn Future> 无生命周期泛型）。
                let system_content = request
                    .messages
                    .first()
                    .map(|message| message.content.clone())
                    .unwrap_or_default();
                let tools = request.tools.clone();
                let tool_choice = request.tool_choice.map(str::to_string);
                let user_contents: Vec<String> = request
                    .messages
                    .iter()
                    .filter(|message| message.role == LLMRole::User)
                    .map(|message| message.content.clone())
                    .collect();
                Box::pin(async move {
                    me.captured.lock().unwrap().push(CapturedRequest {
                        system_content,
                        tools,
                        tool_choice,
                        user_contents,
                    });
                    if let Some(error) = me.errors.lock().unwrap().pop() {
                        return Err(error);
                    }
                    let next = me.script.lock().unwrap().pop_front();
                    match next {
                        Some(response) => Ok(response),
                        // 脚本耗尽 = 模型继续空转（应已被预算闸拦住）。
                        None => Ok(text_response("")),
                    }
                })
            })
        }

        fn captured(&self) -> Vec<CapturedRequest> {
            self.captured.lock().unwrap().clone()
        }
    }

    fn spec(max_turns: Option<u32>) -> WorkerToolSpec {
        WorkerToolSpec {
            name: RESULT_TOOL.into(),
            description: "Submit role state.".into(),
            parameters_schema: serde_json::from_str(RESULT_SCHEMA).unwrap(),
            max_turns,
        }
    }

    fn defaults() -> StructuredDefaults {
        StructuredDefaults { temperature: 0.7, max_tokens: 4096 }
    }

    fn base_messages() -> Vec<LLMMessage> {
        vec![llm(LLMRole::System, "你是角色状态记录员。"), llm(LLMRole::User, "登记当前角色状态")]
    }

    #[tokio::test]
    async fn first_turn_submission_matches_request_shape_golden() {
        // 形态金标准：system 追加 finish-by-calling（逐字）+ tools 首位 result
        // schema + tool_choice required（TS worker-agent.test.ts toolChoice 断言
        // 的 Rust 对应面）。
        let fake = FakeChat::new(vec![response(
            vec![call("c1", RESULT_TOOL, r#"{"label":"母亲","status":"等待退烧药"}"#)],
            10,
        )]);
        let outcome = run_structured_tool(
            fake.chat_fn(),
            "deepseek-v4-flash",
            &base_messages(),
            defaults(),
            spec(None),
            None,
            Vec::new(),
            StructuredRunOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            outcome.result,
            serde_json::json!({ "label": "母亲", "status": "等待退烧药" })
        );
        assert_eq!(outcome.turns, 1);
        let captured = fake.captured();
        assert_eq!(captured.len(), 1);
        assert!(
            captured[0]
                .system_content
                .contains("Finish by calling submit_state exactly once. Do not print the result as prose or JSON."),
            "finish-by-calling 逐字追加：{}",
            captured[0].system_content
        );
        assert!(captured[0].system_content.starts_with("你是角色状态记录员。\n\n"));
        // TS tools: [tool, ...supporting]——单工具时首位即 result schema。
        let tools = &captured[0].tools;
        assert_eq!(tools[0]["function"]["name"], RESULT_TOOL);
        assert_eq!(
            tools[0]["function"]["parameters"]["properties"]["label"]["type"],
            "string"
        );
        assert_eq!(captured[0].tool_choice.as_deref(), Some("required"));
        // usage 聚合（单轮 tokens=10/10/20）
        assert_eq!(outcome.usage.total_tokens, 20);
    }

    #[tokio::test]
    async fn validate_always_red_exhausts_budget_and_surfaces_last_code() {
        // 变异证明（888 号验收核心）：validate 恒红 → 预算耗尽必红，
        // code 透传最后一次校验错误、attempts = max_turns。
        let fake = FakeChat::new((1..=5)
            .map(|index| {
                response(
                    vec![call(
                        format!("c{index}").leak() as &'static str,
                        RESULT_TOOL,
                        r#"{"label":"x","status":"y"}"#,
                    )],
                    1,
                )
            })
            .collect());
        let validate: ValidateFn = Arc::new(|_value: Value| {
            Box::pin(async {
                Err(StructuredToolError::coded("SHORT_CHAPTER_TOO_LONG", "超长"))
            })
        });
        let error = run_structured_tool(
            fake.chat_fn(),
            "test-model",
            &base_messages(),
            defaults(),
            spec(None), // max_turns 默认 3
            Some(validate),
            Vec::new(),
            StructuredRunOptions::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "SHORT_CHAPTER_TOO_LONG", "末次校验码透传：{error:?}");
        assert_eq!(error.attempts, Some(3));
        assert_eq!(error.result_tool.as_deref(), Some(RESULT_TOOL));
        assert_eq!(error.message, "超长");
        let captured = fake.captured();
        assert_eq!(captured.len(), 3, "预算闸在第三轮后停召");
    }

    #[tokio::test]
    async fn readback_rule_defers_same_turn_finalization() {
        // TS READBACK：supporting 调用后的同轮 result 提交 → READBACK_REQUIRED
        // 回喂；下一轮（读回结果后）提交成功。
        let fake = FakeChat::new(vec![
            response(
                vec![
                    call("s1", "lookup_status", r#"{"query":"体温"}"#),
                    call("c1", RESULT_TOOL, r#"{"label":"母亲","status":"等待"}"#),
                ],
                1,
            ),
            response(vec![call("c2", RESULT_TOOL, r#"{"label":"母亲","status":"等待退烧药"}"#)], 2),
        ]);
        let supporting = vec![SupportingToolSpec {
            name: "lookup_status".into(),
            description: "查询当前状态。".into(),
            parameters_schema: serde_json::json!({ "type": "object", "properties": {} }),
            execute: Arc::new(|_args: Value| -> Pin<Box<dyn Future<Output = Result<Value, StructuredToolError>> + Send>> {
                Box::pin(async { Ok(serde_json::json!({ "status": "等待退烧药" })) })
            }),
        }];
        let outcome = run_structured_tool(
            fake.chat_fn(),
            "test-model",
            &base_messages(),
            defaults(),
            spec(None),
            None,
            supporting,
            StructuredRunOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.result["status"], "等待退烧药");
        assert_eq!(outcome.turns, 2, "READBACK 强制下一轮");
        // 第二轮 system 携带预算说明（supporting 非空 → workerBudget 递增）。
        let captured = fake.captured();
        assert!(
            captured[0].system_content.contains(BUDGET_PREAMBLE)
                && captured[0].system_content.contains("\"response\":1"),
            "轮 1 预算说明：{}",
            captured[0].system_content
        );
        assert!(captured[1].system_content.contains("\"response\":2"));
    }

    #[tokio::test]
    async fn duplicate_result_call_is_rejected_after_finalization() {
        // TS：已定稿后后续 result 调用 → ALREADY_FINALIZED；提交不受污染。
        let fake = FakeChat::new(vec![response(
            vec![
                call("c1", RESULT_TOOL, r#"{"label":"母亲","status":"终稿"}"#),
                call("c2", RESULT_TOOL, r#"{"label":"篡改","status":"无效"}"#),
            ],
            5,
        )]);
        let outcome = run_structured_tool(
            fake.chat_fn(),
            "test-model",
            &base_messages(),
            defaults(),
            spec(None),
            None,
            Vec::new(),
            StructuredRunOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.result["label"], "母亲", "定稿槽不被后续调用污染");
        assert_eq!(outcome.turns, 1);
    }

    #[tokio::test]
    async fn no_call_first_turn_prompts_once_then_accepts() {
        // TS：未调用 result tool → 一次性补轮 prompt（金标准文本进 user 消息）。
        let fake = FakeChat::new(vec![
            text_response("我先想想……"),
            response(vec![call("c1", RESULT_TOOL, r#"{"label":"母亲","status":"等待退烧药"}"#)], 3),
        ]);
        let outcome = run_structured_tool(
            fake.chat_fn(),
            "test-model",
            &base_messages(),
            defaults(),
            spec(None),
            None,
            Vec::new(),
            StructuredRunOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.result["status"], "等待退烧药");
        let captured = fake.captured();
        assert!(captured[1]
            .user_contents
            .iter()
            .any(|content| content == &no_call_prompt(RESULT_TOOL)));
    }

    #[tokio::test]
    async fn output_limit_error_maps_to_model_output_limit_code() {
        let fake = FakeChat::new(Vec::new());
        fake.errors.lock().unwrap().push(ChatTurnError {
            message: "model reached the output limit (length)".into(),
            output_limit: true,
        });
        let error = run_structured_tool(
            fake.chat_fn(),
            "test-model",
            &base_messages(),
            defaults(),
            spec(None),
            None,
            Vec::new(),
            StructuredRunOptions::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "MODEL_OUTPUT_LIMIT");
    }

    #[tokio::test]
    async fn system_only_prompt_is_rejected() {
        let fake = FakeChat::new(Vec::new());
        let error = run_structured_tool(
            fake.chat_fn(),
            "test-model",
            &[llm(LLMRole::System, "只有系统消息")],
            defaults(),
            spec(None),
            None,
            Vec::new(),
            StructuredRunOptions::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "WORKER_EMPTY_PROMPT");
    }

    #[tokio::test]
    async fn schema_invalid_arguments_feed_back_as_tool_result() {
        // 参数非法 JSON → WORKER_SCHEMA_INVALID 回喂 → 下一轮合法提交成功。
        let fake = FakeChat::new(vec![
            response(vec![call("c1", RESULT_TOOL, "{broken json")], 1),
            response(vec![call("c2", RESULT_TOOL, r#"{"label":"母亲","status":"等待退烧药"}"#)], 2),
        ]);
        let outcome = run_structured_tool(
            fake.chat_fn(),
            "test-model",
            &base_messages(),
            defaults(),
            spec(None),
            None,
            Vec::new(),
            StructuredRunOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.turns, 2);
        assert_eq!(outcome.result["status"], "等待退烧药");
    }
}
