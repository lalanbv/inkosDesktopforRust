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
    pub extra: Option<&'a HashMap<String, serde_json::Value>>,
    /// OpenAI tools 数组（raw JSON schema 透传；None = 不带工具）。
    pub tools: Option<&'a serde_json::Value>,
    /// 多模态图片（95 号）：注入**最后一条 user 消息**为 OpenAI vision
    /// content 数组（`[{type:"text"},{type:"image_url"}…]`）。TS 侧
    /// `agent.prompt(message, images)` 的当轮 prompt 图——历史轮次保留。
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
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamError::Http(e) => write!(f, "HTTP 错误: {e}"),
            StreamError::BadStatus(code, body) => write!(f, "HTTP {code}: {body}"),
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

    /// 流式 chat completion：POST /chat/completions，收集完整响应。
    /// 非 streaming 用法（一次性收集）；流式 SSE 经 sse_parser 解析。
    pub async fn stream_chat(&self, params: &ChatCompletionParams<'_>) -> Result<StreamedCompletion, StreamError> {
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
        let params = ChatCompletionParams { model: "gpt-4o", messages: &msgs, temperature: 0.7, max_tokens: 1000, stream: true, extra: None, tools: None, images: None };
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
            stream: true, extra: None, tools: None, images: Some(&images),
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
            stream: true, extra: None, tools: None, images: Some(&empty),
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
        let params = ChatCompletionParams { model: "gpt-4o", messages: &msgs, temperature: 0.7, max_tokens: 100, stream: true, extra: Some(&extra), tools: None, images: None };
        let body = build_chat_completion_request(&params);
        assert_eq!(body["model"], "gpt-4o"); // 未被覆盖
        assert_eq!(body["top_p"], 0.9); // 保留
    }
}
