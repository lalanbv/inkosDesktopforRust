//! 长度合同 2.0 形态（891 号 M3c 终）：`packages/core/src/utils/length-metrics.ts`
//! L58-80 的 `chapterLengthDelivery`/`assertChapterLength`——**显式 bounds 硬合同**
//!（旧 utils::length_metrics 为软/硬区间制活体，不动；本模块并排落 2.0 形态）。
//! `LengthSpec` 2.0 = `{target, countingMode, minChapterLength?, maxChapterLength?}`
//!（无软硬区间、显式 bounds 优先——886 勘察判定的 2.0 重写面之一）。

use serde::Serialize;

use crate::models::length_governance::LengthCountingMode;
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{count_chapter_length, resolve_length_counting_mode};

/// TS 2.0 `LengthSpec`（显式 bounds 契约——与旧软/硬区间制并存不混用）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortLengthSpec {
    pub target: u32,
    #[serde(rename = "countingMode")]
    pub counting_mode: LengthCountingMode,
    #[serde(rename = "minChapterLength", skip_serializing_if = "Option::is_none")]
    pub min_chapter_length: Option<u32>,
    #[serde(rename = "maxChapterLength", skip_serializing_if = "Option::is_none")]
    pub max_chapter_length: Option<u32>,
}

/// TS `buildLengthSpec(target, language, bounds)`。
pub fn build_length_spec(
    target: u32,
    language: WritingLanguage,
    bounds: (Option<u32>, Option<u32>),
) -> ShortLengthSpec {
    ShortLengthSpec {
        target,
        counting_mode: resolve_length_counting_mode(language),
        min_chapter_length: bounds.0,
        max_chapter_length: bounds.1,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterLengthIssue {
    pub code: String,
    pub actual: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChapterLengthStatus {
    #[serde(rename = "checks_passed")]
    ChecksPassed,
    #[serde(rename = "needs_revision")]
    NeedsRevision,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterLengthDelivery {
    pub status: ChapterLengthStatus,
    pub measurements: ChapterLengthMeasurements,
    pub target: ShortLengthSpec,
    pub issues: Vec<ChapterLengthIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterLengthMeasurements {
    pub count: u32,
    #[serde(rename = "countingMode")]
    pub counting_mode: LengthCountingMode,
}

/// TS `chapterLengthDelivery`：无显式 bounds → undefined（宿主不做近似目标判定）。
pub fn chapter_length_delivery(count: u32, spec: &ShortLengthSpec) -> Option<ChapterLengthDelivery> {
    if spec.min_chapter_length.is_none() && spec.max_chapter_length.is_none() {
        return None;
    }
    let mut issues = Vec::new();
    let below = spec.min_chapter_length.map(|min| count < min).unwrap_or(false);
    let above = spec.max_chapter_length.map(|max| count > max).unwrap_or(false);
    if below || above {
        issues.push(ChapterLengthIssue {
            code: "CHAPTER_LENGTH_OUT_OF_RANGE".into(),
            actual: count,
            minimum: spec.min_chapter_length,
            maximum: spec.max_chapter_length,
        });
    }
    Some(ChapterLengthDelivery {
        status: if issues.is_empty() {
            ChapterLengthStatus::ChecksPassed
        } else {
            ChapterLengthStatus::NeedsRevision
        },
        measurements: ChapterLengthMeasurements { count, counting_mode: spec.counting_mode },
        target: spec.clone(),
        issues,
    })
}

pub struct ChapterLengthError {
    pub message: String,
    pub delivery: ChapterLengthDelivery,
}

impl std::fmt::Display for ChapterLengthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// TS `assertChapterLength`：越界 → CHAPTER_LENGTH_OUT_OF_RANGE（message =
/// details JSON 串，TS 同形）。
pub fn assert_chapter_length(
    content: &str,
    spec: Option<&ShortLengthSpec>,
) -> Result<(), ChapterLengthError> {
    let Some(spec) = spec else { return Ok(()) };
    let count = count_chapter_length(content, spec.counting_mode);
    let Some(delivery) = chapter_length_delivery(count, spec) else {
        return Ok(());
    };
    if !delivery.issues.is_empty() {
        let message = serde_json::to_string(&serde_json::json!({
            "code": "CHAPTER_LENGTH_OUT_OF_RANGE",
            "delivery": serde_json::to_value(&delivery).unwrap_or(serde_json::Value::Null),
            "instruction": "Revise the complete chapter into the explicit length range, preserving core events and causal continuity.",
        }))
        .unwrap_or_default();
        return Err(ChapterLengthError { message, delivery });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_bounds_means_no_delivery() {
        let spec = build_length_spec(1000, WritingLanguage::Zh, (None, None));
        assert!(chapter_length_delivery(5, &spec).is_none());
        assert!(assert_chapter_length("任意", Some(&spec)).is_ok());
    }

    #[test]
    fn explicit_bounds_are_hard_checks() {
        let spec = build_length_spec(100, WritingLanguage::Zh, (Some(80), Some(120)));
        assert!(assert_chapter_length(&"字".repeat(100), Some(&spec)).is_ok());
        let error = assert_chapter_length(&"字".repeat(50), Some(&spec)).unwrap_err();
        assert!(error.message.contains("CHAPTER_LENGTH_OUT_OF_RANGE"));
        assert_eq!(error.delivery.status, ChapterLengthStatus::NeedsRevision);
        assert_eq!(error.delivery.issues[0].actual, 50);
    }
}
