//! 状态投影渲染（hooks/summaries/currentState → markdown）。
//!
//! 移植自 `packages/core/src/state/state-projections.ts`（255 行，纯函数）。
//! 依赖：[`render_hook_diagnostic_marker`]（hook-stale-detection）+
//! [`resolve_hook_payoff_timing`]/[`localize_hook_payoff_timing`]（hook-lifecycle）+ 模型。

use crate::models::runtime_state::{
    ChapterSummariesState, CurrentStateFact, CurrentStateState, HooksState,
};
use crate::utils::hook_lifecycle::{localize_hook_payoff_timing, resolve_hook_payoff_timing};
use crate::utils::hook_stale_detection::{compute_hook_diagnostics, render_hook_diagnostic_marker};
use crate::utils::language::WritingLanguage;
use regex::Regex;
use std::sync::OnceLock;

fn note_index_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^note_(\d+)$").unwrap())
}

fn escape_table_cell(value: &str) -> String {
    value.replace('|', "\\|").trim().to_string()
}

// ---- hooks 投影 ----

pub fn render_hooks_projection(
    state: &HooksState,
    language: WritingLanguage,
    current_chapter: Option<u32>,
) -> String {
    let en = language == WritingLanguage::En;
    let title = if en { "# Pending Hooks" } else { "# 伏笔池" };
    let headers = if en {
        [
            "| hook_id | start_chapter | type | status | last_advanced_chapter | expected_payoff | payoff_timing | depends_on | pays_off_in_arc | core_hook | half_life | promoted | notes |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    } else {
        [
            "| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    };

    let diagnostics = current_chapter.map(|c| compute_hook_diagnostics(&state.hooks, c));

    let mut sorted: Vec<_> = state.hooks.iter().collect();
    sorted.sort_by(|a, b| {
        a.start_chapter
            .cmp(&b.start_chapter)
            .then_with(|| a.last_advanced_chapter.cmp(&b.last_advanced_chapter))
            .then_with(|| a.hook_id.cmp(&b.hook_id))
    });

    let mut rows: Vec<String> = Vec::new();
    for hook in sorted {
        let marker = diagnostics
            .as_ref()
            .and_then(|d| d.get(&hook.hook_id))
            .map(|diag| render_hook_diagnostic_marker(diag, language))
            .unwrap_or_default();
        let status_str = hook_status_str(hook.status);
        let status_cell = if marker.is_empty() {
            status_str.to_string()
        } else {
            format!("{} ({})", status_str, marker)
        };

        let timing = resolve_hook_payoff_timing(
            hook.payoff_timing.map(|t| match t {
                crate::models::runtime_state::HookPayoffTiming::Immediate => "immediate",
                crate::models::runtime_state::HookPayoffTiming::NearTerm => "near-term",
                crate::models::runtime_state::HookPayoffTiming::MidArc => "mid-arc",
                crate::models::runtime_state::HookPayoffTiming::SlowBurn => "slow-burn",
                crate::models::runtime_state::HookPayoffTiming::Endgame => "endgame",
            }),
            Some(&hook.expected_payoff),
            Some(&hook.notes),
        );
        let timing_label = localize_hook_payoff_timing(timing, language);

        let cells = [
            hook.hook_id.clone(),
            hook.start_chapter.to_string(),
            hook.hook_type.clone(),
            status_cell,
            hook.last_advanced_chapter.to_string(),
            hook.expected_payoff.clone(),
            timing_label.to_string(),
            render_depends_on_cell(hook.depends_on.as_deref(), language).to_string(),
            hook.pays_off_in_arc.clone().unwrap_or_default(),
            render_core_hook_cell(hook.core_hook == Some(true), language).to_string(),
            render_half_life_cell(hook.half_life_chapters).to_string(),
            render_promoted_cell(hook.promoted, language).to_string(),
            hook.notes.clone(),
        ];
        let escaped: Vec<String> = cells.iter().map(|c| escape_table_cell(c)).collect();
        rows.push(format!("| {} |", escaped.join(" | ")));
    }

    let mut out = vec![title.to_string(), String::new()];
    out.extend(headers.iter().map(|s| s.to_string()));
    out.extend(rows);
    out.push(String::new());
    out.join("\n")
}

fn hook_status_str(s: crate::models::runtime_state::HookStatus) -> &'static str {
    use crate::models::runtime_state::HookStatus;
    match s {
        HookStatus::Open => "open",
        HookStatus::Progressing => "progressing",
        HookStatus::Deferred => "deferred",
        HookStatus::Resolved => "resolved",
    }
}

fn render_depends_on_cell(ids: Option<&[String]>, language: WritingLanguage) -> String {
    let empty = language == WritingLanguage::En;
    match ids {
        None | Some([]) => if empty { "none" } else { "无" }.to_string(),
        Some(list) => format!("[{}]", list.join(", ")),
    }
}

fn render_core_hook_cell(is_core: bool, language: WritingLanguage) -> String {
    if language == WritingLanguage::En {
        if is_core { "true" } else { "false" }.into()
    } else if is_core {
        "是".into()
    } else {
        "否".into()
    }
}

fn render_half_life_cell(value: Option<u32>) -> String {
    match value {
        Some(v) if v > 0 => v.to_string(),
        _ => String::new(),
    }
}

fn render_promoted_cell(value: Option<bool>, language: WritingLanguage) -> String {
    match value {
        None => String::new(),
        Some(v) => {
            if language == WritingLanguage::En {
                if v { "true" } else { "false" }.into()
            } else if v {
                "是".into()
            } else {
                "否".into()
            }
        }
    }
}

// ---- chapter summaries 投影 ----

pub fn render_chapter_summaries_projection(
    state: &ChapterSummariesState,
    language: WritingLanguage,
) -> String {
    let en = language == WritingLanguage::En;
    let title = if en { "# Chapter Summaries" } else { "# 章节摘要" };
    // R2/359 号：末尾增冲突强度/揭示强度两列；缺分（旧章/旧书）渲染空单元格。
    let headers = if en {
        [
            "| Chapter | Title | Characters | Key Events | State Changes | Hook Activity | Mood | Chapter Type | Conflict | Reveal |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    } else {
        [
            "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 | 冲突强度 | 揭示强度 |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    };

    let mut sorted: Vec<_> = state.rows.iter().collect();
    sorted.sort_by(|a, b| a.chapter.cmp(&b.chapter));

    let rows: Vec<String> = sorted
        .iter()
        .map(|s| {
            let cells = [
                s.chapter.to_string(),
                s.title.clone(),
                s.characters.clone(),
                s.events.clone(),
                s.state_changes.clone(),
                s.hook_activity.clone(),
                s.mood.clone(),
                s.chapter_type.clone(),
                s.conflict_level.map(|v| v.to_string()).unwrap_or_default(),
                s.reveal_level.map(|v| v.to_string()).unwrap_or_default(),
            ];
            let escaped: Vec<String> = cells.iter().map(|c| escape_table_cell(c)).collect();
            format!("| {} |", escaped.join(" | "))
        })
        .collect();

    let mut out = vec![title.to_string(), String::new()];
    out.extend(headers.iter().map(|s| s.to_string()));
    out.extend(rows);
    out.push(String::new());
    out.join("\n")
}

// ---- current state 投影 ----

pub fn render_current_state_projection(
    state: &CurrentStateState,
    language: WritingLanguage,
) -> String {
    let en = language == WritingLanguage::En;
    let (title, table_header, label_chapter, placeholder, additional_title) = if en {
        (
            "# Current State",
            "| Field | Value |",
            "Current Chapter",
            "(not set)",
            "## Additional State",
        )
    } else {
        (
            "# 当前状态",
            "| 字段 | 值 |",
            "当前章节",
            "（未设定）",
            "## 其他状态",
        )
    };

    // 槽位（label + aliases）
    struct Slot {
        label: &'static str,
        aliases: &'static [&'static str],
    }
    let labels = if en {
        (
            "Current Location", "Protagonist State", "Current Goal",
            "Current Constraint", "Current Alliances", "Current Conflict",
        )
    } else {
        (
            "当前位置", "主角状态", "当前目标", "当前限制", "当前敌我", "当前冲突",
        )
    };
    let slots: [Slot; 6] = [
        Slot { label: labels.0, aliases: &["Current Location", "当前位置"] },
        Slot { label: labels.1, aliases: &["Protagonist State", "主角状态"] },
        Slot { label: labels.2, aliases: &["Current Goal", "当前目标"] },
        Slot { label: labels.3, aliases: &["Current Constraint", "当前限制"] },
        Slot { label: labels.4, aliases: &["Current Alliances", "Current Relationships", "当前敌我"] },
        Slot { label: labels.5, aliases: &["Current Conflict", "当前冲突"] },
    ];

    let known_predicates: std::collections::HashSet<String> = slots
        .iter()
        .flat_map(|s| s.aliases.iter().map(|a| normalize_predicate(a)))
        .collect();

    let mut lines: Vec<String> = vec![
        title.into(),
        String::new(),
        table_header.into(),
        "| --- | --- |".into(),
        format!("| {} | {} |", label_chapter, escape_table_cell(&state.chapter.to_string())),
    ];
    for slot in &slots {
        let value = find_fact_value(state, slot.aliases).unwrap_or_else(|| placeholder.into());
        lines.push(format!("| {} | {} |", slot.label, escape_table_cell(&value)));
    }

    let mut additional: Vec<&CurrentStateFact> = state
        .facts
        .iter()
        .filter(|f| !known_predicates.contains(&normalize_predicate(&f.predicate)))
        .collect();
    additional.sort_by(|a, b| compare_additional_facts(&a.predicate, &b.predicate));

    if additional.is_empty() {
        lines.push(String::new());
        return lines.join("\n");
    }

    lines.push(String::new());
    lines.push(additional_title.into());
    for fact in additional {
        lines.push(render_additional_fact(&fact.predicate, &fact.object));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn find_fact_value(state: &CurrentStateState, aliases: &[&str]) -> Option<String> {
    let alias_set: std::collections::HashSet<String> =
        aliases.iter().map(|a| normalize_predicate(a)).collect();
    state
        .facts
        .iter()
        .find(|f| alias_set.contains(&normalize_predicate(&f.predicate)))
        .map(|f| f.object.clone())
}

fn render_additional_fact(predicate: &str, object: &str) -> String {
    if note_index_re().is_match(predicate) {
        format!("- {}", object)
    } else {
        format!("- {}: {}", predicate, object)
    }
}

fn compare_additional_facts(left: &str, right: &str) -> std::cmp::Ordering {
    let lc = note_index_re().captures(left);
    let rc = note_index_re().captures(right);
    match (lc, rc) {
        (Some(l), Some(r)) => {
            let ln: u32 = l.get(1).unwrap().as_str().parse().unwrap_or(0);
            let rn: u32 = r.get(1).unwrap().as_str().parse().unwrap_or(0);
            ln.cmp(&rn)
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.cmp(right),
    }
}

fn normalize_predicate(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::runtime_state::{
        ChapterSummaryRow, HookRecord, HookStatus, HooksState,
    };

    fn hook(id: &str) -> HookRecord {
        HookRecord {
            kind: None,
            hook_id: id.into(),
            start_chapter: 1,
            hook_type: "plot".into(),
            status: HookStatus::Open,
            status_raw: String::new(),
            last_advanced_chapter: 0,
            expected_payoff: "soon".into(),
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
    fn hooks_projection_zh_headers() {
        let state = HooksState { hooks: vec![hook("h1")] };
        let md = render_hooks_projection(&state, WritingLanguage::Zh, None);
        assert!(md.contains("# 伏笔池"));
        assert!(md.contains("| 起始章节 |"));
        assert!(md.contains("h1"));
    }

    #[test]
    fn hooks_projection_en_with_diagnostics() {
        let state = HooksState { hooks: vec![hook("h1")] };
        let md = render_hooks_projection(&state, WritingLanguage::En, Some(50));
        // distance 49 > halfLife 30 → stale marker
        assert!(md.contains("stale"));
    }

    #[test]
    fn chapter_summaries_projection_sorted() {
        let state = ChapterSummariesState {
            rows: vec![
                ChapterSummaryRow { chapter: 5, title: "five".into(), ..Default::default() },
                ChapterSummaryRow { chapter: 2, title: "two".into(), ..Default::default() },
            ],
        };
        let md = render_chapter_summaries_projection(&state, WritingLanguage::Zh);
        let two_pos = md.find("two").unwrap();
        let five_pos = md.find("five").unwrap();
        assert!(two_pos < five_pos); // 升序
    }

    #[test]
    fn current_state_renders_known_slots_and_additional() {
        let state = CurrentStateState {
            chapter: 7,
            facts: vec![
                CurrentStateFact {
                    subject: "p".into(), predicate: "当前位置".into(), object: "图书馆".into(),
                    valid_from_chapter: 1, valid_until_chapter: None, source_chapter: 1,
                },
                CurrentStateFact {
                    subject: "p".into(), predicate: "note_1".into(), object: "随笔".into(),
                    valid_from_chapter: 1, valid_until_chapter: None, source_chapter: 1,
                },
            ],
        };
        let md = render_current_state_projection(&state, WritingLanguage::Zh);
        assert!(md.contains("图书馆"));
        assert!(md.contains("## 其他状态"));
        assert!(md.contains("- 随笔")); // note_1 → 无前缀
    }

    #[test]
    fn escape_table_cell_pipes() {
        assert_eq!(escape_table_cell("a|b"), "a\\|b");
    }

    #[test]
    fn additional_fact_note_vs_named() {
        assert_eq!(render_additional_fact("note_3", "x"), "- x");
        assert_eq!(render_additional_fact("目标", "活下去"), "- 目标: 活下去");
    }

    #[test]
    fn compare_additional_facts_note_ordering() {
        use std::cmp::Ordering;
        assert_eq!(compare_additional_facts("note_2", "note_10"), Ordering::Less); // 2 < 10
        assert_eq!(compare_additional_facts("note_1", "目标"), Ordering::Less); // note 先
        assert_eq!(compare_additional_facts("b", "a"), Ordering::Greater);
    }
}
