//! 伏笔生命周期（状态规范化 + 回收节奏推断 + 健康度/压力计算）。
//!
//! 移植自 `packages/core/src/utils/hook-lifecycle.ts`（230 行，纯函数）。
//! 依赖已移植的 [`crate::utils::hook_policy`] + 用 [`HookRecord`] 替代 StoredHook
//! （字段兼容：status/promoted/lastAdvancedChapter/startChapter）。

use crate::models::runtime_state::{HookPayoffTiming, HookRecord, HookStatus};
use crate::utils::hook_policy::{
    activity_thresholds, hook_phase_weight, hook_timing_profile, pressure_weights, resolve_hook_phase, HookPhase,
};
use regex::Regex;
use std::sync::OnceLock;

pub const DEFAULT_HOOK_LOOKAHEAD_CHAPTERS: u32 = 3;

fn status_resolved_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(resolved|closed|done|已回收|已解决)$").unwrap())
}
fn status_deferred_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(deferred|paused|hold|dormant|sleeping|延后|延期|搁置|暂缓|未开启|待开启|未启动|待启动|待推进)$").unwrap())
}
fn status_progressing_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(progressing|advanced|重大推进|持续推进)$").unwrap())
}

/// 规范化 hook 状态字符串 → HookStatus。默认 open。
pub fn normalize_stored_hook_status(status: &str) -> HookStatus {
    let s = status.trim();
    if status_resolved_re().is_match(s) {
        return HookStatus::Resolved;
    }
    if status_deferred_re().is_match(s) {
        return HookStatus::Deferred;
    }
    if status_progressing_re().is_match(s) {
        return HookStatus::Progressing;
    }
    HookStatus::Open
}

/// 过滤活跃 hook（非 resolved/deferred，且 promoted !== false）。
pub fn filter_active_hooks(hooks: &[HookRecord]) -> Vec<HookRecord> {
    hooks
        .iter()
        .filter(|h| {
            // HookRecord.status 已是强类型 HookStatus，直接按变体判断
            let active = !matches!(h.status, HookStatus::Resolved | HookStatus::Deferred);
            active && h.promoted != Some(false)
        })
        .cloned()
        .collect()
}

/// 未来计划 hook（未推进 + 起点远超当前章节+lookahead）。
pub fn is_future_planned_hook(hook: &HookRecord, chapter_number: u32, lookahead: u32) -> bool {
    hook.last_advanced_chapter == 0 && hook.start_chapter > chapter_number + lookahead
}

/// hook 是否在章节窗口内（最近推进 / 起点在窗口）。
pub fn is_hook_within_chapter_window(hook: &HookRecord, chapter_number: u32, recent_window: u32, lookahead: u32) -> bool {
    let recent_cutoff = chapter_number.saturating_sub(recent_window);
    if hook.last_advanced_chapter > 0 && hook.last_advanced_chapter >= recent_cutoff {
        return true;
    }
    if hook.last_advanced_chapter > 0 {
        return false;
    }
    if hook.start_chapter == 0 {
        return true;
    }
    if hook.start_chapter >= recent_cutoff && hook.start_chapter <= chapter_number {
        return true;
    }
    hook.start_chapter > chapter_number && hook.start_chapter <= chapter_number + lookahead
}

fn timing_alias_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // 合并 5 个 alias 模式（捕获 timing 由调用方按序匹配）。这里返回单个测试用：直接分别建。
        Regex::new(r"(?i)^(立即|马上|当章|本章|下一章|immediate|instant|next|right away)").unwrap()
    })
}

/// 规范化回收节奏字符串 → HookPayoffTiming。无法识别返回 None。
pub fn normalize_hook_payoff_timing(value: Option<&str>) -> Option<HookPayoffTiming> {
    let normalized = value?.trim();
    if normalized.is_empty() {
        return None;
    }
    // TIMING_ALIASES（按序匹配）
    let patterns: [(&str, &str); 5] = [
        ("immediate", r"(?i)^(立即|马上|当章|本章|下一章|immediate|instant|next(?:\s+chapter|\s+beat)?|right\s+away)$"),
        ("near-term", r"(?i)^(近期|近几章|短线|soon|short(?:\s+run)?|near(?:\s*-\s*|\s+)term|current\s+sequence)$"),
        ("mid-arc", r"(?i)^(中程|中期|卷中|mid(?:\s*-\s*|\s+)arc|mid(?:\s*-\s*|\s+)book|middle)$"),
        ("slow-burn", r"(?i)^(慢烧|长线|后续|later|late(?:r)?|long(?:\s*-\s*|\s+)arc|slow(?:\s*-\s*|\s+)burn)$"),
        ("endgame", r"(?i)^(终局|终章|大结局|最终|climax|finale|endgame|late\s+book)$"),
    ];
    for (timing, pat) in patterns {
        if Regex::new(pat).unwrap().is_match(normalized) {
            return Some(parse_timing(timing));
        }
    }
    None
}

fn parse_timing(s: &str) -> HookPayoffTiming {
    match s {
        "immediate" => HookPayoffTiming::Immediate,
        "near-term" => HookPayoffTiming::NearTerm,
        "mid-arc" => HookPayoffTiming::MidArc,
        "slow-burn" => HookPayoffTiming::SlowBurn,
        "endgame" => HookPayoffTiming::Endgame,
        _ => HookPayoffTiming::MidArc,
    }
}

/// 由 expectedPayoff + notes 推断节奏（信号词匹配）。默认 mid-arc。
pub fn infer_hook_payoff_timing(expected_payoff: Option<&str>, notes: Option<&str>) -> HookPayoffTiming {
    let parts: Vec<&str> = [expected_payoff.unwrap_or(""), notes.unwrap_or("")]
        .iter()
        .filter(|s| !s.trim().is_empty())
        .copied()
        .collect();
    if parts.is_empty() {
        return HookPayoffTiming::MidArc;
    }
    let combined = parts.join(" ");
    // SIGNAL_PATTERNS（按优先级：endgame 先）
    let signals: [(HookPayoffTiming, &str); 5] = [
        (HookPayoffTiming::Endgame, r"(?i)(终局|终章|大结局|最终揭晓|最终摊牌|climax|finale|endgame|final reveal|last act)"),
        (HookPayoffTiming::Immediate, r"(?i)(当章|本章|下一章|马上|立刻|即刻|immediate|next chapter|right away|at once)"),
        (HookPayoffTiming::NearTerm, r"(?i)(近期|近几章|很快|短线|soon|near-term|short run|current sequence)"),
        (HookPayoffTiming::MidArc, r"(?i)(中期|卷中|本卷中段|mid-book|mid arc|middle of the arc)"),
        (HookPayoffTiming::SlowBurn, r"(?i)(长线|慢烧|后续发酵|慢慢揭开|later|slow burn|long arc|long tail)"),
    ];
    for (timing, pat) in signals {
        if Regex::new(pat).unwrap().is_match(&combined) {
            return timing;
        }
    }
    HookPayoffTiming::MidArc
}

/// 解析回收节奏：显式 payoffTiming 优先，否则推断。
pub fn resolve_hook_payoff_timing(payoff_timing: Option<&str>, expected_payoff: Option<&str>, notes: Option<&str>) -> HookPayoffTiming {
    normalize_hook_payoff_timing(payoff_timing).unwrap_or_else(|| infer_hook_payoff_timing(expected_payoff, notes))
}

/// 本地化节奏标签。
pub fn localize_hook_payoff_timing(timing: HookPayoffTiming, language: crate::utils::language::WritingLanguage) -> &'static str {
    use crate::utils::language::WritingLanguage;
    match (timing, language) {
        (HookPayoffTiming::Immediate, WritingLanguage::En) => "immediate",
        (HookPayoffTiming::NearTerm, WritingLanguage::En) => "near-term",
        (HookPayoffTiming::MidArc, WritingLanguage::En) => "mid-arc",
        (HookPayoffTiming::SlowBurn, WritingLanguage::En) => "slow-burn",
        (HookPayoffTiming::Endgame, WritingLanguage::En) => "endgame",
        (HookPayoffTiming::Immediate, WritingLanguage::Zh) => "立即",
        (HookPayoffTiming::NearTerm, WritingLanguage::Zh) => "近期",
        (HookPayoffTiming::MidArc, WritingLanguage::Zh) => "中程",
        (HookPayoffTiming::SlowBurn, WritingLanguage::Zh) => "慢烧",
        (HookPayoffTiming::Endgame, WritingLanguage::Zh) => "终局",
    }
}

/// hook 生命周期描述（timing/phase/age/dormancy/readyToResolve/stale/overdue/advancePressure/resolvePressure）。
#[derive(Debug, Clone)]
pub struct HookLifecycleDescription {
    pub timing: HookPayoffTiming,
    pub phase: HookPhase,
    pub age: u32,
    pub dormancy: u32,
    pub ready_to_resolve: bool,
    pub stale: bool,
    pub overdue: bool,
    pub advance_pressure: u32,
    pub resolve_pressure: u32,
}

/// 计算 hook 生命周期描述。
#[allow(clippy::too_many_arguments)]
pub fn describe_hook_lifecycle(
    payoff_timing: Option<&str>,
    expected_payoff: Option<&str>,
    notes: Option<&str>,
    start_chapter: u32,
    last_advanced_chapter: u32,
    status: &str,
    chapter_number: u32,
    target_chapters: Option<u32>,
) -> HookLifecycleDescription {
    let timing = resolve_hook_payoff_timing(payoff_timing, expected_payoff, notes);
    let profile = hook_timing_profile(timing);
    let phase = resolve_hook_phase(chapter_number, target_chapters);
    let age = chapter_number.saturating_sub(std::cmp::max(1, start_chapter));
    let last_touch = std::cmp::max(start_chapter, last_advanced_chapter);
    let dormancy = chapter_number.saturating_sub(std::cmp::max(1, last_touch));
    let explicit_progressing = status_progressing_re().is_match(status.trim());
    let phase_ready = hook_phase_weight(phase) >= hook_phase_weight(profile.minimum_phase);
    let recently_touched = dormancy <= activity_thresholds::RECENTLY_TOUCHED_DORMANCY;
    let overdue = phase_ready && age >= profile.overdue_age;
    let cadence_ready = match timing {
        HookPayoffTiming::SlowBurn => phase == HookPhase::Late || overdue,
        HookPayoffTiming::Endgame => phase == HookPhase::Late,
        _ => true,
    };
    let momentum = explicit_progressing || recently_touched;
    let stale = phase_ready && (dormancy >= profile.stale_dormancy || (overdue && !momentum));
    let ready_to_resolve = phase_ready
        && cadence_ready
        && age >= profile.earliest_resolve_age
        && (momentum || (overdue && explicit_progressing));

    let advance_pressure = age
        + dormancy
        + if stale { pressure_weights::STALE_ADVANCE_BONUS } else { 0 }
        + if overdue { pressure_weights::OVERDUE_ADVANCE_BONUS } else { 0 };

    let resolve_pressure = if ready_to_resolve {
        profile.resolve_bias * pressure_weights::RESOLVE_BIAS_MULTIPLIER
        + if explicit_progressing { pressure_weights::PROGRESSING_RESOLVE_BONUS } else { 0 }
        + std::cmp::min(pressure_weights::MAX_DORMANCY_RESOLVE_BONUS, dormancy * pressure_weights::DORMANCY_RESOLVE_MULTIPLIER)
        + if overdue { pressure_weights::OVERDUE_RESOLVE_BONUS } else { 0 }
    } else {
        0
    };

    HookLifecycleDescription { timing, phase, age, dormancy, ready_to_resolve, stale, overdue, advance_pressure, resolve_pressure }
}

// 触发未用 import 警告规避
#[allow(dead_code)]
fn _ensure_timing_alias_used() {
    let _ = timing_alias_re();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_status() {
        assert_eq!(normalize_stored_hook_status("resolved"), HookStatus::Resolved);
        assert_eq!(normalize_stored_hook_status("已回收"), HookStatus::Resolved);
        assert_eq!(normalize_stored_hook_status("deferred"), HookStatus::Deferred);
        assert_eq!(normalize_stored_hook_status("progressing"), HookStatus::Progressing);
        assert_eq!(normalize_stored_hook_status("unknown"), HookStatus::Open);
    }

    #[test]
    fn normalize_payoff_timing_aliases() {
        assert_eq!(normalize_hook_payoff_timing(Some("立即")), Some(HookPayoffTiming::Immediate));
        assert_eq!(normalize_hook_payoff_timing(Some("near-term")), Some(HookPayoffTiming::NearTerm));
        assert_eq!(normalize_hook_payoff_timing(Some("endgame")), Some(HookPayoffTiming::Endgame));
        assert_eq!(normalize_hook_payoff_timing(Some("bogus")), None);
        assert_eq!(normalize_hook_payoff_timing(None), None);
    }

    #[test]
    fn infer_from_signal() {
        assert_eq!(infer_hook_payoff_timing(Some("本章揭晓"), None), HookPayoffTiming::Immediate);
        assert_eq!(infer_hook_payoff_timing(Some(""), Some("终局摊牌")), HookPayoffTiming::Endgame);
        assert_eq!(infer_hook_payoff_timing(None, None), HookPayoffTiming::MidArc);
    }

    #[test]
    fn resolve_prefers_explicit() {
        assert_eq!(resolve_hook_payoff_timing(Some("endgame"), Some("本章"), None), HookPayoffTiming::Endgame);
        assert_eq!(resolve_hook_payoff_timing(None, Some("本章"), None), HookPayoffTiming::Immediate);
    }

    #[test]
    fn localize() {
        assert_eq!(localize_hook_payoff_timing(HookPayoffTiming::Immediate, crate::utils::language::WritingLanguage::Zh), "立即");
        assert_eq!(localize_hook_payoff_timing(HookPayoffTiming::Endgame, crate::utils::language::WritingLanguage::En), "endgame");
    }

    #[test]
    fn describe_overdue_old_hook() {
        // immediate hook，age 大，dormancy 大 → overdue + stale
        let d = describe_hook_lifecycle(Some("immediate"), None, None, 1, 1, "open", 10, None);
        assert!(d.overdue); // age 9 >= overdueAge 3
        assert!(d.stale);
        assert!(d.advance_pressure > 0);
    }

    #[test]
    fn describe_fresh_hook_not_ready() {
        let d = describe_hook_lifecycle(None, Some("长线慢烧"), None, 1, 1, "open", 2, None);
        // slow-burn minimumPhase=middle，chapter 2 → opening → phase_ready false → not ready
        assert!(!d.ready_to_resolve);
    }
}
