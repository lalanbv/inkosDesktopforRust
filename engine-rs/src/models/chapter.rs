//! 章节模型。
//!
//! 移植自 `packages/core/src/models/chapter.ts`（42 行）。依赖已移植的 [`LengthTelemetry`]。

use super::length_governance::LengthTelemetry;
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 章节状态机（14 态）。逐字对齐 TS `ChapterStatusSchema` 运行时行为。
/// 注：与 `models::placeholders::ChapterStatus`（PoC 占位 6 态）不同——本类型是真实业务态。
/// `needs-revision` 为 TS reviseDraft 对下游章的运行时强转写入（union 未声明，47 号补齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(
    feature = "export-bindings",
    ts(export, type = "\"card-generated\" | \"drafting\" | \"drafted\" | \"auditing\" | \"audit-passed\" | \"audit-failed\" | \"state-degraded\" | \"revising\" | \"ready-for-review\" | \"approved\" | \"rejected\" | \"published\" | \"imported\" | \"needs-revision\"")
)]
pub enum ChapterStatus {
    #[serde(rename = "card-generated")]
    CardGenerated,
    #[serde(rename = "drafting")]
    Drafting,
    #[serde(rename = "drafted")]
    Drafted,
    #[serde(rename = "auditing")]
    Auditing,
    #[serde(rename = "audit-passed")]
    AuditPassed,
    #[serde(rename = "audit-failed")]
    AuditFailed,
    #[serde(rename = "state-degraded")]
    StateDegraded,
    #[serde(rename = "revising")]
    Revising,
    #[serde(rename = "ready-for-review")]
    ReadyForReview,
    #[serde(rename = "approved")]
    Approved,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "published")]
    Published,
    #[serde(rename = "imported")]
    Imported,
    #[serde(rename = "needs-revision")]
    NeedsRevision,
}

/// token 用量（移植自 TS ChapterMeta 的 tokenUsage 子对象）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
}

/// 章节元数据。逐字对齐 TS `ChapterMetaSchema`（default 字段用 #[serde(default)]）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ChapterMeta {
    pub number: u32,
    pub title: String,
    pub status: ChapterStatus,
    #[serde(default)]
    pub word_count: u32,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub audit_issues: Vec<String>,
    #[serde(default)]
    pub length_warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detection_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detection_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_telemetry: Option<LengthTelemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrips() {
        for s in [ChapterStatus::Drafting, ChapterStatus::AuditPassed, ChapterStatus::ReadyForReview, ChapterStatus::Imported] {
            let j = serde_json::to_string(&s).unwrap();
            let back: ChapterStatus = serde_json::from_str(&j).unwrap();
            assert_eq!(s, back);
        }
        assert_eq!(serde_json::to_string(&ChapterStatus::CardGenerated).unwrap(), "\"card-generated\"");
    }

    #[test]
    fn meta_serializes_camel_case_with_defaults() {
        let m = ChapterMeta {
            number: 1,
            title: "觉醒".into(),
            status: ChapterStatus::Drafted,
            word_count: 0,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            audit_issues: vec![],
            length_warnings: vec![],
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["number"], 1);
        assert_eq!(v["wordCount"], 0);
        assert_eq!(v["status"], "drafted");
        assert!(v.get("reviewNote").is_none()); // skip_serializing_if
    }

    #[test]
    fn meta_with_full_fields() {
        let m = ChapterMeta {
            number: 2,
            title: "x".into(),
            status: ChapterStatus::Approved,
            word_count: 3000,
            created_at: "t".into(),
            updated_at: "t".into(),
            audit_issues: vec!["a".into()],
            length_warnings: vec![],
            review_note: Some("note".into()),
            detection_score: Some(0.5),
            detection_provider: Some("p".into()),
            detected_at: Some("t2".into()),
            length_telemetry: None,
            token_usage: Some(TokenUsage { prompt_tokens: 100, completion_tokens: 50, total_tokens: 150 }),
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["tokenUsage"]["totalTokens"], 150);
        assert_eq!(v["detectionScore"], 0.5);
    }
}
