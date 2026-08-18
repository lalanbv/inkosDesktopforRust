//! OpenAI 兼容流式聊天客户端。
//!
//! 把 reqwest HTTP 拉取接到 [`super::sse_parser`]：构造 chat completions 请求体 → POST
//! （streaming）→ 把响应字节流喂给 SseStreamParser → 产出 SseEvent 流。
//!
//! ## 当前范围
//! - [`build_chat_completion_request`]：构造 OpenAI chat completions 请求体（纯，可测）
//! - [`StreamingChatClient`]：reqwest 客户端 + stream_chat 方法（返回 SseEvent 流）
//!
//! ## 待扩展
//! responses API / anthropic-messages 格式 / 工具调用请求体 / 重试退避（withTransientLLMRetry）。
//! 这些在 SSE 解析器 + 请求构造之上叠加。

use super::provider::{LLMMessage, LLMRole};
use super::providers::TransportApiFormat;
use super::sse_parser::SseEvent;
use super::sse_parser::SseStreamParser;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// chat completions 请求参数。
#[derive(Debug, Clone)]
pub struct ChatCompletionParams<'a> {
    pub model: &'a str,
    pub messages: &'a [LLMMessage],
    pub temperature: f64,
    pub max_tokens: u32,
    pub stream: bool,
    /// 传输协议（106 号）：Chat → POST /chat/completions；Responses →
    /// POST /responses（input 数组 + instructions + 终态事件守卫）。
    pub api_format: TransportApiFormat,
    pub extra: Option<&'a HashMap<String, serde_json::Value>>,
    /// OpenAI tools 数组（raw JSON schema 透传；None = 不带工具）。
    pub tools: Option<&'a serde_json::Value>,
    /// 多模态图片（95 号）：注入**最后一条 user 消息**为 OpenAI vision
    /// content 数组（`[{type:"text"},{type:"image_url"}…]`）。TS 侧
    /// `agent.prompt(message, images)` 的当轮 prompt 图——历史轮次保留。
    /// （responses 传输不注入——TS buildResponsesInput 仅 input_text。）
    pub images: Option<&'a [ChatImage]>,
}

/// 多模态图片内容（base64 + mime；对应 TS `ImageContent`）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChatImage {
    pub data: String,
    pub mime_type: String,
}

/// 构造 OpenAI chat completions 请求体（pure，可单测）。对齐 TS provider 的请求形状。
///
/// 保留字段（max_tokens/temperature/model/messages/stream）不被 extra 覆盖（对齐 stripReservedKeys）。
pub fn build_chat_completion_request(params: &ChatCompletionParams) -> serde_json::Value {
    let images = params.images;
    let images_target = images.is_some_and(|list| !list.is_empty());
    let last_user_index = params
        .messages
        .iter()
        .rposition(|probe| probe.role == LLMRole::User);
    let messages: Vec<serde_json::Value> = params
        .messages
        .iter()
        .enumerate()
        .map(|(index, m)| {
            let role = match m.role {
                LLMRole::System => "system",
                LLMRole::User => "user",
                LLMRole::Assistant => "assistant",
                LLMRole::Tool => "tool",
            };
            {
                let mut obj = serde_json::Map::new();
                obj.insert("role".into(), serde_json::json!(role));
                // 多模态：最后一条 user 消息带图片时输出 vision content 数组。
                let is_last_user =
                    images_target && m.role == LLMRole::User && last_user_index == Some(index);
                if is_last_user {
                    let mut parts = vec![serde_json::json!({ "type": "text", "text": m.content })];
                    for image in images.unwrap() {
                        parts.push(serde_json::json!({
                            "type": "image_url",
                            "image_url": { "url": format!("data:{};base64,{}", image.mime_type, image.data) },
                        }));
                    }
                    obj.insert("content".into(), serde_json::Value::Array(parts));
                } else {
                    obj.insert("content".into(), serde_json::json!(m.content));
                }
                if let Some(tool_calls) = &m.tool_calls {
                    obj.insert("tool_calls".into(), tool_calls.clone());
                }
                if let Some(tool_call_id) = &m.tool_call_id {
                    obj.insert("tool_call_id".into(), serde_json::json!(tool_call_id));
                    // OpenAI 协议要求 tool 角色消息带工具名
                    let _ = tool_call_id;
                }
                serde_json::Value::Object(obj)
            }
        })
        .collect();

    let reserved: &[&str] = &["max_tokens", "temperature", "model", "messages", "stream", "tools"];
    let mut body = serde_json::json!({
        "model": params.model,
        "messages": messages,
        "temperature": params.temperature,
        "max_tokens": params.max_tokens,
        "stream": params.stream,
    });
    if let Some(tools) = params.tools {
        body.as_object_mut()
            .expect("body 是对象")
            .insert("tools".to_string(), tools.clone());
    }
    if let Some(extra) = params.extra {
        let obj = body.as_object_mut().expect("body 是对象");
        for (k, v) in extra {
            if !reserved.contains(&k.as_str()) {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    body
}

/// 流式聊天客户端（OpenAI 兼容）。
pub struct StreamingChatClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    extra_headers: HashMap<String, String>,
}

/// 一次流式响应（收集后的完整结果）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamedCompletion {
    pub content: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub done: bool,
    /// 聚合后的工具调用（按 index 合并 name/arguments 片段）。
    pub tool_calls: Vec<StreamedToolCall>,
}

/// 聚合后的单条工具调用。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreamedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 聚合 tool_calls 增量（pure）：按 index 合并 id/name/arguments 片段。
type ToolCallDeltas = Vec<(u32, Option<String>, Option<String>, Option<String>)>;

pub fn aggregate_tool_calls(deltas: &ToolCallDeltas) -> Vec<StreamedToolCall> {
    let mut order: Vec<u32> = Vec::new();
    let mut by_index: std::collections::HashMap<u32, (String, String, String)> =
        std::collections::HashMap::new();
    for (index, id, name, arguments) in deltas {
        let entry = by_index.entry(*index).or_insert_with(|| {
            order.push(*index);
            (String::new(), String::new(), String::new())
        });
        if let Some(id) = id {
            if !id.is_empty() {
                entry.0 = id.clone();
            }
        }
        if let Some(name) = name {
            entry.1.push_str(name);
        }
        if let Some(arguments) = arguments {
            entry.2.push_str(arguments);
        }
    }
    order.sort();
    order
        .into_iter()
        .filter_map(|index| {
            let (id, name, arguments) = by_index.remove(&index)?;
            (!id.is_empty() && !name.is_empty()).then_some(StreamedToolCall { id, name, arguments })
        })
        .collect()
}

#[derive(Debug)]
pub enum StreamError {
    Http(reqwest::Error),
    BadStatus(u16, String),
    /// 协议层失败（响应体可解析 HTTP 但内容不合协议——responses 空响应/
    /// 缺终态事件等；TS wrapLLMError 族的 Rust 等价锚）。
    Protocol(String),
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamError::Http(e) => write!(f, "HTTP 错误: {e}"),
            StreamError::BadStatus(code, body) => write!(f, "HTTP {code}: {body}"),
            StreamError::Protocol(message) => write!(f, "{message}"),
        }
    }
}
impl std::error::Error for StreamError {}
impl From<reqwest::Error> for StreamError {
    fn from(e: reqwest::Error) -> Self {
        StreamError::Http(e)
    }
}

impl StreamingChatClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, extra_headers: HashMap<String, String>) -> Self {
        StreamingChatClient {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            extra_headers,
        }
    }

    /// 流式 chat completion：按 `params.api_format` 分流——Chat → POST
    /// /chat/completions（OpenAI chat 协议）；Responses → POST /responses
    /// （106 号，TS chatCompletionVia responses 分支逐字语义）。两路均收集
    /// 完整响应（StreamedCompletion）。
    pub async fn stream_chat(&self, params: &ChatCompletionParams<'_>) -> Result<StreamedCompletion, StreamError> {
        match params.api_format {
            TransportApiFormat::Responses => self.responses_completion(params).await,
            TransportApiFormat::Chat => self.chat_completion(params).await,
        }
    }

    /// chat completions 传输（原有主路径）。
    async fn chat_completion(&self, params: &ChatCompletionParams<'_>) -> Result<StreamedCompletion, StreamError> {
        let body = build_chat_completion_request(params);
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .json(&body);
        for (k, v) in &self.extra_headers {
            req = req.header(k, v);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(StreamError::BadStatus(status.as_u16(), text));
        }
        // 非流式（108 号 stream 维度）：整体 JSON——choices[0].message.content +
        // message.tool_calls + usage（TS chatCompletion 非流式同构）。
        if !params.stream {
            let raw = resp.text().await?;
            let json: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|e| StreamError::Protocol(format!("invalid JSON response: {e}")))?;
            let message = json
                .pointer("/choices/0/message")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let content = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let tool_calls: Vec<StreamedToolCall> = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| {
                            let id = call.get("id").and_then(Value::as_str)?;
                            let name = call.pointer("/function/name").and_then(Value::as_str)?;
                            let arguments =
                                call.pointer("/function/arguments").and_then(Value::as_str)?;
                            Some(StreamedToolCall {
                                id: id.to_string(),
                                name: name.to_string(),
                                arguments: arguments.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            return Ok(StreamedCompletion {
                content,
                prompt_tokens: json.pointer("/usage/prompt_tokens").and_then(Value::as_u64),
                completion_tokens: json.pointer("/usage/completion_tokens").and_then(Value::as_u64),
                total_tokens: json.pointer("/usage/total_tokens").and_then(Value::as_u64),
                done: true,
                tool_calls,
            });
        }
        // 流式：按字节块喂给 sse_parser，累积 content
        let mut parser = SseStreamParser::new();
        let mut content = String::new();
        let mut prompt_tokens = None;
        let mut completion_tokens = None;
        let mut total_tokens = None;
        let mut done = false;
        let mut tool_call_deltas: ToolCallDeltas = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            for ev in parser.push(&text) {
                match ev {
                    SseEvent::Delta(s) => content.push_str(&s),
                    SseEvent::Usage { prompt_tokens: p, completion_tokens: c, total_tokens: t } => {
                        prompt_tokens = p;
                        completion_tokens = c;
                        total_tokens = t;
                    }
                    SseEvent::Done => done = true,
                    SseEvent::ToolCallDelta { index, id, name, arguments } => {
                        tool_call_deltas.push((index, id, name, arguments));
                    }
                }
            }
        }
        for ev in parser.finish() {
            match ev {
                SseEvent::Delta(s) => content.push_str(&s),
                SseEvent::Usage { prompt_tokens: p, completion_tokens: c, total_tokens: t } => {
                    prompt_tokens = p;
                    completion_tokens = c;
                    total_tokens = t;
                }
                SseEvent::Done => done = true,
                _ => {}
            }
        }
        let tool_calls = aggregate_tool_calls(&tool_call_deltas);
        Ok(StreamedCompletion { content, prompt_tokens, completion_tokens, total_tokens, done, tool_calls })
    }

    /// responses 传输（106 号）：POST /responses——非流式整体解析 + 流式
    /// 事件解析（`response.output_text.delta` 增量 / `response.completed|
    /// incomplete` 终态守卫），TS chatCompletionVia responses 分支逐字语义。
    async fn responses_completion(&self, params: &ChatCompletionParams<'_>) -> Result<StreamedCompletion, StreamError> {
        let body = build_responses_request(params);
        let url = format!("{}/responses", self.base_url.trim_end_matches('/'));
        let mut req = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .json(&body);
        for (k, v) in &self.extra_headers {
            req = req.header(k, v);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(StreamError::BadStatus(status.as_u16(), text));
        }
        if !params.stream {
            let raw = resp.text().await?;
            let json: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|e| StreamError::Protocol(format!("invalid JSON response: {e}")))?;
            let content = extract_responses_content(&json);
            if content.is_empty() {
                return Err(StreamError::Protocol("LLM returned empty response".to_string()));
            }
            return Ok(StreamedCompletion {
                content,
                prompt_tokens: json.pointer("/usage/input_tokens").and_then(Value::as_u64),
                completion_tokens: json.pointer("/usage/output_tokens").and_then(Value::as_u64),
                total_tokens: json.pointer("/usage/total_tokens").and_then(Value::as_u64),
                done: true,
                tool_calls: Vec::new(),
            });
        }
        // 流式：收集原始字节后统一抽 data: 事件（stream_chat 本就聚合语义）。
        let mut stream = resp.bytes_stream();
        let mut raw = Vec::<u8>::new();
        while let Some(chunk) = stream.next().await {
            raw.extend_from_slice(&chunk?);
        }
        let text = String::from_utf8_lossy(&raw);
        let mut content = String::new();
        let mut prompt_tokens = None;
        let mut completion_tokens = None;
        let mut total_tokens = None;
        let mut saw_terminal = false;
        for data in sse_data_events(&text) {
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) else {
                continue;
            };
            let event_type = json.get("type").and_then(Value::as_str).unwrap_or_default();
            if event_type == "response.output_text.delta" {
                if let Some(delta) = json.get("delta").and_then(Value::as_str) {
                    content.push_str(delta);
                }
            }
            if event_type == "response.completed" || event_type == "response.incomplete" {
                saw_terminal = true;
                let response = json.get("response").cloned().unwrap_or(serde_json::Value::Null);
                prompt_tokens = response.pointer("/usage/input_tokens").and_then(Value::as_u64);
                completion_tokens = response.pointer("/usage/output_tokens").and_then(Value::as_u64);
                total_tokens = response.pointer("/usage/total_tokens").and_then(Value::as_u64);
                if content.is_empty() {
                    content = extract_responses_content(&response);
                }
            }
        }
        if content.is_empty() {
            return Err(StreamError::Protocol(
                "LLM returned empty response from stream".to_string(),
            ));
        }
        if !saw_terminal {
            // Responses 协议的正常结束必须有终态事件（TS PartialResponseError）。
            return Err(StreamError::Protocol(
                "stream closed without response.completed".to_string(),
            ));
        }
        Ok(StreamedCompletion { content, prompt_tokens, completion_tokens, total_tokens, done: true, tool_calls: Vec::new() })
    }
}

// ── responses 传输纯函数（TS provider.ts 逐字语义） ─────────────────────

use serde_json::Value;

/// `joinSystemPrompt`：非空 system 消息 trim 后 `\n\n` 连接；无则 None。
fn join_system_prompt(messages: &[LLMMessage]) -> Option<String> {
    let parts: Vec<&str> = messages
        .iter()
        .filter(|message| matches!(message.role, LLMRole::System) && !message.content.trim().is_empty())
        .map(|message| message.content.trim())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// `buildResponsesInput`：非 system 消息 → `[{role, content:[{type:
/// "input_text", text}]}]`（多模态图片不参与——TS 同）。
fn build_responses_input(messages: &[LLMMessage]) -> Value {
    Value::Array(
        messages
            .iter()
            .filter(|message| !matches!(message.role, LLMRole::System))
            .map(|message| {
                let role = match message.role {
                    LLMRole::System => "system",
                    LLMRole::User => "user",
                    LLMRole::Assistant => "assistant",
                    LLMRole::Tool => "tool",
                };
                serde_json::json!({
                    "role": role,
                    "content": [{ "type": "input_text", "text": message.content }],
                })
            })
            .collect(),
    )
}

/// responses 请求体：`{model, input, stream, store:false, max_output_tokens,
/// temperature, ...extra, instructions?}`（extra 覆盖基础键，instructions
/// 最后置入——TS 展开序逐字）。
fn build_responses_request(params: &ChatCompletionParams<'_>) -> Value {
    let mut body = serde_json::json!({
        "model": params.model,
        "input": build_responses_input(params.messages),
        "stream": params.stream,
        "store": false,
        "max_output_tokens": params.max_tokens,
        "temperature": params.temperature,
    });
    if let Some(extra) = params.extra {
        if let Some(target) = body.as_object_mut() {
            for (key, value) in extra {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(instructions) = join_system_prompt(params.messages) {
        body["instructions"] = serde_json::Value::String(instructions);
    }
    body
}

/// `extractResponsesContent`：output[].content[] 的 text/content/output_text
/// 字段依次取串拼接。
fn extract_responses_content(json: &Value) -> String {
    json.get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("content").and_then(Value::as_array))
                .flatten()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .or_else(|| part.get("content").and_then(Value::as_str))
                        .or_else(|| part.get("output_text").and_then(Value::as_str))
                })
                .collect::<String>()
        })
        .unwrap_or_default()
}

/// 原始 SSE `data:` 载荷提取（空行分块；`data:` 前缀剥离）。responses 协议
/// 事件为独立 JSON，不经 chat 形态的 SseStreamParser。
fn sse_data_events(text: &str) -> Vec<String> {
    let mut events = Vec::new();
    for block in text.split("\n\n") {
        for line in block.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.strip_prefix(' ').unwrap_or(data);
                if !data.is_empty() {
                    events.push(data.to_string());
                }
            }
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::provider::LLMMessage;

    #[test]
    fn build_request_shape() {
        let msgs = vec![
            LLMMessage { role: LLMRole::System, content: "你是助手".into(), tool_calls: None, tool_call_id: None },
            LLMMessage { role: LLMRole::User, content: "你好".into(), tool_calls: None, tool_call_id: None },
        ];
        let params = ChatCompletionParams { model: "gpt-4o", messages: &msgs, temperature: 0.7, max_tokens: 1000, stream: true, api_format: TransportApiFormat::Chat, extra: None, tools: None, images: None };
        let body = build_chat_completion_request(&params);
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["temperature"], 0.7);
        assert_eq!(body["max_tokens"], 1000);
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "你是助手");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn images_inject_vision_array_into_last_user_message_only() {
        let msgs = vec![
            LLMMessage { role: LLMRole::System, content: "sys".into(), tool_calls: None, tool_call_id: None },
            LLMMessage { role: LLMRole::User, content: "早前的用户轮".into(), tool_calls: None, tool_call_id: None },
            LLMMessage { role: LLMRole::Assistant, content: "回复".into(), tool_calls: None, tool_call_id: None },
            LLMMessage { role: LLMRole::User, content: "看这张图".into(), tool_calls: None, tool_call_id: None },
        ];
        let images = vec![
            ChatImage { data: "aGVsbG8=".into(), mime_type: "image/png".into() },
            ChatImage { data: "eXlNQQ==".into(), mime_type: "image/jpeg".into() },
        ];
        let params = ChatCompletionParams {
            model: "gpt-4o", messages: &msgs, temperature: 0.7, max_tokens: 100,
            stream: true, api_format: TransportApiFormat::Chat, extra: None, tools: None, images: Some(&images),
        };
        let body = build_chat_completion_request(&params);
        // 最后一条 user → vision 数组（text 段 + 两个 image_url 段）。
        let last = &body["messages"][3]["content"];
        assert!(last.is_array(), "{last}");
        assert_eq!(last[0]["type"], "text");
        assert_eq!(last[0]["text"], "看这张图");
        assert_eq!(last[1]["type"], "image_url");
        assert_eq!(last[1]["image_url"]["url"], "data:image/png;base64,aGVsbG8=");
        assert_eq!(last[2]["image_url"]["url"], "data:image/jpeg;base64,eXlNQQ==");
        // 早前 user 轮与其它角色保持纯字符串。
        assert_eq!(body["messages"][1]["content"], "早前的用户轮");
        assert_eq!(body["messages"][0]["content"], "sys");
        // 空图片列表 → 全部纯字符串。
        let empty: [ChatImage; 0] = [];
        let params = ChatCompletionParams {
            model: "gpt-4o", messages: &msgs, temperature: 0.7, max_tokens: 100,
            stream: true, api_format: TransportApiFormat::Chat, extra: None, tools: None, images: Some(&empty),
        };
        let body = build_chat_completion_request(&params);
        assert_eq!(body["messages"][3]["content"], "看这张图");
    }

    #[test]
    fn extra_does_not_override_reserved() {
        let mut extra = HashMap::new();
        extra.insert("model".into(), serde_json::json!("EVIL")); // 保留字段，应被忽略
        extra.insert("top_p".into(), serde_json::json!(0.9)); // 非保留，应保留
        let msgs = vec![LLMMessage { role: LLMRole::User, content: "x".into(), tool_calls: None, tool_call_id: None }];
        let params = ChatCompletionParams { model: "gpt-4o", messages: &msgs, temperature: 0.7, max_tokens: 100, stream: true, api_format: TransportApiFormat::Chat, extra: Some(&extra), tools: None, images: None };
        let body = build_chat_completion_request(&params);
        assert_eq!(body["model"], "gpt-4o"); // 未被覆盖
        assert_eq!(body["top_p"], 0.9); // 保留
    }

    // ── 106 号：responses 传输纯函数 ──────────────────────────────────

    fn msg(role: LLMRole, content: &str) -> LLMMessage {
        LLMMessage { role, content: content.into(), tool_calls: None, tool_call_id: None }
    }

    #[test]
    fn responses_request_shape_and_instructions() {
        let msgs = vec![
            msg(LLMRole::System, "你是助手"),
            msg(LLMRole::User, "你好"),
            msg(LLMRole::Assistant, "在"),
            msg(LLMRole::User, "继续"),
        ];
        let params = ChatCompletionParams {
            model: "gpt-5", messages: &msgs, temperature: 0.5, max_tokens: 128,
            stream: true, api_format: TransportApiFormat::Responses, extra: None, tools: None, images: None,
        };
        let body = build_responses_request(&params);
        assert_eq!(body["model"], "gpt-5");
        assert_eq!(body["store"], false);
        assert_eq!(body["max_output_tokens"], 128);
        assert_eq!(body["stream"], true);
        // system 不进 input；非 system 三条按序 input_text。
        assert_eq!(body["instructions"], "你是助手");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 3);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "你好");
        assert!(input[2]["text"].is_null(), "无 images 注入（TS 同）");
    }

    #[test]
    fn responses_extra_overrides_and_empty_system() {
        let mut extra = HashMap::new();
        extra.insert("temperature".to_string(), serde_json::json!(0.2));
        let msgs = vec![msg(LLMRole::User, "x")];
        let params = ChatCompletionParams {
            model: "m", messages: &msgs, temperature: 0.7, max_tokens: 16,
            stream: false, api_format: TransportApiFormat::Responses, extra: Some(&extra), tools: None, images: None,
        };
        let body = build_responses_request(&params);
        assert_eq!(body["temperature"], 0.2, "extra 覆盖基础键（TS ...extra 展开序）");
        assert!(body.get("instructions").is_none(), "无 system → 无 instructions");
    }

    #[test]
    fn extract_responses_content_multi_part() {
        let json: serde_json::Value = serde_json::json!({
            "output": [
                { "content": [ { "type": "reasoning", "summary": "思考中" }, { "type": "output_text", "text": "好的" } ] },
                { "content": [ { "text": "，收到" } ] },
            ]
        });
        assert_eq!(extract_responses_content(&json), "好的，收到");
        assert_eq!(extract_responses_content(&serde_json::json!({})), "");
    }

    #[test]
    fn sse_data_events_split_and_strip() {
        let text = "data: {\"a\":1}\n\ndata:{\"b\":2}\n\n: keep-alive\n\ndata: [DONE]\n\n";
        let events = sse_data_events(text);
        assert_eq!(events, vec!["{\"a\":1}", "{\"b\":2}", "[DONE]"]);
    }
}
