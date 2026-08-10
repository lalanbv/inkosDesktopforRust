//! 节奏压力策略。
//!
//! 移植自 `packages/core/src/utils/cadence-policy.ts`（46 行，纯常量 + 1 函数）。
//! 节奏分析（scene/mood/title 重复压力）使用的阈值与判定函数。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 节奏压力等级。对齐 TS `"medium" | "high" | undefined`（undefined 由 Option::None 表达）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"medium\" | \"high\""))]
pub enum CadencePressure {
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
}

/// 滑窗默认值。逐字移植 TS `CADENCE_WINDOW_DEFAULTS`。
pub mod window_defaults {
    pub const SUMMARY_LOOKBACK: u32 = 4;
    pub const ENGLISH_VARIANCE_LOOKBACK: u32 = 24;
    pub const RECENT_BOUNDARY_PATTERN_BODIES: u32 = 2;
}

/// 场景/情绪/标题重复压力阈值。逐字移植 TS `CADENCE_PRESSURE_THRESHOLDS`。
pub mod pressure_thresholds {
    pub mod scene {
        pub const HIGH_COUNT: u32 = 3;
        pub const MEDIUM_COUNT: u32 = 2;
        pub const MEDIUM_WINDOW_FLOOR: u32 = 4;
    }
    pub mod mood {
        pub const HIGH_COUNT: u32 = 3;
        pub const MEDIUM_COUNT: u32 = 2;
        pub const MEDIUM_WINDOW_FLOOR: u32 = 4;
    }
    pub mod title {
        pub const MINIMUM_REPEATED_COUNT: u32 = 2;
        pub const HIGH_COUNT: u32 = 3;
        pub const MEDIUM_COUNT: u32 = 2;
        pub const MEDIUM_WINDOW_FLOOR: u32 = 4;
    }
}

/// 长跨度疲劳阈值（移植自 TS `LONG_SPAN_FATIGUE_THRESHOLDS`，供 long-span-fatigue 域用）。
pub mod long_span_fatigue_thresholds {
    pub const BOUNDARY_SIMILARITY_FLOOR: f64 = 0.72;
    pub const BOUNDARY_SENTENCE_MIN_LENGTH: u32 = 18;
    pub const BOUNDARY_PATTERN_MIN_BODIES: u32 = 3;
}

/// 节奏压力判定参数。
#[derive(Debug, Clone, Copy)]
pub struct CadencePressureParams {
    pub count: u32,
    pub total: u32,
    pub high_threshold: u32,
    pub medium_threshold: u32,
    pub medium_window_floor: u32,
}

/// 依重复次数与窗口总量判定压力等级。
///
/// - count ≥ high_threshold → `High`
/// - count ≥ medium_threshold 且 total ≥ medium_window_floor → `Medium`
/// - 否则 → `None`
pub fn resolve_cadence_pressure(p: CadencePressureParams) -> Option<CadencePressure> {
    if p.count >= p.high_threshold {
        return Some(CadencePressure::High);
    }
    if p.count >= p.medium_threshold && p.total >= p.medium_window_floor {
        return Some(CadencePressure::Medium);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_when_count_above_high_threshold() {
        let r = resolve_cadence_pressure(CadencePressureParams {
            count: 3, total: 10, high_threshold: 3, medium_threshold: 2, medium_window_floor: 4,
        });
        assert_eq!(r, Some(CadencePressure::High));
    }

    #[test]
    fn medium_requires_window_floor() {
        // count 达 medium 但 total 不足 window_floor → None
        let r = resolve_cadence_pressure(CadencePressureParams {
            count: 2, total: 3, high_threshold: 3, medium_threshold: 2, medium_window_floor: 4,
        });
        assert_eq!(r, None);
        // total 达标 → Medium
        let r = resolve_cadence_pressure(CadencePressureParams {
            count: 2, total: 4, high_threshold: 3, medium_threshold: 2, medium_window_floor: 4,
        });
        assert_eq!(r, Some(CadencePressure::Medium));
    }

    #[test]
    fn none_when_below_thresholds() {
        let r = resolve_cadence_pressure(CadencePressureParams {
            count: 1, total: 10, high_threshold: 3, medium_threshold: 2, medium_window_floor: 4,
        });
        assert_eq!(r, None);
    }

    #[test]
    fn pressure_serializes_to_lowercase() {
        assert_eq!(serde_json::to_string(&CadencePressure::High).unwrap(), "\"high\"");
        assert_eq!(serde_json::to_string(&CadencePressure::Medium).unwrap(), "\"medium\"");
    }
}
