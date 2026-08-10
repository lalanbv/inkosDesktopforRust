//! 上下文智能过滤。
//!
//! 移植自 `packages/core/src/utils/context-filter.ts`（190 行）。依赖已移植的
//! [`crate::utils::chapter_cadence::DEFAULT_CHAPTER_CADENCE_WINDOW`]。
//!
//! 为 Writer/Auditor 提示降噪：注入真相文件的相关切片；过滤会清空时回退原文。
//!
//! ## 移植要点
//! - TS 前瞻 `split(/(?=^###)/m)` 改手写切点分割（同 pov_filter）
//! - extractNames：CJK 名字（2-4 字 + 后接标点/空白/EOF）手写；英文 `(?-u)\b[A-Z][a-z]{2,}\b`
//! - filterTableRows 泛型谓词 → Rust 闭包

use crate::utils::chapter_cadence::DEFAULT_CHAPTER_CADENCE_WINDOW;
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// 上下文封顶选项。移植 TS `ContextCapOptions`。
#[derive(Debug, Clone, Copy)]
pub struct ContextCapOptions<'a> {
    pub label: &'a str,
    pub max_chars: usize,
    pub head_ratio: Option<f64>,
}

fn table_chapter_num_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\|\s*(\d+)\s*\|").unwrap())
}
fn h3_start_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^###").unwrap())
}
fn en_name_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // (?-u)：对齐 JS 无 /u 的 ASCII \b
    R.get_or_init(|| Regex::new(r"(?-u)\b[A-Z][a-z]{2,}\b").unwrap())
}
fn header_row_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^\|\s*(章节|角色|支线|hook_id|Chapter|Character|Subplot)").unwrap())
}

fn is_cjk(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
}
fn is_name_delim(c: char) -> bool {
    matches!(c, '，' | '、' | '。' | '：') || c.is_whitespace()
}

/// 封顶大上下文块：保留开头（durable setup）+ 最新尾部，中间省略并加注。
pub fn cap_context_block(content: &str, opts: ContextCapOptions) -> String {
    if content.is_empty() || content == "(文件尚未创建)" {
        return content.to_string();
    }
    let max_chars = opts.max_chars;
    if max_chars == 0 {
        return String::new();
    }
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    let content_chars: Vec<char> = content.chars().collect();
    let content_len_utf16: usize = content_chars.iter().map(|c| c.len_utf16()).sum();
    let omitted = content_len_utf16.saturating_sub(max_chars);
    let note = format!("\n\n[InkOS context budget: omitted about {omitted} chars from {}; kept beginning and latest tail.]\n\n", opts.label);
    if max_chars <= note.chars().count() + 2 {
        // 取前 max_chars 字符
        return content_chars.iter().take(max_chars).collect();
    }
    let keep_chars = max_chars - note.chars().count();
    let head_ratio = clamp_ratio(opts.head_ratio.unwrap_or(0.45));
    let head_chars = std::cmp::max(1, (keep_chars as f64 * head_ratio).floor() as usize);
    let tail_chars = std::cmp::max(1, keep_chars - head_chars);

    let head: String = content_chars.iter().take(head_chars).collect();
    let tail: String = if tail_chars >= content_chars.len() {
        content_chars.iter().collect()
    } else {
        content_chars[content_chars.len() - tail_chars..].iter().collect()
    };
    format!("{head}{note}{tail}")
}

/// 过滤 pending_hooks：移除已回收/resolved/closed。
pub fn filter_hooks(hooks: &str) -> String {
    if hooks.is_empty() || hooks == "(文件尚未创建)" {
        return hooks.to_string();
    }
    filter_table_rows(hooks, |row| {
        let lower = row.to_lowercase();
        !lower.contains("已回收") && !lower.contains("resolved") && !lower.contains("closed")
    })
}

/// 过滤 chapter_summaries：仅保留最近 keepRecent 章。
pub fn filter_summaries(summaries: &str, current_chapter: u32, keep_recent: Option<u32>) -> String {
    if summaries.is_empty() || summaries == "(文件尚未创建)" {
        return summaries.to_string();
    }
    let keep = keep_recent.unwrap_or(DEFAULT_CHAPTER_CADENCE_WINDOW);
    filter_table_rows(summaries, |row| {
        match table_chapter_num_re().captures(row) {
            Some(caps) => caps[1].parse::<u32>().unwrap_or(0) > current_chapter.saturating_sub(keep),
            None => true,
        }
    })
}

/// 过滤 subplot_board：移除已完结/closed/resolved。
pub fn filter_subplots(board: &str) -> String {
    if board.is_empty() || board == "(文件尚未创建)" {
        return board.to_string();
    }
    filter_table_rows(board, |row| {
        let lower = row.to_lowercase();
        !lower.contains("已回收") && !lower.contains("closed") && !lower.contains("resolved") && !lower.contains("已完结")
    })
}

/// 过滤 emotional_arcs：仅保留最近 keepRecent 章。
pub fn filter_emotional_arcs(arcs: &str, current_chapter: u32, keep_recent: Option<u32>) -> String {
    if arcs.is_empty() || arcs == "(文件尚未创建)" {
        return arcs.to_string();
    }
    let keep = keep_recent.unwrap_or(DEFAULT_CHAPTER_CADENCE_WINDOW);
    filter_table_rows(arcs, |row| {
        match table_chapter_num_re().captures(row) {
            Some(caps) => caps[1].parse::<u32>().unwrap_or(0) > current_chapter.saturating_sub(keep),
            None => true,
        }
    })
}

/// 过滤 character_matrix：仅保留卷大纲当前段落提及的角色 + 主角。
pub fn filter_character_matrix(matrix: &str, volume_outline: &str, protagonist_name: Option<&str>) -> String {
    if matrix.is_empty() || matrix == "(文件尚未创建)" {
        return matrix.to_string();
    }
    let mut names = extract_names(volume_outline);
    if let Some(p) = protagonist_name {
        names.insert(p.to_string());
    }
    if names.is_empty() {
        return matrix.to_string();
    }
    let sections = split_before_h3(matrix);
    let filtered: Vec<String> = sections
        .iter()
        .map(|section| {
            filter_table_rows(section, |row| names.iter().any(|n| row.contains(n)))
        })
        .collect();
    let result = filtered.join("\n");
    // 回退：过滤后无数据行则返回原文
    let data_row_count = result
        .split('\n')
        .filter(|l| l.starts_with('|') && !l.contains("---") && !is_header_row(l))
        .count();
    if data_row_count > 0 {
        result
    } else {
        matrix.to_string()
    }
}

/// 从文本提取角色名：中文 2-4 字（后接标点/空白/EOF）；英文首字母大写 3+ 字母词。
fn extract_names(text: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    // 中文：CJK 游程，若后接 delim/EOF 则取末尾 min(4, len) 字（对齐 TS lookahead 行为）
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_cjk(chars[i]) {
            let start = i;
            while i < chars.len() && is_cjk(chars[i]) {
                i += 1;
            }
            let run_len = i - start;
            let followed_by_delim = i >= chars.len() || is_name_delim(chars[i]);
            if followed_by_delim && run_len >= 2 {
                let take = run_len.min(4);
                let name: String = chars[start + run_len - take..i].iter().collect();
                names.insert(name);
            }
        } else {
            i += 1;
        }
    }
    // 英文：首字母大写 3+ 字母
    for m in en_name_re().find_iter(text) {
        names.insert(m.as_str().to_string());
    }
    names
}

fn clamp_ratio(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.45;
    }
    value.clamp(0.2, 0.8)
}

fn is_header_row(line: &str) -> bool {
    header_row_re().is_match(line)
}

/// 按 ^### 行首切点分割（等价 TS `split(/(?=^###)/m)`）。
fn split_before_h3(s: &str) -> Vec<String> {
    let points: Vec<usize> = h3_start_re().find_iter(s).map(|m| m.start()).collect();
    let mut parts: Vec<String> = Vec::with_capacity(points.len() + 1);
    let mut prev = 0usize;
    for p in points {
        if p > prev {
            parts.push(s[prev..p].to_string());
            prev = p;
        }
    }
    parts.push(s[prev..].to_string());
    parts
}

/// 通用 markdown 表格行过滤：保留非表行 + 表头/分隔行 + 通过谓词的数据行；全空回退原文。
fn filter_table_rows<F: Fn(&str) -> bool>(content: &str, predicate: F) -> String {
    let mut non_table: Vec<&str> = Vec::new();
    let mut header: Vec<&str> = Vec::new();
    let mut data: Vec<&str> = Vec::new();
    for line in content.split('\n') {
        if !line.starts_with('|') {
            non_table.push(line);
        } else if line.contains("---") || is_header_row(line) {
            header.push(line);
        } else {
            data.push(line);
        }
    }
    let filtered: Vec<&&str> = data.iter().filter(|row| predicate(row)).collect();
    if filtered.is_empty() && !data.is_empty() {
        return content.to_string();
    }
    let mut out: Vec<&str> = non_table.into_iter().collect();
    out.extend(header);
    out.extend(filtered.into_iter().copied());
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_passes_through_uncreated() {
        assert_eq!(cap_context_block("(文件尚未创建)", ContextCapOptions { label: "x", max_chars: 100, head_ratio: None }), "(文件尚未创建)");
    }

    #[test]
    fn cap_inserts_omission_note() {
        let content = "A".repeat(300);
        let out = cap_context_block(&content, ContextCapOptions { label: "test", max_chars: 150, head_ratio: Some(0.5) });
        assert!(out.contains("[InkOS context budget: omitted about"));
        assert!(out.contains("from test"));
        assert!(out.starts_with("AAAA"));
        assert!(out.ends_with("AAAA"));
    }

    #[test]
    fn filter_hooks_removes_resolved() {
        let hooks = "| hook_id | 状态 |\n| --- | --- |\n| H1 | 进行中 |\n| H2 | 已回收 |\n| H3 | resolved |\n";
        let out = filter_hooks(hooks);
        assert!(out.contains("H1"));
        assert!(!out.contains("H2"));
        assert!(!out.contains("H3"));
    }

    #[test]
    fn filter_summaries_keeps_recent() {
        let s = "| 章节 | 标题 |\n| --- | --- |\n| 1 | 旧 |\n| 2 | 旧2 |\n| 8 | 新 |\n";
        let out = filter_summaries(s, 10, Some(4));
        assert!(out.contains("| 8 |"));
        assert!(!out.contains("| 1 |"));
    }

    #[test]
    fn filter_subplots_removes_closed() {
        let b = "| id | 状态 |\n| --- | --- |\n| S1 | 开 |\n| S2 | 已完结 |\n";
        let out = filter_subplots(b);
        assert!(out.contains("S1"));
        assert!(!out.contains("S2"));
    }

    #[test]
    fn extract_names_chinese_and_english() {
        let names = extract_names("主角林动来到此地。\nAlice arrived.");
        // CJK 游程「主角林动来到此地」8 字后接「。」→ 取末 4 =「来到此地」
        assert!(names.contains("来到此地"));
        assert!(names.contains("Alice"));
    }

    #[test]
    fn filter_character_matrix_keeps_named() {
        let matrix = "### 角色档案\n| 角色 | 描述 |\n| --- | --- |\n| 林动 | 主角 |\n| 王胖 | 配角 |\n";
        let out = filter_character_matrix(matrix, "林动登场", Some("林动"));
        assert!(out.contains("林动"));
        assert!(!out.contains("王胖"));
    }
}
