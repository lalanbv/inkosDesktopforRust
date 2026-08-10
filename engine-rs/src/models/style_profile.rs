//! 风格画像（从参考文本提取的风格指纹）。
//!
//! 移植自 `packages/core/src/models/style-profile.ts`（15 行，纯类型）。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 段落长度范围。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ParagraphLengthRange {
    pub min: f64,
    pub max: f64,
}

/// 风格指纹：句长/段落/词汇多样性/常用模式/修辞特征。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct StyleProfile {
    pub avg_sentence_length: f64,
    pub sentence_length_std_dev: f64,
    pub avg_paragraph_length: f64,
    pub paragraph_length_range: ParagraphLengthRange,
    pub vocabulary_diversity: f64, // TTR（Type-Token Ratio）
    pub top_patterns: Vec<String>,
    pub rhetorical_features: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyzed_at: Option<String>,
}
