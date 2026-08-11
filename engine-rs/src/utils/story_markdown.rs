//! Story markdown 工具（部分移植）。
//!
//! 移植自 `packages/core/src/utils/story-markdown.ts`（346 行）。本模块当前仅含
//! [`normalize_hook_id`]——它是 [`hook_arbiter`](crate::utils::hook_arbiter) 的唯一外部依赖。
//! story-markdown 的其余部分（hook 表格行解析、章节号解析、depends_on 解析等）属
//! state-bootstrap 的依赖域，后续阶段补齐。
//!
//! ## 移植纪律
//! `normalize_hook_id` 的 markdown 包装剥离顺序、dash 归一、CJK 保留判定须与 TS **逐字一致**——
//! 它决定 hook id 的规范化结果，进而影响 hook 仲裁的去重/映射匹配（load-bearing）。

use regex::Regex;
use std::sync::OnceLock;

fn unwrap_re() -> &'static Regex {
    // 七种 markdown 包装，按 TS 顺序迭代剥离（[text](url) / ** / __ / * / _ / ` / ~~）。
    // anchored ^...$，非贪婪。TS 用 /u，Rust regex 默认 Unicode，等价。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"^(?s)\[(.+?)\]\([^)]+\)$|^\*\*(.+)\*\*$|^__(.+)__$|^\*(.+)\*$|^_(.+)_$|^`(.+)`$|^~~(.+)~~$",
        )
        .expect("normalize_hook_id unwrap regex")
    })
}

fn dash_collapse_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"-{2,}").expect("dash collapse regex"))
}

fn dash_trim_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^-+|-+$").expect("dash trim regex"))
}

fn has_keepable_char_re() -> &'static Regex {
    // TS: /[a-z0-9一-鿿]/iu —— i flag 使 a-z 匹配大写；Rust 用显式 a-zA-Z。
    // 一-鿿 即 CJK 统一汉字基本区。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[a-zA-Z0-9\x{4e00}-\x{9fff}]").expect("keepable char regex"))
}

/// 规范化 hook id：迭代剥离 markdown 包装 → dash 归一 → 去首尾 dash →
/// 若不含字母/数字/CJK 则返回空串。
///
/// 对齐 TS `normalizeHookId(value: string | undefined): string`。
pub fn normalize_hook_id(value: Option<&str>) -> String {
    let mut normalized = value.unwrap_or("").trim().to_string();
    if normalized.is_empty() {
        return String::new();
    }

    // 迭代剥离直到稳定（与 TS while 循环等价）。
    loop {
        if let Some(caps) = unwrap_re().captures(&normalized) {
            // 七个捕获组对应七种包装，取首个匹配的非空内组。
            let unwrapped = caps
                .iter()
                .skip(1)
                .flatten()
                .map(|m| m.as_str())
                .next();
            if let Some(inner) = unwrapped {
                let next = inner.trim().to_string();
                if next == normalized || next.is_empty() {
                    break;
                }
                normalized = next;
                continue;
            }
        }
        break;
    }

    normalized = dash_collapse_re().replace_all(&normalized, "-").to_string();
    normalized = dash_trim_re().replace_all(&normalized, "").to_string();
    normalized = normalized.trim().to_string();

    if has_keepable_char_re().is_match(&normalized) {
        normalized
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_returns_empty() {
        assert_eq!(normalize_hook_id(None), "");
    }

    #[test]
    fn empty_returns_empty() {
        assert_eq!(normalize_hook_id(Some("")), "");
        assert_eq!(normalize_hook_id(Some("   ")), "");
    }

    #[test]
    fn strips_markdown_link_wrapper() {
        // [text](url) → text
        assert_eq!(
            normalize_hook_id(Some("[anonymous-source](./hooks.md#h01)")),
            "anonymous-source"
        );
    }

    #[test]
    fn strips_bold_italics_code_strikethrough() {
        assert_eq!(normalize_hook_id(Some("**bold**")), "bold");
        assert_eq!(normalize_hook_id(Some("__strong__")), "strong");
        assert_eq!(normalize_hook_id(Some("*italics*")), "italics");
        assert_eq!(normalize_hook_id(Some("_em_")), "em");
        assert_eq!(normalize_hook_id(Some("`code`")), "code");
        assert_eq!(normalize_hook_id(Some("~~strike~~")), "strike");
    }

    #[test]
    fn collapses_repeated_dashes_and_trims_edges() {
        assert_eq!(normalize_hook_id(Some("a---b")), "a-b");
        assert_eq!(normalize_hook_id(Some("---a---")), "a");
        assert_eq!(normalize_hook_id(Some("a--b--c")), "a-b-c");
    }

    #[test]
    fn returns_empty_when_no_keepable_chars() {
        // 纯标点/空格不含字母数字 CJK → 空。
        assert_eq!(normalize_hook_id(Some("---***")), "");
        assert_eq!(normalize_hook_id(Some("。。。")), "");
    }

    #[test]
    fn preserves_chinese() {
        assert_eq!(normalize_hook_id(Some("伏笔-01")), "伏笔-01");
        assert_eq!(normalize_hook_id(Some("**伏笔**")), "伏笔");
    }

    #[test]
    fn iterates_until_stable_for_nested_wrappers() {
        // 嵌套：[**text**](url) → 先剥 link → **text** → 再剥 bold → text
        assert_eq!(
            normalize_hook_id(Some("[**nested**](./x.md)")),
            "nested"
        );
    }
}
