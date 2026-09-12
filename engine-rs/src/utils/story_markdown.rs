//! Story markdown 工具（解析子集）。
//!
//! 移植自 `packages/core/src/utils/story-markdown.ts`（346 行）。本模块含：
//! - [`normalize_hook_id`]：hook id 规范化（hook-arbiter 依赖）
//! - **解析子集**（state-bootstrap 依赖）：[`parse_markdown_table_rows`] /
//!   [`parse_chapter_summaries_markdown`] / [`parse_pending_hooks_markdown`] /
//!   [`parse_current_state_facts`] 及其辅助函数
//!
//! 渲染函数：[`render_hook_snapshot`]（governed-working-set 内联快照）+
//! [`render_summary_snapshot`]（planner 意图 markdown 的摘要快照）。
//!
//! ## 移植纪律
//! 所有解析逻辑须与 TS **逐字一致**——markdown 表格行/单元格切分、章节号严格解析（防止
//! 「第141号文明」被误读为章节号）、Phase 7 元数据（depends_on/core_hook/half_life/promoted）
//! 的多形态（7/8/11/12/13 列）行解析均为 load-bearing，state-bootstrap 直接消费产出填充持久化状态。

use regex::Regex;
use std::sync::OnceLock;

use crate::models::runtime_state::{HookRecord, HookStatus};
use crate::state::memory_db::{NewFact, StoredSummary};
use crate::utils::hook_lifecycle::{localize_hook_payoff_timing, resolve_hook_payoff_timing};

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

// =============================================================================
// 解析子集（state-bootstrap 依赖）
// =============================================================================

fn digits_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\d+").expect("digits regex"))
}

fn strict_integer_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\d+$").expect("strict integer regex"))
}

fn chapter_summary_first_col_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\d+$").expect("chapter summary first col regex"))
}

fn current_chapter_label_re() -> &'static Regex {
    // TS: /^(当前章节|current chapter)$/i
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(当前章节|current chapter)$").expect("current chapter label regex"))
}

fn protagonist_label_re() -> &'static Regex {
    // TS inferFactSubject 的 6 个 protagonist label（i flag）。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(当前位置|current location|主角状态|protagonist state|当前目标|current goal|当前限制|current constraint|当前敌我|current alliances|current relationships|当前冲突|current conflict)$")
            .expect("protagonist label regex")
    })
}

fn true_cell_re() -> &'static Regex {
    // TS parseBooleanCell: /^(true|yes|y|是|核心|core|1|✓|✔)$/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(true|yes|y|是|核心|core|1|✓|✔)$").expect("true cell regex"))
}

fn optional_true_re() -> &'static Regex {
    // TS parseOptionalBooleanCell true 分支。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(true|yes|y|是|核心|core|1|✓|✔|promoted|已升级)$").expect("optional true regex")
    })
}

fn optional_false_re() -> &'static Regex {
    // TS parseOptionalBooleanCell false 分支。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(false|no|n|否|未升级|seed|0|✗|✘)$").expect("optional false regex"))
}

fn depends_on_split_re() -> &'static Regex {
    // TS parseDependsOn: split /[,，、\/]+/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[,，、/]+").expect("depends_on split regex"))
}

fn depends_on_wrap_re() -> &'static Regex {
    // TS: replace(/^[\[\(]\s*/, "").replace(/\s*[\]\)]$/, "")
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[\[\(]\s*").expect("depends_on open wrap regex"))
}

fn depends_on_close_wrap_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\s*[\]\)]$").expect("depends_on close wrap regex"))
}

/// 解析 markdown 表格的所有数据行（每个 cell 已 trim）。
///
/// 对齐 TS `parseMarkdownTableRows`：split 行 → trim → `|` 开头 → 排除分隔线（含 `---`）
/// → split `|` 取 `slice(1, -1)`（去首尾空段）→ trim → 排除全空行。
pub fn parse_markdown_table_rows(markdown: &str) -> Vec<Vec<String>> {
    markdown
        .lines()
        .map(|line| line.trim())
        .filter(|line| line.starts_with('|'))
        .filter(|line| !line.contains("---"))
        .filter_map(|line| {
            // JS split('|').slice(1, -1)：去首段（`|` 前空串）与尾段（末 `|` 后空串）。
            // Rust split('|') 对 "|a|b|" → ["", "a", "b", ""]；对 "|a|b" → ["", "a", "b"]。
            let segs: Vec<&str> = line.split('|').collect();
            if segs.len() < 2 {
                return None;
            }
            // 去首；若末段为空（行以 `|` 结尾）则一并去尾，等价 slice(1, -1)。
            let mid = if segs.last().map(|s| s.is_empty()).unwrap_or(false) {
                &segs[1..segs.len().saturating_sub(1)]
            } else {
                &segs[1..]
            };
            let cells: Vec<String> = mid.iter().map(|s| s.trim().to_string()).collect();
            if cells.iter().any(|c| !c.is_empty()) {
                Some(cells)
            } else {
                None
            }
        })
        .collect()
}

/// 解析章节摘要 markdown 表格 → [`StoredSummary`] 列表。
/// 对齐 TS `parseChapterSummariesMarkdown`：首列须为纯数字（章节号）。
pub fn parse_chapter_summaries_markdown(markdown: &str) -> Vec<StoredSummary> {
    parse_markdown_table_rows(markdown)
        .into_iter()
        .filter(|row| row.first().is_some_and(|c| chapter_summary_first_col_re().is_match(c)))
        .map(|row| StoredSummary {
            chapter: row[0].parse::<i64>().unwrap_or(0),
            title: row.get(1).cloned().unwrap_or_default(),
            characters: row.get(2).cloned().unwrap_or_default(),
            events: row.get(3).cloned().unwrap_or_default(),
            state_changes: row.get(4).cloned().unwrap_or_default(),
            hook_activity: row.get(5).cloned().unwrap_or_default(),
            mood: row.get(6).cloned().unwrap_or_default(),
            chapter_type: row.get(7).cloned().unwrap_or_default(),
            // R2/358 号：8 列旧表（无张力列）→ None；10 列新表解析并 clamp。
            conflict_level: parse_tension_cell(row.get(8)),
            reveal_level: parse_tension_cell(row.get(9)),
        })
        .collect()
}

/// 张力列单元格 → 1–10 分；空/非纯数字 → `None`（对齐 TS `parseTensionCell`）。
fn parse_tension_cell(cell: Option<&String>) -> Option<i64> {
    let raw = cell?;
    if !raw.chars().all(|c| c.is_ascii_digit()) || raw.is_empty() {
        return None;
    }
    Some(raw.parse::<i64>().ok()?.clamp(1, 10))
}

/// 解析 pending hooks markdown → [`HookRecord`] 列表（含 Phase 7 元数据）。
///
/// 对齐 TS `parsePendingHooksMarkdown`：优先表格路径（过滤 hook_id 表头、空 id 行），
/// fallback 到 bullet 列表（`- xxx` → notes-only hook，id=`hook-N`）。
pub fn parse_pending_hooks_markdown(markdown: &str) -> Vec<HookRecord> {
    let table_rows: Vec<Vec<String>> = parse_markdown_table_rows(markdown)
        .into_iter()
        .filter(|row| row.first().map(|c| c.to_lowercase() != "hook_id").unwrap_or(true))
        .filter(|row| row.first().is_some_and(|c| !normalize_hook_id(Some(c)).is_empty()))
        .collect();

    if !table_rows.is_empty() {
        return table_rows.into_iter().map(|row| parse_pending_hook_row(&row)).collect();
    }

    // bullet fallback：notes-only。
    markdown
        .lines()
        .map(|line| line.trim())
        .filter(|line| line.starts_with('-'))
        .map(|line| line.replacen("- ", "", 1).replacen('-', "", 1).trim().to_string())
        .filter(|line| !line.is_empty())
        .enumerate()
        .map(|(index, notes)| HookRecord {
            hook_id: format!("hook-{}", index + 1),
            start_chapter: 0,
            hook_type: "unspecified".to_string(),
            status: HookStatus::Open,
            status_raw: String::new(),
            last_advanced_chapter: 0,
            expected_payoff: String::new(),
            payoff_timing: None,
            notes,
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        })
        .collect()
}

/// 渲染 hook 快照表（无标题行，空表 → `- none`）。
///
/// 对齐 TS `renderHookSnapshot`（governed-working-set 的结算工作集渲染）。
/// 与 [`crate::state::projections::render_hooks_projection`] 的差异：无 `# 伏笔池`
/// 标题、无诊断标注、无排序（保持传入顺序）、无尾随空行——它是嵌入 prompt 的
/// 内联片段，不是独立真相文件。
pub fn render_hook_snapshot(
    hooks: &[HookRecord],
    language: crate::utils::language::WritingLanguage,
) -> String {
    if hooks.is_empty() {
        return "- none".to_string();
    }

    let en = language == crate::utils::language::WritingLanguage::En;
    let headers: [&str; 2] = if en {
        [
            "| hook_id | start_chapter | type | status | last_advanced | expected_payoff | payoff_timing | depends_on | pays_off_in_arc | core_hook | half_life | promoted | notes |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    } else {
        [
            "| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    };

    let mut lines: Vec<String> = headers.iter().map(|s| s.to_string()).collect();
    for hook in hooks {
        let timing = resolve_hook_payoff_timing(
            hook.payoff_timing.map(hook_payoff_timing_str),
            Some(&hook.expected_payoff),
            Some(&hook.notes),
        );
        let cells = [
            hook.hook_id.clone(),
            hook.start_chapter.to_string(),
            hook.hook_type.clone(),
            crate::utils::hook_lifecycle::hook_status_text(hook).to_string(),
            hook.last_advanced_chapter.to_string(),
            hook.expected_payoff.clone(),
            localize_hook_payoff_timing(timing, language).to_string(),
            render_depends_on_cell(hook.depends_on.as_deref(), language),
            hook.pays_off_in_arc.clone().unwrap_or_default(),
            render_core_hook_cell(hook.core_hook == Some(true), language),
            render_half_life_cell(hook.half_life_chapters),
            render_promoted_cell(hook.promoted, language),
            hook.notes.clone(),
        ];
        let escaped: Vec<String> = cells.iter().map(|c| escape_table_cell(c)).collect();
        lines.push(format!("| {} |", escaped.join(" | ")));
    }
    lines.join("\n")
}

fn hook_payoff_timing_str(t: crate::models::runtime_state::HookPayoffTiming) -> &'static str {
    use crate::models::runtime_state::HookPayoffTiming;
    match t {
        HookPayoffTiming::Immediate => "immediate",
        HookPayoffTiming::NearTerm => "near-term",
        HookPayoffTiming::MidArc => "mid-arc",
        HookPayoffTiming::SlowBurn => "slow-burn",
        HookPayoffTiming::Endgame => "endgame",
    }
}

/// 渲染章节摘要快照表（空表 → `- none`）。
///
/// 对齐 TS `renderSummarySnapshot`：10 列固定表头（zh/en 双语，R2/358 号起
/// 增冲突强度/揭示强度两列），单元格 `escapeTableCell`（`|` 转义 + trim），
/// 无标题行、无尾随空行。缺分渲染空单元格，保持旧表消费者无感。
pub fn render_summary_snapshot(
    summaries: &[StoredSummary],
    language: crate::utils::language::WritingLanguage,
) -> String {
    if summaries.is_empty() {
        return "- none".to_string();
    }

    let en = language == crate::utils::language::WritingLanguage::En;
    let headers: [&str; 2] = if en {
        [
            "| chapter | title | characters | events | stateChanges | hookActivity | mood | chapterType | conflict | reveal |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    } else {
        [
            "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 | 冲突强度 | 揭示强度 |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    };

    let mut lines: Vec<String> = headers.iter().map(|s| s.to_string()).collect();
    for summary in summaries {
        let cells = [
            summary.chapter.to_string(),
            summary.title.clone(),
            summary.characters.clone(),
            summary.events.clone(),
            summary.state_changes.clone(),
            summary.hook_activity.clone(),
            summary.mood.clone(),
            summary.chapter_type.clone(),
            summary.conflict_level.map(|v| v.to_string()).unwrap_or_default(),
            summary.reveal_level.map(|v| v.to_string()).unwrap_or_default(),
        ];
        let escaped: Vec<String> = cells.iter().map(|c| escape_table_cell(c)).collect();
        lines.push(format!("| {} |", escaped.join(" | ")));
    }
    lines.join("\n")
}

fn render_depends_on_cell(
    ids: Option<&[String]>,
    language: crate::utils::language::WritingLanguage,
) -> String {
    let empty = ids.map(|v| v.is_empty()).unwrap_or(true);
    if empty {
        return if language == crate::utils::language::WritingLanguage::En {
            "none".to_string()
        } else {
            "无".to_string()
        };
    }
    format!("[{}]", ids.unwrap().join(", "))
}

fn render_core_hook_cell(is_core: bool, language: crate::utils::language::WritingLanguage) -> String {
    if language == crate::utils::language::WritingLanguage::En {
        return if is_core { "true" } else { "false" }.to_string();
    }
    if is_core { "是" } else { "否" }.to_string()
}

fn render_half_life_cell(value: Option<u32>) -> String {
    match value {
        Some(v) if v > 0 => v.to_string(),
        _ => String::new(),
    }
}

fn render_promoted_cell(
    value: Option<bool>,
    language: crate::utils::language::WritingLanguage,
) -> String {
    match value {
        None => String::new(),
        Some(v) => {
            if language == crate::utils::language::WritingLanguage::En {
                v.to_string()
            } else if v {
                "是".to_string()
            } else {
                "否".to_string()
            }
        }
    }
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|").trim().to_string()
}

/// 解析 current_state markdown → [`NewFact`] 列表。
///
/// 对齐 TS `parseCurrentStateFacts`：优先字段/值表格（识别「当前章节」行定 stateChapter，
/// 其余 label/value 行 → fact），fallback 到 bullet 列表（note_N）。
pub fn parse_current_state_facts(markdown: &str, fallback_chapter: i64) -> Vec<NewFact> {
    let table_rows = parse_markdown_table_rows(markdown);
    let field_value_rows: Vec<&Vec<String>> = table_rows
        .iter()
        .filter(|row| row.len() >= 2)
        .filter(|row| !is_state_table_header_row(row))
        .collect();

    if !field_value_rows.is_empty() {
        let state_chapter = field_value_rows
            .iter()
            .find(|row| row.first().is_some_and(|c| is_current_chapter_label(c)))
            .and_then(|row| row.get(1))
            .map(|v| parse_integer(Some(v)))
            .filter(|v| *v != 0)
            .unwrap_or(fallback_chapter);

        return field_value_rows
            .iter()
            .filter(|row| row.first().is_some_and(|c| !is_current_chapter_label(c)))
            .filter_map(|row| {
                let label = row.first().map(|s| s.trim()).unwrap_or("");
                let value = row.get(1).map(|s| s.trim()).unwrap_or("");
                if label.is_empty() || value.is_empty() {
                    return None;
                }
                Some(NewFact {
                    subject: infer_fact_subject(label),
                    predicate: label.to_string(),
                    object: value.to_string(),
                    valid_from_chapter: state_chapter,
                    valid_until_chapter: None,
                    source_chapter: state_chapter,
                })
            })
            .collect();
    }

    // bullet fallback：predicate=note_N，subject=current_state。
    let bullet_facts: Vec<String> = markdown
        .lines()
        .map(|line| line.trim())
        .filter(|line| line.starts_with('-'))
        .map(|line| line.replacen("- ", "", 1).replacen('-', "", 1).trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    bullet_facts
        .into_iter()
        .enumerate()
        .map(|(index, object)| NewFact {
            subject: "current_state".to_string(),
            predicate: format!("note_{}", index + 1),
            object,
            valid_from_chapter: fallback_chapter,
            valid_until_chapter: None,
            source_chapter: fallback_chapter,
        })
        .collect()
}

/// 判定是否为状态表表头行（「字段|值」/「field|value」）。对齐 TS `isStateTableHeaderRow`。
pub fn is_state_table_header_row(row: &[String]) -> bool {
    let first = row.first().map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let second = row.get(1).map(|s| s.trim().to_lowercase()).unwrap_or_default();
    (first == "字段" && second == "值") || (first == "field" && second == "value")
}

/// 判定是否为「当前章节」label。对齐 TS `isCurrentChapterLabel`。
pub fn is_current_chapter_label(label: &str) -> bool {
    current_chapter_label_re().is_match(label.trim())
}

/// 由 label 推断 fact subject（protagonist 系列 → "protagonist"，其余 → "current_state"）。
/// 对齐 TS `inferFactSubject`。
pub fn infer_fact_subject(label: &str) -> String {
    if protagonist_label_re().is_match(label.trim()) {
        "protagonist".to_string()
    } else {
        "current_state".to_string()
    }
}

/// 宽松整数解析（提取首个数字序列）。对齐 TS `parseInteger`；空/无数字 → 0。
pub fn parse_integer(value: Option<&str>) -> i64 {
    let Some(v) = value else { return 0 };
    if v.trim().is_empty() {
        return 0;
    }
    digits_re()
        .find(v)
        .and_then(|m| m.as_str().parse::<i64>().ok())
        .unwrap_or(0)
}

/// 严格章节号解析：normalizeHookId 后须为纯数字，否则 0。
/// 防止「第141号文明」被误读为章节号。对齐 TS `parseStrictChapterInteger`。
fn parse_strict_chapter_integer(value: Option<&str>) -> u32 {
    let Some(v) = value else { return 0 };
    let stripped = normalize_hook_id(Some(v));
    if strict_integer_re().is_match(&stripped) {
        stripped.parse::<u32>().unwrap_or(0)
    } else {
        0
    }
}

/// 解析 pending hook 表格行为 [`HookRecord`]（含 Phase 7 元数据）。
///
/// 对齐 TS `parsePendingHookRow`：按列数分 7（legacy）/8（Phase5-6）/11（Phase7 compact）
/// /12（+half_life）/13（+promoted）形态；payoff_timing 经
/// [`normalize_hook_payoff_timing`](crate::utils::hook_lifecycle::normalize_hook_payoff_timing) 规范化。
fn parse_pending_hook_row(row: &[String]) -> HookRecord {
    use crate::utils::hook_lifecycle::normalize_hook_payoff_timing;

    let phase7_promoted = row.len() >= 13;
    let phase7_half_life = row.len() == 12;
    let phase7_compact = row.len() == 11;
    let phase7 = phase7_promoted || phase7_half_life || phase7_compact;
    let legacy_shape = row.len() < 8;

    let payoff_timing = if legacy_shape {
        None
    } else {
        normalize_hook_payoff_timing(row.get(6).map(|s| s.as_str()))
    };

    // notes 列随形态变化（trailing columns 可能含 stale/blocked 诊断列，parser 跳过到 notes）。
    let notes = if phase7_promoted {
        row.get(12).cloned().unwrap_or_default()
    } else if phase7_half_life {
        row.get(11).cloned().unwrap_or_default()
    } else if phase7_compact {
        row.get(10).cloned().unwrap_or_default()
    } else if legacy_shape {
        row.get(6).cloned().unwrap_or_default()
    } else {
        row.get(7).cloned().unwrap_or_default()
    };

    let cell = |idx: usize| row.get(idx).cloned().unwrap_or_default();
    let status_cell = cell(3);
    let status = parse_hook_status(status_cell.clone());

    let mut record = HookRecord {
        hook_id: normalize_hook_id(row.first().map(|s| s.as_str())),
        start_chapter: parse_strict_chapter_integer(row.get(1).map(|s| s.as_str())),
        hook_type: cell(2),
        status,
        status_raw: status_cell,
        last_advanced_chapter: parse_strict_chapter_integer(row.get(4).map(|s| s.as_str())),
        expected_payoff: cell(5),
        payoff_timing,
        notes,
        depends_on: None,
        pays_off_in_arc: None,
        core_hook: None,
        half_life_chapters: None,
        advanced_count: None,
        promoted: None,
    };

    if !phase7 {
        return record;
    }

    record.depends_on = Some(parse_depends_on(&cell(7)));
    record.pays_off_in_arc = Some(cell(8).trim().to_string());
    record.core_hook = Some(parse_boolean_cell(row.get(9).map(|s| s.as_str())));
    if phase7_half_life || phase7_promoted {
        record.half_life_chapters = parse_optional_int(row.get(10).map(|s| s.as_str()));
    }
    if phase7_promoted {
        record.promoted = parse_optional_boolean_cell(row.get(11).map(|s| s.as_str()));
    }
    record
}

/// 把状态单元格解析为 [`HookStatus`]。
/// TS 用字符串（"open"/"progressing"/"deferred"/"resolved" 等），Rust 收敛为枚举；
/// 默认 open，resolved 系列（resolved/closed/done/已回收/已解决）→ Resolved。
fn parse_hook_status(cell: String) -> HookStatus {
    let lower = cell.trim().to_lowercase();
    if lower.is_empty() {
        return HookStatus::Open;
    }
    match lower.as_str() {
        "open" => HookStatus::Open,
        "progressing" | "active" | "ongoing" | "进行中" | "推进中" => HookStatus::Progressing,
        "deferred" | "defer" | "paused" | "延后" | "搁置" => HookStatus::Deferred,
        "resolved" | "closed" | "done" | "complete" | "completed" | "已解决" | "已回收" | "已完成" | "已关闭" => {
            HookStatus::Resolved
        }
        _ => HookStatus::Open,
    }
}

/// 解析 depends_on 单元格（`[H01, H02]` / `H01,H02` / `H01/H02` / `none`/`-`/`无` → 空）。
/// 对齐 TS `parseDependsOn`：剥离方括号 → split 分隔符 → 逐项 normalizeHookId → 去空。
fn parse_depends_on(cell: &str) -> Vec<String> {
    let trimmed = cell.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let lower = trimmed.to_lowercase();
    if lower == "none" || lower == "n/a" || lower == "-" || trimmed == "无" {
        return Vec::new();
    }
    let stripped = depends_on_wrap_re().replace_all(trimmed, "").into_owned();
    let stripped = depends_on_close_wrap_re().replace_all(stripped.as_str(), "").into_owned();
    depends_on_split_re()
        .split(stripped.as_str())
        .map(|item| normalize_hook_id(Some(item)))
        .filter(|item| !item.is_empty())
        .collect()
}

/// 解析布尔单元格（空→false；命中 true 模式→true）。对齐 TS `parseBooleanCell`。
fn parse_boolean_cell(cell: Option<&str>) -> bool {
    let normalized = cell.unwrap_or("").trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    true_cell_re().is_match(&normalized)
}

/// 解析可选布尔单元格（空→None；true/false 模式分别→Some(true/false)；未识别→None）。
/// 对齐 TS `parseOptionalBooleanCell`。
fn parse_optional_boolean_cell(cell: Option<&str>) -> Option<bool> {
    let normalized = cell.unwrap_or("").trim();
    if normalized.is_empty() {
        return None;
    }
    let lower = normalized.to_lowercase();
    if optional_true_re().is_match(&lower) {
        Some(true)
    } else if optional_false_re().is_match(&lower) {
        Some(false)
    } else {
        None
    }
}

/// 解析可选正整数单元格（空/非数字/≤0 → None）。对齐 TS `parseOptionalInt`。
fn parse_optional_int(cell: Option<&str>) -> Option<u32> {
    let normalized = cell.unwrap_or("").trim();
    if normalized.is_empty() {
        return None;
    }
    let m = digits_re().find(normalized)?;
    let value: u32 = m.as_str().parse().ok()?;
    if value > 0 { Some(value) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::runtime_state::HookPayoffTiming;

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

    // --- 解析子集 ---------------------------------------------------------------

    #[test]
    fn parse_markdown_table_rows_handles_trailing_pipe() {
        let md = "| 章节号 | 标题 |\n|---|---|\n| 1 | 初遇 |\n| 2 | 决意 |";
        let rows = parse_markdown_table_rows(md);
        // 表头行 + 2 数据行（分隔线被过滤）。
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec!["章节号".to_string(), "标题".to_string()]);
        assert_eq!(rows[1], vec!["1".to_string(), "初遇".to_string()]);
    }

    #[test]
    fn parse_markdown_table_rows_drops_empty_and_separator_rows() {
        let md = "| a | b |\n|---|---|\n|  |  |\n| x | y |";
        let rows = parse_markdown_table_rows(md);
        // 全空行被过滤；分隔线（含 ---）被过滤。
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1], vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn parse_chapter_summaries_markdown_filters_non_numeric_first_col() {
        let md = "| 章节号 | 标题 |\n|---|---|\n| 1 | t1 |\n| note | t2 |\n| 3 | t3 |";
        let summaries = parse_chapter_summaries_markdown(md);
        let chapters: Vec<i64> = summaries.iter().map(|s| s.chapter).collect();
        assert_eq!(chapters, vec![1, 3]);
        assert_eq!(summaries[0].title, "t1");
    }

    #[test]
    fn parse_chapter_summaries_markdown_maps_all_columns() {
        let md = "| 5 | 标题 | 角色 | 事件 | 状态变化 | 伏笔活动 | 心情 | 章节类型 |";
        let s = &parse_chapter_summaries_markdown(md)[0];
        assert_eq!(s.chapter, 5);
        assert_eq!(s.title, "标题");
        assert_eq!(s.characters, "角色");
        assert_eq!(s.events, "事件");
        assert_eq!(s.state_changes, "状态变化");
        assert_eq!(s.hook_activity, "伏笔活动");
        assert_eq!(s.mood, "心情");
        assert_eq!(s.chapter_type, "章节类型");
    }

    #[test]
    fn parse_pending_hooks_markdown_table_8_col_shape() {
        // 8 列（Phase 5/6）：id, ch, type, status, last_adv, expected, timing, notes
        let md = "| hook_id | ch | type | status | last_adv | expected | timing | notes |\n|---|---|---|---|---|---|---|---|\n| mentor-debt | 1 | relationship | open | 3 | Reveal the debt | mid-arc | unresolved |";
        let hooks = parse_pending_hooks_markdown(md);
        assert_eq!(hooks.len(), 1);
        let h = &hooks[0];
        assert_eq!(h.hook_id, "mentor-debt");
        assert_eq!(h.start_chapter, 1);
        assert_eq!(h.hook_type, "relationship");
        assert_eq!(h.status, HookStatus::Open);
        assert_eq!(h.last_advanced_chapter, 3);
        assert_eq!(h.expected_payoff, "Reveal the debt");
        assert_eq!(h.payoff_timing, Some(HookPayoffTiming::MidArc));
        assert_eq!(h.notes, "unresolved");
        // 非 phase7 行无元数据。
        assert_eq!(h.depends_on, None);
    }

    #[test]
    fn parse_pending_hooks_markdown_phase7_promoted_13_col() {
        // 13 列（Phase 7 hotfix 2）：id,ch,type,status,last_adv,expected,timing,
        //   depends_on,pays_off_in_arc,core_hook,half_life,promoted,notes
        let md = "| h01 | 1 | mystery | open | 5 | payoff | mid-arc | h02 | arc1 | ✓ | 30 | promoted | unresolved |";
        let h = &parse_pending_hooks_markdown(md)[0];
        assert_eq!(h.depends_on.as_deref(), Some(&["h02".to_string()][..]));
        assert_eq!(h.pays_off_in_arc.as_deref(), Some("arc1"));
        assert_eq!(h.core_hook, Some(true));
        assert_eq!(h.half_life_chapters, Some(30));
        assert_eq!(h.promoted, Some(true));
        // notes 是第 13 列（index 12）。
        assert_eq!(h.notes, "unresolved");
    }

    #[test]
    fn parse_pending_hooks_markdown_strips_markdown_id() {
        // hook id 含 markdown 包装 → normalize。
        let md = "| **bold-id** | 1 | mystery | open | 1 | x | mid-arc | n |";
        let h = &parse_pending_hooks_markdown(md)[0];
        assert_eq!(h.hook_id, "bold-id");
    }

    #[test]
    fn parse_pending_hooks_markdown_bullet_fallback() {
        // 无表格 → bullet 列表，notes-only，id=hook-N。
        let md = "- 第一条线索\n- 第二条线索";
        let hooks = parse_pending_hooks_markdown(md);
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0].hook_id, "hook-1");
        assert_eq!(hooks[0].notes, "第一条线索");
        assert_eq!(hooks[1].hook_id, "hook-2");
        assert_eq!(hooks[1].hook_type, "unspecified");
    }

    #[test]
    fn parse_pending_hooks_markdown_ignores_empty_id_rows() {
        // 首列经 normalize 为空（纯标点）→ 过滤。
        let md = "| --- | 1 | mystery | open | 1 | x | mid-arc | n |";
        let hooks = parse_pending_hooks_markdown(md);
        assert!(hooks.is_empty(), "空 id 行应被过滤");
    }

    #[test]
    fn parse_current_state_facts_table_with_chapter_row() {
        let md = "| 字段 | 值 |\n|---|---|\n| 当前章节 | 7 |\n| 当前位置 | 森林 |";
        let facts = parse_current_state_facts(md, 99);
        // 「当前章节」行用于定 stateChapter，本身不产 fact；「字段|值」表头被过滤。
        assert_eq!(facts.len(), 1);
        let f = &facts[0];
        assert_eq!(f.subject, "protagonist");
        assert_eq!(f.predicate, "当前位置");
        assert_eq!(f.object, "森林");
        assert_eq!(f.valid_from_chapter, 7);
        assert_eq!(f.source_chapter, 7);
    }

    #[test]
    fn parse_current_state_facts_falls_back_chapter_when_missing() {
        // 无「当前章节」行 → 用 fallback_chapter。
        let md = "| 当前目标 | 找到圣杯 |";
        let f = &parse_current_state_facts(md, 12)[0];
        assert_eq!(f.valid_from_chapter, 12);
    }

    #[test]
    fn parse_current_state_facts_bullet_fallback() {
        let md = "- 一条笔记\n- 另一条笔记";
        let facts = parse_current_state_facts(md, 3);
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].predicate, "note_1");
        assert_eq!(facts[0].subject, "current_state");
        assert_eq!(facts[0].object, "一条笔记");
        assert_eq!(facts[1].predicate, "note_2");
    }

    #[test]
    fn parse_current_state_facts_skips_empty_label_or_value() {
        let md = "|  | 有值 |\n| 有标签 |  |";
        let facts = parse_current_state_facts(md, 1);
        assert!(facts.is_empty(), "空 label 或空 value 的行应跳过");
    }

    #[test]
    fn is_state_table_header_row_recognizes_zh_and_en() {
        assert!(is_state_table_header_row(&["字段".to_string(), "值".to_string()]));
        assert!(is_state_table_header_row(&["field".to_string(), "value".to_string()]));
        assert!(!is_state_table_header_row(&["章节号".to_string(), "标题".to_string()]));
    }

    #[test]
    fn is_current_chapter_label_case_insensitive() {
        assert!(is_current_chapter_label("当前章节"));
        assert!(is_current_chapter_label("Current Chapter"));
        assert!(is_current_chapter_label("  current chapter  "));
        assert!(!is_current_chapter_label("章节"));
    }

    #[test]
    fn infer_fact_subject_maps_known_labels_to_protagonist() {
        assert_eq!(infer_fact_subject("当前位置"), "protagonist");
        assert_eq!(infer_fact_subject("current goal"), "protagonist");
        assert_eq!(infer_fact_subject("当前冲突"), "protagonist");
        assert_eq!(infer_fact_subject("其它标签"), "current_state");
    }

    #[test]
    fn parse_integer_extracts_first_digit_run() {
        assert_eq!(parse_integer(Some("42")), 42);
        assert_eq!(parse_integer(Some("第7章")), 7);
        assert_eq!(parse_integer(Some("abc")), 0);
        assert_eq!(parse_integer(None), 0);
        assert_eq!(parse_integer(Some("  ")), 0);
    }

    #[test]
    fn parse_strict_chapter_integer_rejects_narrative_numbers() {
        // 「第141号文明」不应被误读为 141（normalizeHookId 不改变它，非纯数字 → 0）。
        assert_eq!(parse_strict_chapter_integer(Some("12")), 12);
        assert_eq!(parse_strict_chapter_integer(Some("第141号文明")), 0);
        assert_eq!(parse_strict_chapter_integer(Some("12章")), 0);
        assert_eq!(parse_strict_chapter_integer(None), 0);
    }

    #[test]
    fn parse_depends_on_handles_brackets_and_separators() {
        assert_eq!(parse_depends_on("[H01, H02]"), vec!["H01".to_string(), "H02".to_string()]);
        assert_eq!(parse_depends_on("H01/H02"), vec!["H01".to_string(), "H02".to_string()]);
        assert_eq!(parse_depends_on("H01、H02"), vec!["H01".to_string(), "H02".to_string()]);
        assert_eq!(parse_depends_on("none"), Vec::<String>::new());
        assert_eq!(parse_depends_on("无"), Vec::<String>::new());
        assert_eq!(parse_depends_on("-"), Vec::<String>::new());
        assert_eq!(parse_depends_on(""), Vec::<String>::new());
    }

    #[test]
    fn parse_optional_int_rejects_zero_and_non_numeric() {
        assert_eq!(parse_optional_int(Some("30")), Some(30));
        assert_eq!(parse_optional_int(Some("0")), None);
        assert_eq!(parse_optional_int(Some("abc")), None);
        assert_eq!(parse_optional_int(Some("")), None);
        assert_eq!(parse_optional_int(None), None);
    }

    #[test]
    fn parse_optional_boolean_cell_recognizes_promoted_vocab() {
        assert_eq!(parse_optional_boolean_cell(Some("promoted")), Some(true));
        assert_eq!(parse_optional_boolean_cell(Some("已升级")), Some(true));
        assert_eq!(parse_optional_boolean_cell(Some("seed")), Some(false));
        assert_eq!(parse_optional_boolean_cell(Some("未升级")), Some(false));
        assert_eq!(parse_optional_boolean_cell(Some("")), None);
        assert_eq!(parse_optional_boolean_cell(Some("???")), None);
    }

    #[test]
    fn parse_hook_status_normalizes_aliases() {
        assert_eq!(parse_hook_status("open".to_string()), HookStatus::Open);
        assert_eq!(parse_hook_status("Progressing".to_string()), HookStatus::Progressing);
        assert_eq!(parse_hook_status("deferred".to_string()), HookStatus::Deferred);
        assert_eq!(parse_hook_status("已解决".to_string()), HookStatus::Resolved);
        assert_eq!(parse_hook_status("closed".to_string()), HookStatus::Resolved);
        assert_eq!(parse_hook_status("".to_string()), HookStatus::Open);
        assert_eq!(parse_hook_status("unknown".to_string()), HookStatus::Open);
    }
}
