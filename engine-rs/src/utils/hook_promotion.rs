//! 伏笔晋升（半衰期推导子集）。
//!
//! 移植自 `packages/core/src/utils/hook-promotion.ts` 中
//! `defaultHalfLifeChapters` + `resolveHalfLifeChapters` 两函数。
//! （晋升 pass / escapeRegex / 中文数字解析等属于 consolidator/runner 运行时，后续阶段移植。）
//!
//! 半衰期默认值（对齐 prompt 默认）：
//! - immediate / near-term = 10
//! - mid-arc（含未知）= 30
//! - slow-burn / endgame = 80

use crate::models::runtime_state::{HookPayoffTiming, HookRecord};

pub fn default_half_life_chapters(timing: Option<HookPayoffTiming>) -> u32 {
    match timing {
        Some(HookPayoffTiming::Immediate) | Some(HookPayoffTiming::NearTerm) => 10,
        Some(HookPayoffTiming::SlowBurn) | Some(HookPayoffTiming::Endgame) => 80,
        Some(HookPayoffTiming::MidArc) | None => 30,
    }
}

/// 解析半衰期：显式 halfLifeChapters 优先，否则按 timing 默认。
pub fn resolve_half_life_chapters(hook: &HookRecord) -> u32 {
    hook.half_life_chapters.unwrap_or_else(|| default_half_life_chapters(hook.payoff_timing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_by_timing() {
        assert_eq!(default_half_life_chapters(Some(HookPayoffTiming::Immediate)), 10);
        assert_eq!(default_half_life_chapters(Some(HookPayoffTiming::NearTerm)), 10);
        assert_eq!(default_half_life_chapters(Some(HookPayoffTiming::MidArc)), 30);
        assert_eq!(default_half_life_chapters(Some(HookPayoffTiming::SlowBurn)), 80);
        assert_eq!(default_half_life_chapters(Some(HookPayoffTiming::Endgame)), 80);
        assert_eq!(default_half_life_chapters(None), 30);
    }
}

// ---- 轻量晋级通道（rerunPromotionPass，42 号 write-next 消费） ----

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// 晋级通道结果。对齐 TS `PromotionPassResult`。
#[derive(Debug, Clone, PartialEq)]
pub struct PromotionPassResult {
    pub updated: bool,
    pub hooks: Vec<HookRecord>,
    pub flipped_count: usize,
}

/// 轻量晋级：读 hooks + chapter_summaries，`advancedCount >= 2` 翻 promoted。
/// 零 LLM 调用；不做 I/O——调用方决定是否持久化。
pub fn rerun_promotion_pass(
    hooks: &[HookRecord],
    summaries_raw: &str,
) -> PromotionPassResult {
    if hooks.is_empty() {
        return PromotionPassResult { updated: false, hooks: hooks.to_vec(), flipped_count: 0 };
    }

    let ids: Vec<&str> = hooks.iter().map(|h| h.hook_id.as_str()).collect();
    let derived_counts = derive_advanced_counts_from_summaries(summaries_raw, &ids);

    let mut flipped = 0usize;
    let next_hooks: Vec<HookRecord> = hooks
        .iter()
        .map(|hook| {
            if hook.promoted == Some(true) {
                return hook.clone();
            }
            let advanced = hook
                .advanced_count
                .or_else(|| derived_counts.get(hook.hook_id.as_str()).copied())
                .unwrap_or(0);
            if advanced >= 2 {
                flipped += 1;
                let mut promoted = hook.clone();
                promoted.promoted = Some(true);
                promoted
            } else {
                hook.clone()
            }
        })
        .collect();

    PromotionPassResult {
        updated: flipped > 0,
        hooks: next_hooks,
        flipped_count: flipped,
    }
}

/// 从章节摘要表推导各 hook 的推进计数（仅 hookActivity 列匹配，词边界）。
pub fn derive_advanced_counts_from_summaries(
    summaries_raw: &str,
    hook_ids: &[&str],
) -> HashMap<String, u32> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    if summaries_raw.trim().is_empty() || hook_ids.is_empty() {
        return counts;
    }

    let lines: Vec<&str> = summaries_raw.split('\n').collect();
    let hook_activity_index = detect_hook_activity_column_index(&lines);

    for hook_id in hook_ids {
        let escaped = escape_regex(hook_id);
        let pattern = Regex::new(&format!(r"(?i)\b{escaped}\b")).expect("hook id 正则应合法");
        let mut count = 0u32;
        for line in &lines {
            if !line.starts_with('|') {
                continue;
            }
            // 跳过表头/分隔行。
            if line.contains("---") || header_row_re().is_match(line) {
                continue;
            }
            let cell = extract_column(line, hook_activity_index);
            if let Some(cell) = cell {
                if pattern.is_match(&cell) {
                    count += 1;
                }
            }
        }
        if count > 0 {
            counts.insert((*hook_id).to_string(), count);
        }
    }
    counts
}

/// hookActivity / 伏笔动态 列的 0 基下标；缺失回退 5（schema 标准位）。
fn detect_hook_activity_column_index(lines: &[&str]) -> usize {
    const DEFAULT_INDEX: usize = 5;
    for line in lines {
        if !line.starts_with('|') {
            continue;
        }
        if header_row_re().is_match(line) {
            let cols: Vec<&str> = line.split('|').map(|c| c.trim()).collect();
            if let Some(position) = cols
                .iter()
                .position(|c| activity_header_re().is_match(c))
            {
                return position;
            }
            return DEFAULT_INDEX;
        }
    }
    DEFAULT_INDEX
}

fn extract_column(row: &str, index: usize) -> Option<String> {
    let cols: Vec<&str> = row.split('|').collect();
    cols.get(index).map(|c| c.trim().to_string())
}

fn escape_regex(value: &str) -> String {
    escape_re().replace_all(value, r"\$0").into_owned()
}

fn escape_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[.*+?^${}()|\[\]\\]").unwrap())
}

fn header_row_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\|\s*(章节|Chapter)\s*\|").unwrap())
}

fn activity_header_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(伏笔动态|hookActivity)$").unwrap())
}

#[cfg(test)]
mod promotion_tests {
    use super::*;
    use crate::models::runtime_state::HookStatus;

    fn hook(id: &str, advanced: Option<u32>, promoted: Option<bool>) -> HookRecord {
        HookRecord {
            hook_id: id.into(),
            start_chapter: 1,
            hook_type: "plot".into(),
            status: HookStatus::Open,
            status_raw: String::new(),
            last_advanced_chapter: 1,
            expected_payoff: String::new(),
            payoff_timing: None,
            notes: String::new(),
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: advanced,
            promoted,
        }
    }

    const SUMMARIES: &str = "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n| 1 | a | 甲 | e | s | H01 推进 | m | t |\n| 2 | b | 乙 | e | s | H01 推进、H02 open | m | t |\n| 3 | c | 丙 | e | s | H02 open | m | t |\n";

    #[test]
    fn derives_counts_from_activity_column_only() {
        let counts = derive_advanced_counts_from_summaries(SUMMARIES, &["H01", "H02", "H03"]);
        assert_eq!(counts.get("H01"), Some(&2));
        assert_eq!(counts.get("H02"), Some(&2));
        assert!(!counts.contains_key("H03"));
        // 关键事件列含 H01 的行不计（仅 hookActivity 列）。
        let tricky = "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 |\n| --- |\n| 1 | a | H01 | e | s | 无 | m | t |\n";
        let counts = derive_advanced_counts_from_summaries(tricky, &["H01"]);
        assert!(!counts.contains_key("H01"));
    }

    #[test]
    fn promotion_flips_at_two_advances() {
        let hooks = vec![
            hook("H01", None, None),  // 摘要推导 = 2 → 翻
            hook("H02", Some(2), None), // 显式 advancedCount = 2 → 翻
            hook("H03", Some(1), None), // 1 → 不翻
            hook("H04", None, Some(true)), // 已晋级 → 原样
        ];
        let result = rerun_promotion_pass(&hooks, SUMMARIES);
        assert!(result.updated);
        assert_eq!(result.flipped_count, 2);
        assert_eq!(result.hooks[0].promoted, Some(true));
        assert_eq!(result.hooks[2].promoted, None);
        assert_eq!(result.hooks[3].promoted, Some(true));

        let empty = rerun_promotion_pass(&[], "");
        assert!(!empty.updated);
    }
}
