//! 伏笔生命周期策略常量。
//!
//! 移植自 `packages/core/src/utils/hook-policy.ts`（纯常量 + 1 函数）。
//! 依赖已移植的 HookPayoffTiming。

use crate::models::runtime_state::HookPayoffTiming;

/// hook 阶段（基于章节进度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase {
    Opening,
    Middle,
    Late,
}

/// hook 生命周期画像（按回收节奏）。
#[derive(Debug, Clone, Copy)]
pub struct HookLifecycleProfile {
    pub earliest_resolve_age: u32,
    pub stale_dormancy: u32,
    pub overdue_age: u32,
    pub minimum_phase: HookPhase,
    pub resolve_bias: u32,
}

/// 按 timing 的生命周期画像（移植 HOOK_TIMING_PROFILES）。
pub fn hook_timing_profile(timing: HookPayoffTiming) -> HookLifecycleProfile {
    match timing {
        HookPayoffTiming::Immediate => HookLifecycleProfile { earliest_resolve_age: 1, stale_dormancy: 1, overdue_age: 3, minimum_phase: HookPhase::Opening, resolve_bias: 5 },
        HookPayoffTiming::NearTerm => HookLifecycleProfile { earliest_resolve_age: 1, stale_dormancy: 2, overdue_age: 5, minimum_phase: HookPhase::Opening, resolve_bias: 4 },
        HookPayoffTiming::MidArc => HookLifecycleProfile { earliest_resolve_age: 2, stale_dormancy: 4, overdue_age: 8, minimum_phase: HookPhase::Opening, resolve_bias: 3 },
        HookPayoffTiming::SlowBurn => HookLifecycleProfile { earliest_resolve_age: 4, stale_dormancy: 5, overdue_age: 12, minimum_phase: HookPhase::Middle, resolve_bias: 2 },
        HookPayoffTiming::Endgame => HookLifecycleProfile { earliest_resolve_age: 6, stale_dormancy: 6, overdue_age: 16, minimum_phase: HookPhase::Late, resolve_bias: 1 },
    }
}

pub fn hook_phase_weight(phase: HookPhase) -> u32 {
    match phase {
        HookPhase::Opening => 0,
        HookPhase::Middle => 1,
        HookPhase::Late => 2,
    }
}

pub mod phase_thresholds {
    pub const MIDDLE_PROGRESS: f64 = 0.33;
    pub const LATE_PROGRESS: f64 = 0.72;
    pub const MIDDLE_CHAPTER: u32 = 8;
    pub const LATE_CHAPTER: u32 = 24;
}

pub mod pressure_weights {
    pub const STALE_ADVANCE_BONUS: u32 = 8;
    pub const OVERDUE_ADVANCE_BONUS: u32 = 6;
    pub const RESOLVE_BIAS_MULTIPLIER: u32 = 10;
    pub const PROGRESSING_RESOLVE_BONUS: u32 = 5;
    pub const DORMANCY_RESOLVE_MULTIPLIER: u32 = 2;
    pub const MAX_DORMANCY_RESOLVE_BONUS: u32 = 12;
    pub const OVERDUE_RESOLVE_BONUS: u32 = 10;
    pub const MUST_ADVANCE_PRESSURE_FLOOR: u32 = 8;
    pub const CRITICAL_RESOLVE_PRESSURE: u32 = 40;
}

pub mod activity_thresholds {
    pub const RECENTLY_TOUCHED_DORMANCY: u32 = 1;
    pub const LONG_ARC_QUIET_HOLD_MAX_AGE: u32 = 2;
    pub const LONG_ARC_QUIET_HOLD_MAX_DORMANCY: u32 = 1;
    pub const REFRESH_DORMANCY: u32 = 2;
    pub const FRESH_PROMISE_AGE: u32 = 1;
}

/// 按 timing 的可见窗口（章节数）。
pub fn hook_visibility_window(timing: HookPayoffTiming) -> u32 {
    match timing {
        HookPayoffTiming::Immediate | HookPayoffTiming::NearTerm => 5,
        HookPayoffTiming::MidArc => 6,
        HookPayoffTiming::SlowBurn => 8,
        HookPayoffTiming::Endgame => 10,
    }
}

/// 由章节进度解析阶段（移植 resolveHookPhase）。
pub fn resolve_hook_phase(chapter_number: u32, target_chapters: Option<u32>) -> HookPhase {
    if let Some(target) = target_chapters {
        if target > 0 {
            let progress = chapter_number as f64 / target as f64;
            if progress >= phase_thresholds::LATE_PROGRESS {
                return HookPhase::Late;
            }
            if progress >= phase_thresholds::MIDDLE_PROGRESS {
                return HookPhase::Middle;
            }
            return HookPhase::Opening;
        }
    }
    if chapter_number >= phase_thresholds::LATE_CHAPTER {
        HookPhase::Late
    } else if chapter_number >= phase_thresholds::MIDDLE_CHAPTER {
        HookPhase::Middle
    } else {
        HookPhase::Opening
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_match_ts() {
        let p = hook_timing_profile(HookPayoffTiming::Immediate);
        assert_eq!(p.earliest_resolve_age, 1);
        assert_eq!(p.overdue_age, 3);
        assert_eq!(p.resolve_bias, 5);
        let p2 = hook_timing_profile(HookPayoffTiming::Endgame);
        assert_eq!(p2.minimum_phase, HookPhase::Late);
        assert_eq!(p2.resolve_bias, 1);
    }

    #[test]
    fn phase_by_progress() {
        assert_eq!(resolve_hook_phase(5, Some(100)), HookPhase::Opening); // 5%
        assert_eq!(resolve_hook_phase(50, Some(100)), HookPhase::Middle); // 50%
        assert_eq!(resolve_hook_phase(80, Some(100)), HookPhase::Late); // 80%
    }

    #[test]
    fn phase_by_absolute_chapter() {
        assert_eq!(resolve_hook_phase(3, None), HookPhase::Opening);
        assert_eq!(resolve_hook_phase(10, None), HookPhase::Middle);
        assert_eq!(resolve_hook_phase(30, None), HookPhase::Late);
    }

    #[test]
    fn visibility_windows() {
        assert_eq!(hook_visibility_window(HookPayoffTiming::Immediate), 5);
        assert_eq!(hook_visibility_window(HookPayoffTiming::Endgame), 10);
    }
}
