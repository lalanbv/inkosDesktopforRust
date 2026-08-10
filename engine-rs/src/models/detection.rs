//! 检测历史与统计。
//!
//! 移植自 `packages/core/src/models/detection.ts`（25 行，纯类型）。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 单次检测/重写事件（detection_history.json 条目）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DetectionHistoryEntry {
    pub chapter_number: u32,
    pub timestamp: String,
    pub provider: String,
    pub score: f64,
    pub action: DetectionAction,
    pub attempt: u32,
}

/// 检测动作。对齐 TS `"detect" | "rewrite"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"detect\" | \"rewrite\""))]
pub enum DetectionAction {
    #[serde(rename = "detect")]
    Detect,
    #[serde(rename = "rewrite")]
    Rewrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ChapterDetectionBreakdown {
    pub chapter_number: u32,
    pub original_score: f64,
    pub final_score: f64,
    pub rewrite_attempts: u32,
}

/// 聚合检测统计。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DetectionStats {
    pub total_detections: u32,
    pub total_rewrites: u32,
    pub avg_original_score: f64,
    pub avg_final_score: f64,
    pub avg_score_reduction: f64,
    pub pass_rate: f64,
    pub chapter_breakdown: Vec<ChapterDetectionBreakdown>,
}
