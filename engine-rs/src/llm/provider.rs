//! LLM 客户端 —— 纯函数基础（provider.ts 的可测子集）。
//!
//! 移植自 `packages/core/src/llm/provider.ts` 的纯函数部分：
//! - [`estimate_text_tokens`]：启发式 token 估算（CJK 1:1，其余 4:1）
//! - [`is_transient_llm_http_error`]：瞬时 HTTP 错误判定（429/5xx/限流短语，排除 model_not_available）
//! - [`sanitize_http_headers`] / [`is_valid_header_name`] / [`is_byte_string`]：头部清洗
//! - [`LLMMessage`] / [`LLMResponse`] / [`StreamProgress`] 类型
//!
//! ## 待移植（需 reqwest + 录制响应回放测试）
//! createLLMClient（流式 chat/responses 客户端，147-262 行）/ createStreamMonitor
//! （定时器，用 tokio::interval）/ estimatePiContextTokens（依赖 pi-ai 的 PiContext 类型）/
//! withTransientLLMRetry（重试退避）

use regex::Regex;
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::collections::HashMap;
use std::sync::OnceLock;

const INKOS_USER_AGENT: &str = "InkOS/1.3.5";

/// LLM 响应（内容 + token 用量）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LLMResponse {
    pub content: String,
    pub usage: LLMUsage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LLMUsage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
}

/// 聊天消息（system/user/assistant）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct LLMMessage {
    pub role: LLMRole,
    pub content: String,
    /// assistant 轮的 tool_calls 数组（OpenAI 形态，raw JSON 透传）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    /// tool 轮的工具调用 id。
    #[serde(rename = "tool_call_id", default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"system\" | \"user\" | \"assistant\" | \"tool\""))]
pub enum LLMRole {
    #[serde(rename = "system")] System,
    #[serde(rename = "user")] User,
    #[serde(rename = "assistant")] Assistant,
    #[serde(rename = "tool")] Tool,
}

/// 流式进度（已用时/总字符/中文字符/状态）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct StreamProgress {
    pub elapsed_ms: u64,
    pub total_chars: u32,
    pub chinese_chars: u32,
    pub status: StreamStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"streaming\" | \"done\""))]
pub enum StreamStatus {
    #[serde(rename = "streaming")] Streaming,
    #[serde(rename = "done")] Done,
}

// ── Token 估算 ──────────────────────────────────────────────────

/// 启发式 token 估算：CJK（U+3400..U+9FFF，含 Ext A + 主区）按 1:1，其余按 4 字符/token。
/// 对齐 TS `estimateTextTokens`（用 UTF-16 码元数对齐 JS .length）。
pub fn estimate_text_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    let cjk = text.chars().filter(|c| ('\u{3400}'..='\u{9fff}').contains(c)).count() as u32;
    let utf16_len = text.encode_utf16().count() as u32;
    let non_cjk = utf16_len - cjk;
    // Math.ceil(cjk + nonCjk / 4)（cjk 已整数，等价 cjk + ceil(nonCjk/4)）
    cjk + non_cjk.div_ceil(4)
}

// ── 瞬时错误判定 ────────────────────────────────────────────────

fn status_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b(429|502|503|504)\b").unwrap())
}

const TRANSIENT_PHRASES: &[&str] = &[
    "temporarily unavailable",
    "service unavailable",
    "bad gateway",
    "gateway timeout",
    "too many requests",
    "rate limit",
    "overloaded",
    "please retry",
    "try again later",
];

/// 判定是否瞬时 HTTP 错误（可重试）。排除 `model_not_available`（非瞬时）。
/// 输入是已收集的错误文本（调用方负责 collect）。
pub fn is_transient_llm_http_error(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("model_not_available") || lower.contains("model not available") {
        return false;
    }
    let status_hit = status_re().is_match(&lower);
    let phrase_hit = TRANSIENT_PHRASES.iter().any(|n| lower.contains(n));
    status_hit || phrase_hit
}

// ── HTTP 头部清洗 ───────────────────────────────────────────────

fn header_name_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$").unwrap())
}

/// 头部名是否合法（RFC token 字符集）。
pub fn is_valid_header_name(value: &str) -> bool {
    header_name_re().is_match(value)
}

/// 值是否纯字节（每 char ≤ U+00FF）——对齐 TS isByteString。
pub fn is_byte_string(value: &str) -> bool {
    value.chars().all(|c| c as u32 <= 0xFF)
}

/// 过滤头部：丢弃非法名或非字节值的项；空则返回 None。
pub fn sanitize_http_headers(headers: Option<&HashMap<String, String>>) -> Option<HashMap<String, String>> {
    let headers = headers?;
    let mut sanitized: HashMap<String, String> = HashMap::new();
    for (k, v) in headers {
        if !is_valid_header_name(k) || !is_byte_string(v) {
            continue;
        }
        sanitized.insert(k.clone(), v.clone());
    }
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

/// 合并默认 User-Agent + 清洗后的自定义头部。
pub fn merge_user_agent(headers: Option<&HashMap<String, String>>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    out.insert("User-Agent".to_string(), INKOS_USER_AGENT.to_string());
    if let Some(custom) = sanitize_http_headers(headers) {
        for (k, v) in custom {
            out.insert(k, v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_cjk_and_latin() {
        // 纯 CJK：4 字 = 4 token
        assert_eq!(estimate_text_tokens("主角觉醒"), 4);
        // 纯拉丁：8 字符 / 4 = 2 token
        assert_eq!(estimate_text_tokens("abcdefgh"), 2);
        // 混合：CJK 2 + 非CJK 5（空格+abcd）→ ceil(2 + 5/4) = ceil(3.25) = 4
        assert_eq!(estimate_text_tokens("主角 abcd"), 4);
        // 空
        assert_eq!(estimate_text_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_non_divisible_4_rounds_up() {
        // 6 拉丁 → ceil(6/4) = 2
        assert_eq!(estimate_text_tokens("abcdef"), 2);
    }

    #[test]
    fn transient_status_codes() {
        assert!(is_transient_llm_http_error("HTTP 429 Too Many Requests"));
        assert!(is_transient_llm_http_error("503 Service Unavailable"));
        assert!(is_transient_llm_http_error("got 502 from gateway"));
        assert!(is_transient_llm_http_error("504 timeout"));
    }

    #[test]
    fn transient_phrases() {
        assert!(is_transient_llm_http_error("server overloaded, retry later"));
        assert!(is_transient_llm_http_error("rate limit exceeded"));
        assert!(is_transient_llm_http_error("please retry your request"));
    }

    #[test]
    fn non_transient_excluded() {
        assert!(!is_transient_llm_http_error("model_not_available"));
        assert!(!is_transient_llm_http_error("model not available"));
        assert!(!is_transient_llm_http_error("invalid api key"));
    }

    #[test]
    fn header_validation() {
        assert!(is_valid_header_name("X-Custom-Header"));
        assert!(is_valid_header_name("Authorization"));
        assert!(!is_valid_header_name("Bad Header")); // 含空格
        assert!(!is_valid_header_name("中文")); // 非 token 字符
        assert!(is_byte_string("plain-ascii"));
        assert!(!is_byte_string("中文")); // > 255
    }

    #[test]
    fn sanitize_drops_invalid() {
        let mut h = HashMap::new();
        h.insert("X-Valid".into(), "ok".into());
        h.insert("Bad Name".into(), "x".into());
        h.insert("X-Bad-Val".into(), "中文".into());
        let out = sanitize_http_headers(Some(&h)).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out.get("X-Valid").unwrap(), "ok");
        assert!(sanitize_http_headers(None).is_none());
    }

    #[test]
    fn merge_user_agent_keeps_default() {
        let mut h = HashMap::new();
        h.insert("X-Custom".into(), "v".into());
        let merged = merge_user_agent(Some(&h));
        assert_eq!(merged.get("User-Agent").unwrap(), "InkOS/1.3.5");
        assert_eq!(merged.get("X-Custom").unwrap(), "v");
    }
}
