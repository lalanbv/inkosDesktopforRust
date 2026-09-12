//! 结算工作集构造（governed settlement working set）。
//!
//! 移植自 `packages/core/src/utils/governed-working-set.ts`（395 行）。本模块含：
//! - [`build_governed_hook_working_set`]：writer 结算 Phase 2 的伏笔工作集裁剪
//!   （选中 ID ∪ 意图 agenda ∪ 章节窗口）
//! - [`merge_table_markdown_by_key`] / [`merge_character_matrix_markdown`]：
//!   legacy 结算输出的表格按 key 合并回原文（防 LLM 丢行）
//! - [`build_governed_character_matrix_working_set`]：角色矩阵工作集裁剪
//!   （本章活跃角色过滤）
//!
//! ## 移植纪律
//! 表格解析（首表定位 / 分隔行归入 header）、section 切分（`### ` 前缀）、
//! key 合并语义（覆盖同行 / 追加新行 / 保留脚手架行）与 TS 逐字一致——
//! 结算回写路径直接消费这些产物，丢行即丢真相。

use std::collections::HashSet;

use regex::Regex;
use std::sync::OnceLock;

use crate::models::input_governance::ContextPackage;
use crate::models::runtime_state::HookRecord;
use crate::utils::hook_lifecycle::{
    is_hook_within_chapter_window, DEFAULT_HOOK_LOOKAHEAD_CHAPTERS,
};
use crate::utils::language::WritingLanguage;
use crate::utils::story_markdown::{parse_pending_hooks_markdown, render_hook_snapshot};

/// 占位文案（governed-working-set 判定两个占位符）。
const FILE_MISSING: &str = "(文件不存在)";
const FILE_NOT_CREATED: &str = "(文件尚未创建)";

/// [`build_governed_hook_working_set`] 入参。
pub struct GovernedHookWorkingSetInput<'a> {
    pub hooks_markdown: &'a str,
    pub context_package: &'a ContextPackage,
    pub chapter_intent: Option<&'a str>,
    pub chapter_number: u32,
    pub language: WritingLanguage,
    pub keep_recent: Option<u32>,
}

/// 构造 governed 结算的伏笔工作集。对齐 TS `buildGovernedHookWorkingSet`：
/// 选中 ID（`story/pending_hooks.md#` 前缀条目）∪ 意图 agenda ID ∪ 章节窗口内
/// hook；裁剪结果为空或反而变大时回退原文。
pub fn build_governed_hook_working_set(params: &GovernedHookWorkingSetInput<'_>) -> String {
    let hooks_markdown = params.hooks_markdown;
    if hooks_markdown.is_empty() || hooks_markdown == FILE_MISSING || hooks_markdown == FILE_NOT_CREATED {
        return hooks_markdown.to_string();
    }

    let hooks = parse_pending_hooks_markdown(hooks_markdown);
    if hooks.is_empty() {
        return hooks_markdown.to_string();
    }

    const SOURCE_PREFIX: &str = "story/pending_hooks.md#";
    let selected_ids: HashSet<String> = params
        .context_package
        .selected_context
        .iter()
        .filter(|entry| entry.source.starts_with(SOURCE_PREFIX))
        .map(|entry| entry.source[SOURCE_PREFIX.len()..].to_string())
        .filter(|id| !id.is_empty())
        .collect();
    let agenda_ids = collect_hook_agenda_ids(params.chapter_intent);
    let keep_recent = params.keep_recent.unwrap_or(5);

    let working_set: Vec<HookRecord> = hooks
        .iter()
        .filter(|hook| {
            selected_ids.contains(&hook.hook_id)
                || agenda_ids.contains(&hook.hook_id)
                || is_hook_within_chapter_window(
                    hook,
                    params.chapter_number,
                    keep_recent,
                    DEFAULT_HOOK_LOOKAHEAD_CHAPTERS,
                )
        })
        .cloned()
        .collect();

    if working_set.is_empty() || working_set.len() >= hooks.len() {
        return hooks_markdown.to_string();
    }

    render_hook_snapshot(&working_set, params.language)
}

/// 从章节意图提取 Hook Agenda ID。对齐 TS `collectHookAgendaIds` 的行状态机：
/// 进入 `## Hook Agenda` 节 → 在 `### Must Advance` / `### Eligible Resolve` /
/// `### Stale Debt` 子节内捕获 `- ` 行（值非空且非 "none"）→ 遇下一个 `## ` 节终止。
fn collect_hook_agenda_ids(chapter_intent: Option<&str>) -> HashSet<String> {
    let Some(intent) = chapter_intent else {
        return HashSet::new();
    };
    if intent.trim().is_empty() {
        return HashSet::new();
    }

    let mut ids = HashSet::new();
    let mut in_hook_agenda = false;
    let mut capture_ids = false;

    for raw_line in intent.split('\n') {
        let line = raw_line.trim();

        if line == "## Hook Agenda" {
            in_hook_agenda = true;
            capture_ids = false;
            continue;
        }

        if !in_hook_agenda {
            continue;
        }

        if line.starts_with("## ") && line != "## Hook Agenda" {
            break;
        }

        if line == "### Must Advance" || line == "### Eligible Resolve" || line == "### Stale Debt" {
            capture_ids = true;
            continue;
        }

        if line.starts_with("### ") {
            capture_ids = false;
            continue;
        }

        if !capture_ids || !line.starts_with("- ") {
            continue;
        }

        let value = line[2..].trim();
        if !value.is_empty() && value.to_lowercase() != "none" {
            ids.insert(value.to_string());
        }
    }

    ids
}

/// 单表解析产物。对齐 TS `ParsedTable`。
struct ParsedTable {
    leading_lines: Vec<String>,
    data_rows: Vec<Vec<String>>,
    trailing_lines: Vec<String>,
}

/// 定位首个 markdown 表格。对齐 TS `parseSingleTable`：
/// 首个 `|` 行为 header 起点，紧随且含 `---` 的 `|` 行归入 header（leading），
/// 其后 `|` 行为数据行，末数据行之后为 trailing。
fn parse_single_table(content: &str) -> Option<ParsedTable> {
    let lines: Vec<&str> = content.split('\n').collect();
    let table_indexes: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim().starts_with('|'))
        .map(|(index, _)| index)
        .collect();
    if table_indexes.is_empty() {
        return None;
    }

    let header_start = table_indexes[0];
    let next_index = table_indexes.get(1).copied();
    let header_end = match next_index {
        Some(next) if lines[next].contains("---") => next,
        _ => header_start,
    };
    let data_indexes: Vec<usize> = table_indexes
        .iter()
        .copied()
        .filter(|index| *index > header_end)
        .collect();
    let last_data_index = data_indexes.last().copied().unwrap_or(header_end);

    Some(ParsedTable {
        leading_lines: lines[..=header_end].iter().map(|s| s.to_string()).collect(),
        data_rows: data_indexes
            .iter()
            .map(|index| parse_row(lines[*index]))
            .collect(),
        trailing_lines: lines[last_data_index + 1..]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    })
}

/// `### ` 节解析产物。对齐 TS `parseSections`。
struct ParsedSections {
    top_lines: Vec<String>,
    sections: Vec<MarkdownSection>,
}

struct MarkdownSection {
    heading: String,
    body: String,
}

fn parse_sections(content: &str) -> ParsedSections {
    let mut top_lines: Vec<String> = Vec::new();
    let mut sections: Vec<MarkdownSection> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_body: Vec<String> = Vec::new();

    for line in content.split('\n') {
        if line.starts_with("### ") {
            if let Some(heading) = current_heading.take() {
                sections.push(MarkdownSection {
                    heading,
                    body: current_body.join("\n").trim_end().to_string(),
                });
                current_body = Vec::new();
            }
            current_heading = Some(line.to_string());
            continue;
        }

        if current_heading.is_some() {
            current_body.push(line.to_string());
        } else {
            top_lines.push(line.to_string());
        }
    }

    if let Some(heading) = current_heading.take() {
        sections.push(MarkdownSection {
            heading,
            body: current_body.join("\n").trim_end().to_string(),
        });
    }

    ParsedSections { top_lines, sections }
}

/// 表格行 → 单元格数组。对齐 TS `parseRow`：`split("|").slice(1, -1).map(trim)`
/// （丢弃首尾竖线产生的空段）。
fn parse_row(line: &str) -> Vec<String> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 2 {
        return Vec::new();
    }
    parts[1..parts.len() - 1]
        .iter()
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn render_row(row: &[String]) -> String {
    format!("| {} |", row.join(" | "))
}

fn build_key(row: &[String], key_columns: &[usize]) -> String {
    key_columns
        .iter()
        .map(|index| row.get(*index).cloned().unwrap_or_default())
        .collect::<Vec<String>>()
        .join("::")
}

fn pick_scaffold(primary: &[String], fallback: &[String]) -> Vec<String> {
    if !primary.is_empty() {
        primary.to_vec()
    } else {
        fallback.to_vec()
    }
}

/// 按 key 列合并两张 markdown 表。对齐 TS `mergeTableMarkdownByKey`：
/// 原表行为底、更新行按 key 覆盖或追加；任一侧无表或更新无数据行 → 原样返回
/// updated；脚手架行（表头/分隔）优先取原表，原表无则取更新表。
pub fn merge_table_markdown_by_key(
    original: &str,
    updated: &str,
    key_columns: &[usize],
) -> String {
    let Some(original_table) = parse_single_table(original) else {
        return updated.to_string();
    };
    let Some(updated_table) = parse_single_table(updated) else {
        return updated.to_string();
    };
    if updated_table.data_rows.is_empty() {
        return updated.to_string();
    }

    let mut merged_rows = original_table.data_rows.clone();
    let mut original_index: std::collections::HashMap<String, usize> = merged_rows
        .iter()
        .enumerate()
        .map(|(index, row)| (build_key(row, key_columns), index))
        .collect();

    for row in &updated_table.data_rows {
        let key = build_key(row, key_columns);
        match original_index.get(&key) {
            None => {
                original_index.insert(key, merged_rows.len());
                merged_rows.push(row.clone());
            }
            Some(existing) => {
                merged_rows[*existing] = row.clone();
            }
        }
    }

    let mut lines = pick_scaffold(&original_table.leading_lines, &updated_table.leading_lines);
    lines.extend(merged_rows.iter().map(|row| render_row(row)));
    lines.extend(pick_scaffold(
        &original_table.trailing_lines,
        &updated_table.trailing_lines,
    ));
    lines.join("\n").trim_end().to_string()
}

/// 角色矩阵 section 合并。对齐 TS `mergeCharacterMatrixMarkdown`：
/// 按 section 下标配对合并（各 section key 列不同：`[0]` / `[0,1]` / `[0,3]`），
/// 更新多出的 section 原样追加。
pub fn merge_character_matrix_markdown(original: &str, updated: &str) -> String {
    let original_sections = parse_sections(original);
    let updated_sections = parse_sections(updated);
    if original_sections.sections.is_empty() || updated_sections.sections.is_empty() {
        return updated.to_string();
    }

    const SECTION_KEY_COLUMNS: [&[usize]; 3] = [&[0], &[0, 1], &[0, 3]];

    let mut merged_sections: Vec<MarkdownSection> = Vec::new();
    for (index, section) in original_sections.sections.iter().enumerate() {
        let Some(next) = updated_sections.sections.get(index) else {
            merged_sections.push(MarkdownSection {
                heading: section.heading.clone(),
                body: section.body.clone(),
            });
            continue;
        };

        let key_columns = SECTION_KEY_COLUMNS.get(index).copied().unwrap_or(&[0]);
        merged_sections.push(MarkdownSection {
            heading: section.heading.clone(),
            body: merge_table_markdown_by_key(&section.body, &next.body, key_columns),
        });
    }

    for index in original_sections.sections.len()..updated_sections.sections.len() {
        merged_sections.push(MarkdownSection {
            heading: updated_sections.sections[index].heading.clone(),
            body: updated_sections.sections[index].body.clone(),
        });
    }

    let mut lines = pick_scaffold(
        &original_sections.top_lines,
        &updated_sections.top_lines,
    );
    lines.extend(
        merged_sections
            .iter()
            .flat_map(|section| [section.heading.clone(), section.body.clone()]),
    );
    lines.join("\n").trim_end().to_string()
}

/// [`build_governed_character_matrix_working_set`] 入参。
pub struct GovernedMatrixWorkingSetInput<'a> {
    pub matrix_markdown: &'a str,
    pub chapter_intent: &'a str,
    pub context_package: &'a ContextPackage,
    pub protagonist_name: Option<&'a str>,
}

/// 构造 governed 结算的角色矩阵工作集。对齐 TS
/// `buildGovernedCharacterMatrixWorkingSet`：按本章活跃角色名过滤各 section 行。
pub fn build_governed_character_matrix_working_set(
    params: &GovernedMatrixWorkingSetInput<'_>,
) -> String {
    let matrix_markdown = params.matrix_markdown;
    if matrix_markdown.is_empty() || matrix_markdown == FILE_MISSING || matrix_markdown == FILE_NOT_CREATED {
        return matrix_markdown.to_string();
    }

    let parsed = parse_sections(matrix_markdown);
    if parsed.sections.is_empty() {
        return matrix_markdown.to_string();
    }

    let active_names = collect_governed_character_names(params);
    let filtered_sections: Vec<MarkdownSection> = parsed
        .sections
        .iter()
        .enumerate()
        .map(|(index, section)| MarkdownSection {
            heading: section.heading.clone(),
            body: filter_matrix_section(&section.body, index, &active_names),
        })
        .collect();

    let mut lines = parsed.top_lines;
    lines.extend(
        filtered_sections
            .iter()
            .flat_map(|section| [section.heading.clone(), section.body.clone()]),
    );
    lines.join("\n").trim_end().to_string()
}

/// 收集本章活跃角色名。对齐 TS `collectGovernedCharacterNames`：矩阵候选 ×
/// （意图 + 选中上下文 reason/excerpt）语料；CJK 名 contains 匹配，拉丁名
/// 大小写不敏感整词匹配；主角名恒入集。
fn collect_governed_character_names(params: &GovernedMatrixWorkingSetInput<'_>) -> HashSet<String> {
    let candidates = extract_character_candidates_from_matrix(params.matrix_markdown);
    let mut corpus_parts: Vec<String> = vec![params.chapter_intent.to_string()];
    for entry in &params.context_package.selected_context {
        corpus_parts.push(entry.reason.clone());
        corpus_parts.push(entry.excerpt.clone().unwrap_or_default());
    }
    let corpus = corpus_parts.join("\n");

    let mut active_names: HashSet<String> = HashSet::new();
    for candidate in &candidates {
        if let Some(protagonist) = params.protagonist_name {
            if matches_name(candidate, protagonist) {
                active_names.insert(candidate.clone());
                continue;
            }
        }
        if is_name_mentioned(candidate, &corpus) {
            active_names.insert(candidate.clone());
        }
    }

    if let Some(protagonist) = params.protagonist_name {
        for candidate in &candidates {
            if matches_name(candidate, protagonist) {
                active_names.insert(candidate.clone());
            }
        }
    }

    active_names
}

/// 从矩阵各 section 提取角色名候选。对齐 TS
/// `extractCharacterCandidatesFromMatrix`：section 1（关系表）取前两列，其余取首列。
fn extract_character_candidates_from_matrix(matrix_markdown: &str) -> Vec<String> {
    let parsed = parse_sections(matrix_markdown);
    let mut names: HashSet<String> = HashSet::new();

    for (index, section) in parsed.sections.iter().enumerate() {
        let Some(table) = parse_single_table(&section.body) else {
            continue;
        };

        for row in &table.data_rows {
            let candidates: Vec<Option<&String>> = if index == 1 {
                vec![row.first(), row.get(1)]
            } else {
                vec![row.first()]
            };
            for candidate in candidates.into_iter().flatten() {
                let normalized = candidate.trim();
                if !normalized.is_empty() {
                    names.insert(normalized.to_string());
                }
            }
        }
    }

    names.into_iter().collect()
}

/// 过滤矩阵 section 表行。对齐 TS `filterMatrixSection`：section 1（关系表）
/// 要求两端角色都活跃（右端空视为活跃），其余 section 首列活跃即保留。
fn filter_matrix_section(
    section_body: &str,
    section_index: usize,
    active_names: &HashSet<String>,
) -> String {
    let Some(table) = parse_single_table(section_body) else {
        return section_body.to_string();
    };

    let filtered_rows: Vec<&Vec<String>> = table
        .data_rows
        .iter()
        .filter(|row| {
            if row.is_empty() {
                return false;
            }

            if section_index == 1 {
                let left = row.first().cloned().unwrap_or_default();
                let right = row.get(1).cloned().unwrap_or_default();
                return active_names.contains(&left)
                    && (right.is_empty() || active_names.contains(&right));
            }

            let first = row.first().cloned().unwrap_or_default();
            active_names.contains(&first)
        })
        .collect();

    let mut lines = table.leading_lines;
    lines.extend(filtered_rows.iter().map(|row| render_row(row)));
    lines.extend(table.trailing_lines);
    lines.join("\n").trim_end().to_string()
}

/// 名字是否被语料提及。对齐 TS `isNameMentioned`：CJK 名 contains；
/// 拉丁名 `\b{escaped}\b` 大小写不敏感。
fn is_name_mentioned(candidate: &str, corpus: &str) -> bool {
    if candidate.is_empty() || corpus.is_empty() {
        return false;
    }

    if contains_cjk(candidate) {
        return corpus.contains(candidate);
    }

    let pattern = format!(r"(?i)\b{}\b", escape_regex(candidate));
    Regex::new(&pattern)
        .map(|re| re.is_match(corpus))
        .unwrap_or(false)
}

fn matches_name(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    left.trim().to_lowercase() == right.trim().to_lowercase()
}

fn contains_cjk(value: &str) -> bool {
    value.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// 对齐 TS `escapeRegExp`：仅转义 `[.*+?^${}()|[\]\\]`（regex::escape 转义集
/// 更宽，会产生 `\ ` 等差异，此处逐字对齐）。
fn escape_regex(value: &str) -> String {
    static SPECIAL: OnceLock<Regex> = OnceLock::new();
    let re = SPECIAL.get_or_init(|| Regex::new(r"[.*+?^${}()|\[\]\\]").expect("escape regex"));
    re.replace_all(value, "\\$0").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook_record(id: &str, start: u32, last_advanced: u32) -> HookRecord {
        HookRecord {
            hook_id: id.to_string(),
            start_chapter: start,
            hook_type: "main".to_string(),
            status: crate::models::runtime_state::HookStatus::Open,
            status_raw: String::new(),
            last_advanced_chapter: last_advanced,
            expected_payoff: String::new(),
            payoff_timing: None,
            notes: String::new(),
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        }
    }

    #[test]
    fn hook_working_set_placeholder_passthrough() {
        for placeholder in ["", "(文件不存在)", "(文件尚未创建)"] {
            let input = GovernedHookWorkingSetInput {
                hooks_markdown: placeholder,
                context_package: &ContextPackage::default(),
                chapter_intent: None,
                chapter_number: 3,
                language: WritingLanguage::Zh,
                keep_recent: None,
            };
            assert_eq!(build_governed_hook_working_set(&input), placeholder);
        }
    }

    #[test]
    fn hook_working_set_selected_id_overrides_window() {
        let markdown = "| hook_id | 起始章节 |\n| --- | --- |\n| H01 | 1 |\n| H02 | 40 |\n";
        let context_package = ContextPackage {
            chapter: 3,
            selected_context: vec![crate::models::input_governance::ContextSource {
                source: "story/pending_hooks.md#H02".to_string(),
                reason: "本章回收".to_string(),
                excerpt: None,
                rank: None,
            }],
        };
        let input = GovernedHookWorkingSetInput {
            hooks_markdown: markdown,
            context_package: &context_package,
            chapter_intent: None,
            chapter_number: 3,
            language: WritingLanguage::Zh,
            keep_recent: Some(0),
        };
        let out = build_governed_hook_working_set(&input);
        assert!(out.contains("H02"));
        assert!(!out.contains("H01"));
    }

    #[test]
    fn hook_working_set_full_set_returns_original() {
        let markdown = "| hook_id | 起始章节 |\n| --- | --- |\n| H01 | 1 |\n";
        let input = GovernedHookWorkingSetInput {
            hooks_markdown: markdown,
            context_package: &ContextPackage::default(),
            chapter_intent: None,
            chapter_number: 3,
            language: WritingLanguage::Zh,
            keep_recent: Some(5),
        };
        assert_eq!(build_governed_hook_working_set(&input), markdown);
    }

    #[test]
    fn agenda_ids_capture_must_advance_only() {
        let intent = "## Hook Agenda\n### Must Advance\n- H01\n- H02\n### Something\n- H03\n## Next\n- H04";
        let ids = collect_hook_agenda_ids(Some(intent));
        assert!(ids.contains("H01"));
        assert!(ids.contains("H02"));
        assert!(!ids.contains("H03"));
        assert!(!ids.contains("H04"));
    }

    #[test]
    fn agenda_ids_none_value_skipped() {
        let intent = "## Hook Agenda\n### Stale Debt\n- none\n- H09\n";
        let ids = collect_hook_agenda_ids(Some(intent));
        assert!(!ids.contains("none"));
        assert!(ids.contains("H09"));
    }

    #[test]
    fn merge_table_updates_and_appends() {
        let original = "# 标题\n\n| 名字 | 状态 |\n| --- | --- |\n| 甲 | open |\n| 乙 | open |\n";
        let updated = "| 名字 | 状态 |\n| --- | --- |\n| 甲 | resolved |\n| 丙 | open |\n";
        let merged = merge_table_markdown_by_key(original, updated, &[0]);
        assert!(merged.starts_with("# 标题"));
        assert!(merged.contains("| 甲 | resolved |"));
        assert!(merged.contains("| 乙 | open |"));
        assert!(merged.contains("| 丙 | open |"));
    }

    #[test]
    fn merge_table_no_table_returns_updated() {
        assert_eq!(
            merge_table_markdown_by_key("纯文本", "| a | b |\n| --- | --- |\n| 1 | 2 |", &[0]),
            "| a | b |\n| --- | --- |\n| 1 | 2 |"
        );
    }

    #[test]
    fn merge_matrix_sections_by_index() {
        let original = "# 矩阵\n### 一级角色\n| 名字 | 状态 |\n| --- | --- |\n| 甲 | 高 |\n### 关系\n| 左 | 右 | 强度 |\n| --- | --- | --- |\n| 甲 | 乙 | 强 |\n";
        let updated = "### 一级角色\n| 名字 | 状态 |\n| --- | --- |\n| 甲 | 低 |\n### 关系\n| 左 | 右 | 强度 |\n| --- | --- | --- |\n| 甲 | 乙 | 弱 |\n### 新节\n| x | y |\n| --- | --- |\n";
        let merged = merge_character_matrix_markdown(original, updated);
        assert!(merged.contains("# 矩阵"));
        assert!(merged.contains("| 甲 | 低 |"));
        assert!(merged.contains("| 甲 | 乙 | 弱 |"));
        assert!(merged.contains("### 新节"));
    }

    #[test]
    fn matrix_working_set_filters_inactive() {
        let matrix = "### 一级角色\n| 名字 | 状态 |\n| --- | --- |\n| 甲 | 高 |\n| 乙 | 低 |\n### 关系\n| 左 | 右 | 强度 |\n| --- | --- | --- |\n| 甲 | 乙 | 强 |\n";
        let context_package = ContextPackage {
            chapter: 5,
            selected_context: vec![crate::models::input_governance::ContextSource {
                source: "story/current_state.md#甲".to_string(),
                reason: "甲在场".to_string(),
                excerpt: Some("甲 拔剑".to_string()),
                rank: None,
            }],
        };
        let input = GovernedMatrixWorkingSetInput {
            matrix_markdown: matrix,
            chapter_intent: "本章甲独行",
            context_package: &context_package,
            protagonist_name: None,
        };
        let out = build_governed_character_matrix_working_set(&input);
        assert!(out.contains("甲"));
        assert!(!out.contains("| 乙 | 低 |"));
        assert!(!out.contains("甲 | 乙"));
    }

    #[test]
    fn protagonist_always_active() {
        let matrix = "### 一级角色\n| 名字 | 状态 |\n| --- | --- |\n| 主角 | 高 |\n| 路人 | 低 |\n";
        let input = GovernedMatrixWorkingSetInput {
            matrix_markdown: matrix,
            chapter_intent: "无关文本",
            context_package: &ContextPackage::default(),
            protagonist_name: Some("主角"),
        };
        let out = build_governed_character_matrix_working_set(&input);
        assert!(out.contains("主角"));
        assert!(!out.contains("路人"));
    }

    #[test]
    fn latin_name_word_boundary_case_insensitive() {
        assert!(is_name_mentioned("Alice", "alice walked into the room"));
        assert!(is_name_mentioned("Alice", "say Alice! now"));
        assert!(!is_name_mentioned("Alice", "malicious intent"));
        assert!(is_name_mentioned("林动", "林动出手"));
    }

    #[test]
    fn render_hook_snapshot_empty_is_none_marker() {
        assert_eq!(
            render_hook_snapshot(&[], WritingLanguage::En),
            "- none".to_string()
        );
    }

    #[test]
    fn render_hook_snapshot_escapes_pipes() {
        let mut hook = hook_record("H01", 1, 2);
        hook.notes = "含|竖线".to_string();
        let out = render_hook_snapshot(&[hook], WritingLanguage::Zh);
        assert!(out.starts_with("| hook_id |"));
        assert!(out.contains("含\\|竖线"));
    }
}
