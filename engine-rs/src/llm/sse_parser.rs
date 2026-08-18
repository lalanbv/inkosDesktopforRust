//! OpenAI 兼容 SSE 流式块解析器。
//!
//! 流式 LLM 客户端的核心纯函数：解析 `data: {json}\n\n` 行 + `[DONE]` 终止符，
//! 从 choices[].delta.content 累积文本。不依赖 pi-ai/providers bank，可独立测试。
//!
//! ## 移植要点
//! - 容错：跨 chunk 的不完整 data 行缓冲（push 可分片调用）
//! - 跳过非 data 行（event:/comment/空行）
//! - [DONE] 终止；其余 data 行解析 JSON，取 choices[0].delta.content（含 tool_calls 增量）
//!
//! ## 待移植（需 reqwest + pi-ai）
//! createLLMClient 的 HTTP 拉取 + piModel 构造 + 多格式（responses/anthropic-messages）。

use serde::Deserialize;

/// 单个 SSE data 块解析结果。
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    /// 文本增量
    Delta(String),
    /// 推理增量（129 号：pi-ai 同款 reasoning_content | reasoning |
    /// reasoning_text 首个非空字段——thinking 块的流式载体）。
    ReasoningDelta(String),
    /// 工具调用增量（index, tool_call id, name 片段, arguments 片段）
    ToolCallDelta {
        index: u32,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    /// 用量（通常在末帧）
    Usage {
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        total_tokens: Option<u64>,
    },
    /// choices[0].finish_reason 非空（终态信号之一；可与 delta 同帧——
    /// TS `stream closed without [DONE]/finish_reason` 守卫的数据源）。
    FinishReason(String),
    /// 流结束（[DONE]）
    Done,
}

/// 增量 SSE 解析器：跨 chunk 缓冲不完整行。
pub struct SseStreamParser {
    pending: String,
}

impl SseStreamParser {
    pub fn new() -> Self {
        SseStreamParser { pending: String::new() }
    }

    /// 送入一段原始字节（文本），返回该段解析出的事件（可能为空，表示仍在缓冲）。
    pub fn push(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.pending.push_str(chunk);
        let mut events = Vec::new();
        loop {
            // SSE 事件以空行（\n\n 或 \r\n\r\n）分隔
            let split_at = self.pending.find("\n\n").or_else(|| self.pending.find("\r\n\r\n"));
            let Some(end) = split_at else { break };
            let sep_len = if self.pending[end..].starts_with("\r\n\r\n") { 4 } else { 2 };
            let block: String = self.pending.drain(..end + sep_len).collect();
            events.extend(parse_sse_block(&block));
        }
        events
    }

    /// 流结束：刷新剩余缓冲（最后一帧可能无尾随空行）。
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if !self.pending.is_empty() {
            events.extend(parse_sse_block(&self.pending));
            self.pending.clear();
        }
        events
    }
}

impl Default for SseStreamParser {
    fn default() -> Self {
        Self::new()
    }
}

/// 解析单个 SSE 块（一个或多个 `data:` 行 + 可能的 event/id 行）。
/// 取最后一个 data 行作为 payload（OpenAI 兼容每块单 data 行）。
/// 返回 0..2 个事件：delta 增量与 finish_reason/usage 可同帧共存（TS 逐项
/// 处理语义——单事件模型无法表达 content+finish_reason 同帧）。
fn parse_sse_block(block: &str) -> Vec<SseEvent> {
    let mut data_payload: Option<&str> = None;
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("data:").or_else(|| line.strip_prefix("data ")) {
            let trimmed = rest.trim();
            if trimmed.is_empty() {
                continue;
            }
            data_payload = Some(trimmed);
        }
        // event:/id:/comment 行忽略
    }
    let Some(payload) = data_payload else { return Vec::new() };
    if payload == "[DONE]" {
        return vec![SseEvent::Done];
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return Vec::new();
    };
    parse_openai_chunk(&value)
}

#[derive(Debug, Deserialize)]
struct OpenAiChunk {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    // pi-ai reasoningFields 同款三字段（llama.cpp reasoning_content / 其它
    // OpenAI 兼容 reasoning / reasoning_text），首个非空者生效（防重复）。
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    reasoning_text: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDeltaRaw>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDeltaRaw {
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<ToolCallFunctionRaw>,
}

#[derive(Debug, Default, Deserialize)]
struct ToolCallFunctionRaw {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
}

fn parse_openai_chunk(value: &serde_json::Value) -> Vec<SseEvent> {
    let chunk: OpenAiChunk = serde_json::from_value(value.clone()).unwrap_or(OpenAiChunk { choices: vec![], usage: None });
    let mut events = Vec::new();
    // TS 逐项处理语义：delta 增量 / finish_reason 终态 / usage 可同帧共存
    if let Some(choice) = chunk.choices.first() {
        if let Some(content) = &choice.delta.content {
            if !content.is_empty() {
                events.push(SseEvent::Delta(content.clone()));
            }
        }
        let reasoning = [
            choice.delta.reasoning_content.as_deref(),
            choice.delta.reasoning.as_deref(),
            choice.delta.reasoning_text.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|text| !text.is_empty());
        if let Some(reasoning) = reasoning {
            events.push(SseEvent::ReasoningDelta(reasoning.to_string()));
        }
        if let Some(tc) = choice.delta.tool_calls.first() {
            events.push(SseEvent::ToolCallDelta {
                index: tc.index,
                id: tc.id.clone(),
                name: tc.function.as_ref().and_then(|f| f.name.clone()),
                arguments: tc.function.as_ref().and_then(|f| f.arguments.clone()),
            });
        }
        if let Some(reason) = choice.finish_reason.as_deref() {
            if !reason.is_empty() {
                events.push(SseEvent::FinishReason(reason.to_string()));
            }
        }
    }
    if let Some(usage) = chunk.usage {
        events.push(SseEvent::Usage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        });
    }
    events
}

/// 便捷：把完整 SSE 文本一次性解析为所有事件。
pub fn parse_sse_stream(text: &str) -> Vec<SseEvent> {
    let mut parser = SseStreamParser::new();
    let mut events = parser.push(text);
    events.extend(parser.finish());
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_deltas() {
        let stream = "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"世界\"}}]}\n\n";
        let events = parse_sse_stream(stream);
        let text: String = events.iter().filter_map(|e| if let SseEvent::Delta(s) = e { Some(s.clone()) } else { None }).collect();
        assert_eq!(text, "你好世界");
    }

    #[test]
    fn done_terminator() {
        let stream = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\ndata: [DONE]\n\n";
        let events = parse_sse_stream(stream);
        assert!(events.iter().any(|e| matches!(e, SseEvent::Done)));
    }

    #[test]
    fn reasoning_delta_first_non_empty_field_wins() {
        // reasoning_content 优先（llama.cpp 形态）。
        let events = parse_sse_stream(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"让我想想\"}}]}\n\n",
        );
        assert_eq!(
            events,
            vec![SseEvent::ReasoningDelta("让我想想".to_string())]
        );
        // 无 reasoning_content 时 reasoning 兜底；两者同帧不重复（pi-ai 防重复语义）。
        let events = parse_sse_stream(
            "data: {\"choices\":[{\"delta\":{\"reasoning\":\"B\"}}]}\n\n",
        );
        assert_eq!(events, vec![SseEvent::ReasoningDelta("B".to_string())]);
        // content 与 reasoning 同帧：双双产出（pi-ai 双分支语义——content 块与
        // thinking 块独立累积；130 号 Vec 化后不再互相遮蔽）。
        let events = parse_sse_stream(
            "data: {\"choices\":[{\"delta\":{\"content\":\"正文\",\"reasoning_content\":\"思考\"}}]}\n\n",
        );
        assert_eq!(
            events,
            vec![
                SseEvent::Delta("正文".to_string()),
                SseEvent::ReasoningDelta("思考".to_string()),
            ]
        );
    }

    #[test]
    fn finish_reason_shares_frame_with_delta() {
        // 终态信号与增量同帧：两个事件都要产出（TS 逐项处理语义）。
        let events = parse_sse_stream(
            "data: {\"choices\":[{\"delta\":{\"content\":\"收尾\"},\"finish_reason\":\"stop\"}]}\n\n",
        );
        assert_eq!(
            events,
            vec![
                SseEvent::Delta("收尾".to_string()),
                SseEvent::FinishReason("stop".to_string()),
            ]
        );
        // 空帧（无 delta 无 finish_reason 无 usage）→ 零事件（原空占位移除）。
        let events = parse_sse_stream("data: {\"choices\":[{\"delta\":{}}]}\n\n");
        assert!(events.is_empty());
    }

    #[test]
    fn tool_call_delta() {
        let stream = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"write\",\"arguments\":\"{\\\"x\\\":\"}}]}}]}\n\n";
        let events = parse_sse_stream(stream);
        assert!(matches!(events.first(), Some(SseEvent::ToolCallDelta { index: 0, id, name, arguments }) if id.as_deref() == Some("call_1") && name.as_deref() == Some("write") && arguments.is_some()));
    }

    #[test]
    fn usage_frame() {
        let stream = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n\n";
        let events = parse_sse_stream(stream);
        assert!(matches!(events.first(), Some(SseEvent::Usage { prompt_tokens: Some(10), completion_tokens: Some(5), total_tokens: Some(15) })));
    }

    #[test]
    fn incremental_chunks_buffer() {
        let mut p = SseStreamParser::new();
        // 跨 chunk 的不完整行
        assert!(p.push("data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}").is_empty());
        assert!(p.push("]}\n\n").len() == 1);
    }

    #[test]
    fn skips_non_data_lines() {
        let stream = ": comment\nevent: ping\ndata: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n";
        let events = parse_sse_stream(stream);
        assert!(events.iter().any(|e| matches!(e, SseEvent::Delta(s) if s == "ok")));
    }

    #[test]
    fn finish_flushes_last_frame() {
        let mut p = SseStreamParser::new();
        p.push("data: {\"choices\":[{\"delta\":{\"content\":\"end\"}}]}"); // 无尾随 \n\n
        let events = p.finish();
        assert!(events.iter().any(|e| matches!(e, SseEvent::Delta(s) if s == "end")));
    }

    #[test]
    fn crlf_separators() {
        let stream = "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\r\n\r\n";
        let text: String = parse_sse_stream(stream).iter().filter_map(|e| if let SseEvent::Delta(s) = e { Some(s.clone()) } else { None }).collect();
        assert_eq!(text, "ab");
    }
}
