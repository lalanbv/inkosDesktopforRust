//! 伏笔健康度分析（hook-health）。
//!
//! 移植自 `packages/core/src/utils/hook-health.ts`（192 行）。检查活跃伏笔的债务压力：
//! - 活跃数超 maxActiveHooks 上限
//! - 已进入压力区（stale/overdue/readyToResolve）但本章未推进/回收/延后
//! - 连续 noAdvanceWindow 章无真实推进
//! - newHookBurstThreshold 个新伏笔但无回收
//!
//! 全部产出 [`AuditIssue`]（warning 级）。

use std::collections::HashSet;

use crate::agents::continuity::{AuditIssue, AuditSeverity};
use crate::models::runtime_state::{HookPayoffTiming, HookRecord, HookStatus, RuntimeStateDelta};
use crate::utils::hook_governance::{classify_hook_disposition, collect_stale_hook_debt, HookDisposition};
use crate::utils::hook_lifecycle::{describe_hook_lifecycle, HookLifecycleDescription};
use crate::utils::hook_policy::HOOK_HEALTH_DEFAULTS;
use crate::utils::language::WritingLanguage;

/// hook 健康度分析输入。对齐 TS `analyzeHookHealth` 的 params。
pub struct HookHealthParams<'a> {
    pub language: WritingLanguage,
    pub chapter_number: u32,
    pub target_chapters: Option<u32>,
    pub hooks: &'a [HookRecord],
    pub delta: Option<&'a RuntimeStateDelta>,
    pub existing_hook_ids: Option<&'a [String]>,
    pub max_active_hooks: Option<u32>,
    pub stale_after_chapters: Option<u32>,
    pub no_advance_window: Option<u32>,
    pub new_hook_burst_threshold: Option<u32>,
}

/// 分析伏笔健康度，产出 warning 级 [`AuditIssue`] 列表。对齐 TS `analyzeHookHealth`。
pub fn analyze_hook_health(params: &HookHealthParams<'_>) -> Vec<AuditIssue> {
    let defaults = HOOK_HEALTH_DEFAULTS;
    let max_active_hooks = params.max_active_hooks.unwrap_or(defaults.max_active_hooks);
    let stale_after = params.stale_after_chapters.unwrap_or(defaults.stale_after_chapters);
    let no_advance_window = params.no_advance_window.unwrap_or(defaults.no_advance_window);
    let new_hook_burst = params.new_hook_burst_threshold.unwrap_or(defaults.new_hook_burst_threshold);

    let mut issues: Vec<AuditIssue> = Vec::new();

    // 活跃伏笔：非 resolved/deferred。
    let active_hooks: Vec<&HookRecord> = params
        .hooks
        .iter()
        .filter(|h| {
            let st = hook_status_str(h.status);
            let normalized = crate::utils::hook_lifecycle::normalize_stored_hook_status(&st);
            normalized != HookStatus::Resolved && normalized != HookStatus::Deferred
        })
        .collect();

    let lifecycle_entries: Vec<(&HookRecord, HookLifecycleDescription)> = active_hooks
        .iter()
        .map(|hook| {
            let lc = describe_hook_lifecycle(
                hook.payoff_timing.map(payoff_timing_str).as_deref(),
                Some(&hook.expected_payoff),
                Some(&hook.notes),
                hook.start_chapter,
                hook.last_advanced_chapter,
                hook_status_str(hook.status).as_str(),
                params.chapter_number,
                params.target_chapters,
            );
            (*hook, lc)
        })
        .collect();

    // 上限警告。
    if active_hooks.len() as u32 > max_active_hooks {
        issues.push(warning(
            params.language,
            if params.language == WritingLanguage::En {
                format!("There are {} active hooks, above the recommended cap of {}.", active_hooks.len(), max_active_hooks)
            } else {
                format!("当前有 {} 个活跃伏笔，已经高于建议上限 {} 个。", active_hooks.len(), max_active_hooks)
            },
            if params.language == WritingLanguage::En {
                "Prefer advancing, resolving, or deferring existing debt before opening more hooks.".to_string()
            } else {
                "优先推进、回收或延后已有伏笔，再继续开新伏笔。".to_string()
            },
        ));
    }

    // 压力区伏笔。
    let stale_hook_ids: HashSet<String> = collect_stale_hook_debt(
        &active_hooks.iter().copied().cloned().collect::<Vec<_>>(),
        params.chapter_number,
        params.target_chapters,
        Some(stale_after),
    )
    .iter()
    .map(|h| h.hook_id.clone())
    .collect();

    let pressured: Vec<(&HookRecord, &HookLifecycleDescription)> = lifecycle_entries
        .iter()
        .filter(|(hook, lc)| {
            stale_hook_ids.contains(&hook.hook_id) || lc.ready_to_resolve || lc.overdue
        })
        .map(|(h, lc)| (*h, lc))
        .collect();

    let unresolved: Vec<(&HookRecord, &HookLifecycleDescription)> = pressured
        .iter()
        .filter(|(hook, _)| {
            if let Some(delta) = params.delta {
                let disp = classify_hook_disposition(&hook.hook_id, delta);
                disp == HookDisposition::None || disp == HookDisposition::Mention
            } else {
                true
            }
        })
        .copied()
        .collect();

    if !unresolved.is_empty() {
        issues.push(warning(
            params.language,
            build_pressure_description(params.language, &unresolved, params.delta.is_some()),
            if params.language == WritingLanguage::En {
                "Move one pressured hook with a real payoff, escalation, or explicit defer before opening adjacent debt.".to_string()
            } else {
                "先让一个已进入压力区的伏笔发生真实推进、回收或明确延后，再继续扩展同类债务。".to_string()
            },
        ));
    } else if let Some(_window) = params.no_advance_window {
        // 无压力区未处理时，检查连续无推进。
        if !active_hooks.is_empty() {
            let latest_real_advance = active_hooks.iter().map(|h| h.last_advanced_chapter).max().unwrap_or(0);
            let gap = params.chapter_number.saturating_sub(latest_real_advance);
            if gap >= no_advance_window {
                issues.push(warning(
                    params.language,
                    if params.language == WritingLanguage::En {
                        format!("No real hook advancement has landed for {gap} chapters.")
                    } else {
                        format!("已经连续 {gap} 章没有真实伏笔推进。")
                    },
                    if params.language == WritingLanguage::En {
                        "Schedule one old hook for real movement instead of opening parallel restatements.".to_string()
                    } else {
                        "下一章优先让一个旧伏笔发生真实推进，而不是继续平行重述。".to_string()
                    },
                ));
            }
        }
    }

    // 新伏笔爆发但无回收。
    if let Some(delta) = params.delta {
        let existing: HashSet<&str> = params.existing_hook_ids.map(|ids| ids.iter().map(|s| s.as_str()).collect()).unwrap_or_default();
        let resulting: HashSet<&str> = params.hooks.iter().map(|h| h.hook_id.as_str()).collect();
        let new_count = delta
            .hook_ops
            .upsert
            .iter()
            .filter(|h| !existing.contains(h.hook_id.as_str()) && resulting.contains(h.hook_id.as_str()))
            .count();
        if new_count as u32 >= new_hook_burst && delta.hook_ops.resolve.is_empty() {
            issues.push(warning(
                params.language,
                if params.language == WritingLanguage::En {
                    format!("Opened {new_count} new hooks without resolving any older debt.")
                } else {
                    format!("本章新开了 {new_count} 个伏笔，但没有回收任何旧债。")
                },
                if params.language == WritingLanguage::En {
                    "Keep the hook table from ballooning by pairing new openings with old payoffs.".to_string()
                } else {
                    "控制伏笔膨胀，新开伏笔时尽量配套回收旧伏笔。".to_string()
                },
            ));
        }
    }

    issues
}

/// 压力区描述（top3 摘要 + 剩余计数）。对齐 TS `buildPressureDescription`。
fn build_pressure_description(
    language: WritingLanguage,
    entries: &[(&HookRecord, &HookLifecycleDescription)],
    mentions_current_chapter: bool,
) -> String {
    let en = language == WritingLanguage::En;
    let joiner = if en { ", " } else { "、" };
    let summarized: Vec<String> = entries
        .iter()
        .take(3)
        .map(|(hook, lc)| {
            let timing = crate::utils::hook_lifecycle::localize_hook_payoff_timing(lc.timing, language);
            let pressure = localize_pressure_label(lc, language);
            if en {
                format!("{} ({}, {})", hook.hook_id, timing, pressure)
            } else {
                format!("{}（{}，{}）", hook.hook_id, timing, pressure)
            }
        })
        .collect();
    let suffix = if entries.len() > summarized.len() {
        let more = entries.len() - summarized.len();
        if en { format!(", +{more} more") } else { format!("，另有 {more} 条") }
    } else {
        String::new()
    };
    let list = summarized.join(joiner);
    if en {
        if mentions_current_chapter {
            format!("Hooks are already under payoff pressure but this chapter left them untouched: {list}{suffix}.")
        } else {
            format!("Hooks are already under payoff pressure without recent movement: {list}{suffix}.")
        }
    } else if mentions_current_chapter {
        format!("这些伏笔已经进入回收/推进压力，但本章没有真正处理：{list}{suffix}。")
    } else {
        format!("这些伏笔已经进入回收/推进压力，但近期没有真实推进：{list}{suffix}。")
    }
}

/// 压力标签本地化。对齐 TS `localizePressureLabel`。
fn localize_pressure_label(lc: &HookLifecycleDescription, language: WritingLanguage) -> &'static str {
    if lc.overdue {
        return if language == WritingLanguage::En { "overdue" } else { "已逾期" };
    }
    if lc.ready_to_resolve {
        return if language == WritingLanguage::En { "ready to pay off" } else { "可回收" };
    }
    if language == WritingLanguage::En { "stale" } else { "陈旧" }
}

fn warning(language: WritingLanguage, description: String, suggestion: String) -> AuditIssue {
    AuditIssue {
        severity: AuditSeverity::Warning,
        category: if language == WritingLanguage::En { "Hook Debt" } else { "伏笔债务" }.to_string(),
        description,
        suggestion,
        repair_scope: None,
    }
}

/// HookStatus 枚举 → serde 名字符串（normalize_stored_hook_status 接收 &str）。
fn hook_status_str(status: HookStatus) -> String {
    match status {
        HookStatus::Open => "open".to_string(),
        HookStatus::Progressing => "progressing".to_string(),
        HookStatus::Deferred => "deferred".to_string(),
        HookStatus::Resolved => "resolved".to_string(),
    }
}

/// HookPayoffTiming 枚举 → serde 名字符串（describe_hook_lifecycle 的 payoff_timing 接收 Option<&str>）。
fn payoff_timing_str(timing: HookPayoffTiming) -> String {
    match timing {
        HookPayoffTiming::Immediate => "immediate".to_string(),
        HookPayoffTiming::NearTerm => "near-term".to_string(),
        HookPayoffTiming::MidArc => "mid-arc".to_string(),
        HookPayoffTiming::SlowBurn => "slow-burn".to_string(),
        HookPayoffTiming::Endgame => "endgame".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::runtime_state::{HookOps, HookStatus};

    fn hook(id: &str, start: u32, last_adv: u32, timing: Option<HookPayoffTiming>) -> HookRecord {
        HookRecord {
            hook_id: id.into(),
            start_chapter: start,
            hook_type: "mystery".into(),
            status: HookStatus::Open,
            last_advanced_chapter: last_adv,
            expected_payoff: "Reveal the hidden truth".into(),
            payoff_timing: timing,
            notes: "Still unresolved".into(),
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        }
    }

    fn params<'a>(language: WritingLanguage, chapter: u32, hooks: &'a [HookRecord]) -> HookHealthParams<'a> {
        HookHealthParams {
            language,
            chapter_number: chapter,
            target_chapters: Some(40),
            hooks,
            delta: None,
            existing_hook_ids: None,
            max_active_hooks: None,
            stale_after_chapters: None,
            no_advance_window: None,
            new_hook_burst_threshold: None,
        }
    }

    #[test]
    fn no_issues_when_few_active_hooks() {
        // 新埋伏笔（start=lastAdv=chapter=2），age 小，不在压力区。
        let hooks = vec![hook("h001", 2, 2, Some(HookPayoffTiming::MidArc))];
        let issues = analyze_hook_health(&params(WritingLanguage::Zh, 2, &hooks));
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn warns_when_active_hooks_exceed_cap() {
        // 13 个活跃 > max 12。
        let hooks: Vec<HookRecord> = (1..=13).map(|i| hook(&format!("h{i:03}"), 1, 1, Some(HookPayoffTiming::SlowBurn))).collect();
        let issues = analyze_hook_health(&params(WritingLanguage::Zh, 5, &hooks));
        assert!(issues.iter().any(|i| i.description.contains("高于建议上限")));
    }

    #[test]
    fn warns_when_no_advance_for_long_window() {
        // 1 个活跃 hook，lastAdvanced=1，当前 10 → gap=9 >= noAdvanceWindow(5)。
        let hooks = vec![hook("h001", 1, 1, Some(HookPayoffTiming::SlowBurn))];
        let mut p = params(WritingLanguage::Zh, 10, &hooks);
        p.no_advance_window = Some(5);
        let issues = analyze_hook_health(&p);
        assert!(issues.iter().any(|i| i.description.contains("连续 9 章没有真实伏笔推进")));
    }

    #[test]
    fn english_language_emits_english_messages() {
        let hooks: Vec<HookRecord> = (1..=13).map(|i| hook(&format!("h{i:03}"), 1, 1, Some(HookPayoffTiming::SlowBurn))).collect();
        let issues = analyze_hook_health(&params(WritingLanguage::En, 5, &hooks));
        assert!(issues.iter().any(|i| i.description.contains("above the recommended cap")));
        assert!(issues.iter().all(|i| i.category == "Hook Debt"));
    }

    #[test]
    fn new_hook_burst_without_resolve_warns() {
        // delta 含 2 个 upsert 新 hook，无 resolve。
        let existing: Vec<HookRecord> = vec![hook("h001", 1, 1, Some(HookPayoffTiming::MidArc))];
        // resulting hooks 含 h001 + h002 + h003（新）。
        let mut resulting = existing.clone();
        resulting.push(hook("h002", 5, 5, Some(HookPayoffTiming::MidArc)));
        resulting.push(hook("h003", 5, 5, Some(HookPayoffTiming::MidArc)));
        let upsert_new = vec![hook("h002", 5, 5, Some(HookPayoffTiming::MidArc)), hook("h003", 5, 5, Some(HookPayoffTiming::MidArc))];
        let delta = RuntimeStateDelta {
            chapter: 5,
            current_state_patch: None,
            hook_ops: HookOps { upsert: upsert_new, mention: vec![], resolve: vec![], defer: vec![] },
            new_hook_candidates: vec![],
            chapter_summary: None,
            subplot_ops: vec![],
            emotional_arc_ops: vec![],
            character_matrix_ops: vec![],
            notes: vec![],
        };
        let existing_ids = vec!["h001".to_string()];
        let p = HookHealthParams {
            language: WritingLanguage::Zh,
            chapter_number: 5,
            target_chapters: Some(40),
            hooks: &resulting,
            delta: Some(&delta),
            existing_hook_ids: Some(&existing_ids),
            max_active_hooks: None,
            stale_after_chapters: None,
            no_advance_window: None,
            new_hook_burst_threshold: None,
        };
        let issues = analyze_hook_health(&p);
        assert!(issues.iter().any(|i| i.description.contains("本章新开了 2 个伏笔，但没有回收任何旧债")));
    }

    #[test]
    fn resolved_and_deferred_hooks_excluded_from_active() {
        let mut h_resolved = hook("h001", 1, 1, Some(HookPayoffTiming::MidArc));
        h_resolved.status = HookStatus::Resolved;
        let mut h_deferred = hook("h002", 1, 1, Some(HookPayoffTiming::MidArc));
        h_deferred.status = HookStatus::Deferred;
        let hooks = vec![h_resolved, h_deferred];
        // 全部非活跃 → 无 issue（活跃数为 0）。
        let issues = analyze_hook_health(&params(WritingLanguage::Zh, 5, &hooks));
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn pressure_description_truncates_to_three_with_suffix() {
        // 构造 4 个 overdue hook（lastAdvanced=1, chapter=20, slow-burn overdue_age=12）。
        let hooks: Vec<HookRecord> = (1..=4).map(|i| hook(&format!("h{i:03}"), 1, 1, Some(HookPayoffTiming::SlowBurn))).collect();
        let lifecycle_pairs: Vec<(&HookRecord, HookLifecycleDescription)> = hooks
            .iter()
            .map(|h| {
                let lc = describe_hook_lifecycle(
                    Some("slow-burn"),
                    Some(&h.expected_payoff),
                    Some(&h.notes),
                    h.start_chapter,
                    h.last_advanced_chapter,
                    "open",
                    20,
                    Some(40),
                );
                (h, lc)
            })
            .collect();
        let unresolved: Vec<(&HookRecord, &HookLifecycleDescription)> = lifecycle_pairs.iter().map(|(h, lc)| (*h, lc)).collect();
        let desc = build_pressure_description(WritingLanguage::Zh, &unresolved, false);
        assert!(desc.contains("另有 1 条"), "应显示剩余计数: {desc}");
        assert!(desc.contains("h001"));
    }
}
