//! 长度治理类型。
//!
//! 移植自 `packages/core/src/models/length-governance.ts`（zod schema 定义）。
//! 这些类型被 [`crate::utils::length_metrics`] 与后续 pipeline/state 域共用。
//!
//! 131 号合并同步：上游 e7c04465 移除 length-normalizer 阶段——
//! `LengthNormalizeMode` 与 `LengthSpec.normalizeMode` 删除；遥测的
//! postWriterNormalizeCount/normalizeApplied 换 repairApplied（审核环修复
//! 是否改变了正文）。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 章节长度计量模式。对齐 TS `"zh_chars" | "en_words"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"zh_chars\" | \"en_words\""))]
pub enum LengthCountingMode {
    #[serde(rename = "zh_chars")]
    ZhChars,
    #[serde(rename = "en_words")]
    EnWords,
}

/// 章节长度规格（目标 + 软/硬区间 + 计量模式）。
///
/// 所有数值字段对齐 TS `z.number().int().min(1)`（Rust 侧用 u32，编译期非负）。
/// `rename_all = "camelCase"` 对齐 TS JSON 契约（softMin/softMax/hardMin/hardMax/
/// countingMode），保证 strangler 切换时端点响应字段名一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LengthSpec {
    pub target: u32,
    pub soft_min: u32,
    pub soft_max: u32,
    pub hard_min: u32,
    pub hard_max: u32,
    pub counting_mode: LengthCountingMode,
}

/// 长度遥测（移植自 TS `LengthTelemetrySchema`）。计数类字段对齐 `z.number().int().min(0)`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LengthTelemetry {
    pub target: u32,
    pub soft_min: u32,
    pub soft_max: u32,
    pub hard_min: u32,
    pub hard_max: u32,
    pub counting_mode: LengthCountingMode,
    pub writer_count: u32,
    pub post_revise_count: u32,
    pub final_count: u32,
    /// 审核环修复是否改变了正文（= revised 同式：有修复快照且终稿 ≠ 初稿）。
    pub repair_applied: bool,
    pub length_warning: bool,
}
