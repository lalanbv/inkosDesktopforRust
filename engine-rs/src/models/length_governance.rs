//! 长度治理类型。
//!
//! 移植自 `packages/core/src/models/length-governance.ts`（zod schema 定义）。
//! 这些类型被 [`crate::utils::length_metrics`] 与后续 pipeline/state 域共用。

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

/// 长度归一化模式。对齐 TS `"expand" | "compress" | "none"`。
/// 变体 `None` 经 serde 重命名为 `"none"`（与 `Option::None` 无关，全限定使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"expand\" | \"compress\" | \"none\""))]
pub enum LengthNormalizeMode {
    #[serde(rename = "expand")]
    Expand,
    #[serde(rename = "compress")]
    Compress,
    #[serde(rename = "none")]
    None,
}

/// 章节长度规格（目标 + 软/硬区间 + 计量/归一化模式）。
///
/// 所有数值字段对齐 TS `z.number().int().min(1)`（Rust 侧用 u32，编译期非负）。
/// `rename_all = "camelCase"` 对齐 TS JSON 契约（softMin/softMax/hardMin/hardMax/
/// countingMode/normalizeMode），保证 strangler 切换时端点响应字段名一致。
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
    pub normalize_mode: LengthNormalizeMode,
}
