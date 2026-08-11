//! 通知文本格式化。
//!
//! 移植自 `packages/core/src/notify/format.ts`（17 行，纯函数）。

use regex::Regex;
use std::sync::OnceLock;

fn fence_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"```[^\n]*\n?").unwrap())
}
fn bold_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\*\*([^*]+)\*\*").unwrap())
}
fn inline_code_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"`([^`]+)`").unwrap())
}

/// 剥除常见 markdown 标记（代码栅栏/加粗/内联代码），使 markdown 通道消息在纯文本通道可读。
pub fn strip_markdown_marks(text: &str) -> String {
    let s = fence_re().replace_all(text, "").into_owned();
    let s = bold_re().replace_all(&s, "$1").into_owned();
    inline_code_re().replace_all(&s, "$1").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_code_fence() {
        // 仅剥栅栏标记行（```js\n 与 ```\n），栅栏内代码内容保留（对齐 TS）
        assert_eq!(strip_markdown_marks("前\n```js\nconst x = 1;\n```\n后"), "前\nconst x = 1;\n后");
    }

    #[test]
    fn unwraps_bold() {
        assert_eq!(strip_markdown_marks("**重点**内容"), "重点内容");
    }

    #[test]
    fn unwraps_inline_code() {
        assert_eq!(strip_markdown_marks("用 `cargo build` 构建"), "用 cargo build 构建");
    }

    #[test]
    fn combined_marks() {
        let s = strip_markdown_marks("## 标题\n\n**加粗** 与 `code` 和\n```\nfenced\n```\n");
        assert!(!s.contains("**"));
        assert!(!s.contains("```"));
        assert!(!s.contains('`'));
        assert!(s.contains("加粗"));
        assert!(s.contains("code"));
    }

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(strip_markdown_marks("普通文本无标记"), "普通文本无标记");
    }
}
