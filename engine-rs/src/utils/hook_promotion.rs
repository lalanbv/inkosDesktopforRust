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
