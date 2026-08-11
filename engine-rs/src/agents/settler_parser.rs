//! 结算输出解析器（settler-parser）。
//!
//! 移植自 `packages/core/src/agents/settler-parser.ts`（38 行）。
//! 从结算 agent 的 `=== TAG ===` 分隔输出中提取各段。

use crate::models::genre_profile::GenreProfile;

/// 结算输出（对齐 TS `SettlementOutput`）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettlementOutput {
    pub post_settlement: String,
    pub updated_state: String,
    pub updated_ledger: String,
    pub updated_hooks: String,
    pub chapter_summary: String,
    pub updated_subplots: String,
    pub updated_emotional_arcs: String,
    pub updated_character_matrix: String,
}

/// 从结算内容提取各段。对齐 TS `parseSettlementOutput`。
///
/// `updated_ledger` 仅在 `genre_profile.numerical_system` 为真时填充（与 TS 一致）；
/// `updated_state`/`updated_hooks` 缺省时有中文兜底文案（对齐 TS `|| "(状态卡未更新)"`）。
pub fn parse_settlement_output(content: &str, genre_profile: &GenreProfile) -> SettlementOutput {
    let updated_state = fallback(extract_tag(content, "UPDATED_STATE"), "(状态卡未更新)");
    let updated_ledger = if genre_profile.numerical_system {
        fallback(extract_tag(content, "UPDATED_LEDGER"), "(账本未更新)")
    } else {
        String::new()
    };
    let updated_hooks = fallback(extract_tag(content, "UPDATED_HOOKS"), "(伏笔池未更新)");

    SettlementOutput {
        post_settlement: extract_tag(content, "POST_SETTLEMENT"),
        updated_state,
        updated_ledger,
        updated_hooks,
        chapter_summary: extract_tag(content, "CHAPTER_SUMMARY"),
        updated_subplots: extract_tag(content, "UPDATED_SUBPLOTS"),
        updated_emotional_arcs: extract_tag(content, "UPDATED_EMOTIONAL_ARCS"),
        updated_character_matrix: extract_tag(content, "UPDATED_CHARACTER_MATRIX"),
    }
}

/// 提取 `=== {tag} ===` 到下一个 `=== TAG ===` 或末尾的内容（trim）。
///
/// 对齐 TS regex 的 lookahead 语义，但 Rust regex crate 不支持 lookahead——
/// 改用手动扫描：定位 `=== {tag} ===` 后，从下一个 `=== ` 起始处截断。
///
/// `pub(crate)` 以供 [`crate::agents::settler_delta_parser`] 复用（同形态 TAG 提取）。
pub(crate) fn extract_tag(content: &str, tag: &str) -> String {
    let header = format!("=== {tag} ===");
    let start = match content.find(&header) {
        Some(i) => i + header.len(),
        None => return String::new(),
    };
    let rest = &content[start..];
    // 找下一个 `=== TAG ===` 起始（trim 前导空白后）。
    let trimmed_start = rest.trim_start();
    let next = find_next_tag_header(trimmed_start).unwrap_or(trimmed_start.len());
    trimmed_start[..next].trim().to_string()
}

/// 在内容中定位下一个 `=== [A-Z_]+ ===` 的起始偏移（相对入参）。无则 None。
fn find_next_tag_header(s: &str) -> Option<usize> {
    // 从偏移 1 开始搜 `=== `（跳过当前 tag 头本身已在前一步剥离）。
    let needle = "=== ";
    let mut search_from = 0;
    while let Some(idx) = s[search_from..].find(needle) {
        let abs = search_from + idx;
        // 确认是行首（前面只有空白）或位于换行后。
        let before = &s[..abs];
        if before.is_empty() || before.ends_with([' ', '\t', '\n', '\r']) {
            // 进一步确认 header 形态：=== [A-Z_]+ ===
            if is_tag_header_at(s, abs) {
                return Some(abs);
            }
        }
        search_from = abs + needle.len();
    }
    None
}

/// 判断 s[pos..] 是否以合法 `=== TAG ===` header 开头。
fn is_tag_header_at(s: &str, pos: usize) -> bool {
    let rest = &s[pos..];
    if !rest.starts_with("=== ") {
        return false;
    }
    let after = &rest[4..];
    // 读到下一个 ` ===` 或行末。
    let name_end = after.find(" ===").or_else(|| after.find('\n')).unwrap_or(after.len());
    let name = &after[..name_end];
    !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
}

/// 空串 → 兜底文案。
fn fallback(s: String, default: &str) -> String {
    if s.is_empty() { default.to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::genre_profile::GenreProfile;

    fn profile(numerical: bool) -> GenreProfile {
        GenreProfile { numerical_system: numerical, ..GenreProfile::default() }
    }

    #[test]
    fn extracts_all_tags() {
        let content = "\
=== POST_SETTLEMENT ===
主角解开了谜题。

=== UPDATED_STATE ===
森林深处

=== UPDATED_LEDGER ===
金币 +10

=== UPDATED_HOOKS ===
h01 推进

=== CHAPTER_SUMMARY ===
本章高潮

=== UPDATED_SUBPLOTS ===
支线A

=== UPDATED_EMOTIONAL_ARCS ===
情绪曲线

=== UPDATED_CHARACTER_MATRIX ===
角色矩阵
";
        let out = parse_settlement_output(content, &profile(true));
        assert_eq!(out.post_settlement, "主角解开了谜题。");
        assert_eq!(out.updated_state, "森林深处");
        assert_eq!(out.updated_ledger, "金币 +10");
        assert_eq!(out.updated_hooks, "h01 推进");
        assert_eq!(out.chapter_summary, "本章高潮");
        assert_eq!(out.updated_subplots, "支线A");
        assert_eq!(out.updated_emotional_arcs, "情绪曲线");
        assert_eq!(out.updated_character_matrix, "角色矩阵");
    }

    #[test]
    fn ledger_empty_when_numerical_system_false() {
        let content = "=== UPDATED_LEDGER ===\n金币 +10\n\n=== UPDATED_STATE ===\nx";
        let out = parse_settlement_output(content, &profile(false));
        assert_eq!(out.updated_ledger, "", "numerical_system=false 时 ledger 强制空");
        assert_eq!(out.updated_state, "x");
    }

    #[test]
    fn missing_state_falls_back_to_default_text() {
        let content = "=== POST_SETTEMENT ===\nx"; // 无 UPDATED_STATE/UPDATED_HOOKS
        let out = parse_settlement_output(content, &profile(false));
        assert_eq!(out.updated_state, "(状态卡未更新)");
        assert_eq!(out.updated_hooks, "(伏笔池未更新)");
        assert_eq!(out.updated_ledger, "");
    }

    #[test]
    fn missing_ledger_with_numerical_falls_back() {
        let content = "=== POST_SETTLEMENT ===\nx";
        let out = parse_settlement_output(content, &profile(true));
        assert_eq!(out.updated_ledger, "(账本未更新)");
    }

    #[test]
    fn empty_content_returns_defaults() {
        let out = parse_settlement_output("", &profile(false));
        assert_eq!(out.post_settlement, "");
        assert_eq!(out.updated_state, "(状态卡未更新)");
        assert_eq!(out.updated_hooks, "(伏笔池未更新)");
    }

    #[test]
    fn extract_stops_at_next_tag() {
        // 段内容含换行，但到下一 tag 截止。
        let content = "=== POST_SETTLEMENT ===\n第一行\n第二行\n=== UPDATED_STATE ===\nstate";
        let out = parse_settlement_output(content, &profile(false));
        assert_eq!(out.post_settlement, "第一行\n第二行");
    }
}
