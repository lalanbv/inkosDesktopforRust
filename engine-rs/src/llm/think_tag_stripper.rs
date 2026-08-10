//! 剥离响应起始处的完整 `<think>...</think>` 块。
//!
//! 移植自 `packages/core/src/llm/think-tag-stripper.ts`（73 行）。
//!
//! 部分 OpenAI 兼容服务（MiniMax M2.x、网关代理的 DeepSeek-R1 等）把思考内容以
//! `<think>...</think>` 内联在 content 字段开头返回。只剥离「响应起始处的**完整** think 块」：
//! 正文中间出现的 `<think>` 不动；起始处未闭合的 think 块原样保留（正文没生成，剥掉会丢数据）。
//!
//! 流式剥离器在能确定「开头不是 think 块」前缓冲，不发出任何文本——思考内容根本不会先发出再消失。

use std::sync::OnceLock;
use regex::Regex;

const OPEN_TAG: &str = "<think>";
const CLOSE_TAG: &str = "</think>";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StripperState {
    Detecting,
    InsideThink,
    Passthrough,
}

/// 流式剥离器：只处理响应起始处的完整 `<think>...</think>` 块。
pub struct LeadingThinkTagStripper {
    state: StripperState,
    pending: String,
}

fn leading_ws_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s*").unwrap())
}

/// JS `\s` 等价（含 `\u{feff}`），与项目其他移植一致。
fn is_js_ws(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

impl LeadingThinkTagStripper {
    pub fn new() -> Self {
        LeadingThinkTagStripper { state: StripperState::Detecting, pending: String::new() }
    }

    /// 送入增量文本，返回可安全并入正文的部分（可能为空，表示仍在缓冲判断）。
    pub fn push(&mut self, chunk: &str) -> String {
        if self.state == StripperState::Passthrough {
            return chunk.to_string();
        }
        self.pending.push_str(chunk);

        if self.state == StripperState::Detecting {
            // leadingWhitespace = /^\s*/.exec(pending)[0]
            let ws_len: usize = leading_ws_re()
                .find(&self.pending)
                .map(|m| m.end())
                .unwrap_or(0);
            let rest = &self.pending[ws_len..];
            let rest_utf16_len = rest.encode_utf16().count();
            if rest_utf16_len < OPEN_TAG.len() {
                // 还无法判断
                if OPEN_TAG.starts_with(rest) {
                    return String::new(); // 继续缓冲
                }
                // 不是 think 前缀 → passthrough 全部
                self.state = StripperState::Passthrough;
                return std::mem::take(&mut self.pending);
            }
            if !rest.starts_with(OPEN_TAG) {
                self.state = StripperState::Passthrough;
                return std::mem::take(&mut self.pending);
            }
            self.state = StripperState::InsideThink;
        }

        // insideThink：等待闭合标签
        match self.pending.find(CLOSE_TAG) {
            None => String::new(),
            Some(close_idx) => {
                self.state = StripperState::Passthrough;
                let after = &self.pending[close_idx + CLOSE_TAG.len()..];
                let trimmed: String = after.trim_start_matches(is_js_ws).to_string();
                self.pending.clear();
                trimmed
            }
        }
    }

    /// 流结束：仍在缓冲的文本原样返回（未闭合 think 块不剥离）。
    pub fn flush(&mut self) -> String {
        self.state = StripperState::Passthrough;
        std::mem::take(&mut self.pending)
    }
}

impl Default for LeadingThinkTagStripper {
    fn default() -> Self {
        Self::new()
    }
}

/// 非流式版：剥离字符串起始处的完整 `<think>...</think>` 块（语义与流式一致）。
pub fn strip_leading_think_block(text: &str) -> String {
    let mut s = LeadingThinkTagStripper::new();
    let pushed = s.push(text);
    let flushed = s.flush();
    format!("{pushed}{flushed}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_complete_leading_block() {
        assert_eq!(strip_leading_think_block("<think>hidden</think>visible"), "visible");
    }

    #[test]
    fn preserves_leading_whitespace_then_strip() {
        assert_eq!(strip_leading_think_block("  <think>x</think>real"), "real");
    }

    #[test]
    fn passthrough_when_no_think_prefix() {
        assert_eq!(strip_leading_think_block("normal text"), "normal text");
    }

    #[test]
    fn preserves_mid_content_think_literal() {
        // 正文中间的 <think> 不动
        assert_eq!(strip_leading_think_block("正文 <think>x</think> 后续"), "正文 <think>x</think> 后续");
    }

    #[test]
    fn unclosed_leading_block_preserved_on_flush() {
        // 起始处未闭合 → flush 原样返回（不剥离）
        let out = strip_leading_think_block("<think>未闭合的思考");
        assert_eq!(out, "<think>未闭合的思考");
    }

    #[test]
    fn streaming_chunks_buffered_correctly() {
        let mut s = LeadingThinkTagStripper::new();
        // 跨 chunk 的 think 块
        assert_eq!(s.push("<thi"), "");
        assert_eq!(s.push("nk>sec"), "");
        assert_eq!(s.push("ret</think>real"), "real");
        assert_eq!(s.push(" body"), " body");
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn streaming_decides_passthrough_across_chunks() {
        let mut s = LeadingThinkTagStripper::new();
        // "<t" 是 <think> 前缀 → 缓冲；"ext" 使 pending="<text"，OPEN_TAG 不以其为前缀 → passthrough 全部
        assert_eq!(s.push("<t"), "");
        assert_eq!(s.push("ext"), "<text");
        assert_eq!(s.push(" body"), " body");
    }

    #[test]
    fn trims_whitespace_after_close_tag() {
        assert_eq!(strip_leading_think_block("<think>x</think>\n\nreal"), "real");
    }
}
