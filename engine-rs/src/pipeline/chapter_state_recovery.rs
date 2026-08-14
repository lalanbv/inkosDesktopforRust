//! 章节状态降级恢复（review note 构建子集）。
//!
//! 移植自 `packages/core/src/pipeline/chapter-state-recovery.ts`（236 行）的
//! note 三件（`buildStateDegradedReviewNote` / `parseStateDegradedReviewNote` /
//! `resolveStateDegradedBaseStatus`）——chapter-persistence 的落盘依赖。
//! 重试结算链（`retrySettlementAfterValidationFailure` / 反馈与降级问题构建）
//! 依赖 StateValidatorAgent，随 38 号 truth-validation 一并移植。

use crate::agents::continuity::{AuditIssue, AuditSeverity};
use crate::models::chapter::ChapterMeta;

/// 降级审查注记。对齐 TS `StateDegradedReviewNote`（JSON 持久化在
/// ChapterMeta.reviewNote）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDegradedReviewNote {
    pub kind: &'static str,
    pub base_status: String,
    pub injected_issues: Vec<String>,
}

/// 构建降级注记 JSON（kind=state-degraded + 基础状态 + 注入问题行）。
pub fn build_state_degraded_review_note(
    base_status: &str,
    issues: &[AuditIssue],
) -> String {
    serde_json::to_string(&StateDegradedReviewNote {
        kind: "state-degraded",
        base_status: base_status.to_string(),
        injected_issues: issues
            .iter()
            .map(|issue| format!("[{}] {}", severity_label(issue.severity), issue.description))
            .collect(),
    })
    .unwrap_or_default()
}

/// 解析降级注记；字段缺失/类型不符/JSON 损坏 → None。
pub fn parse_state_degraded_review_note(review_note: Option<&str>) -> Option<StateDegradedReviewNote> {
    let raw = review_note?;
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if value.get("kind")?.as_str()? != "state-degraded" {
        return None;
    }
    let base_status = value.get("baseStatus")?.as_str()?;
    if base_status != "ready-for-review" && base_status != "audit-failed" {
        return None;
    }
    let injected = value.get("injectedIssues")?.as_array()?;
    Some(StateDegradedReviewNote {
        kind: "state-degraded",
        base_status: base_status.to_string(),
        injected_issues: injected
            .iter()
            .filter_map(|item| item.as_str().map(String::from))
            .collect(),
    })
}

/// 从章节元数据解析降级前的基基础状态：无注记时按审计问题里是否有
/// `[critical]` 行判定。
pub fn resolve_state_degraded_base_status(chapter: &ChapterMeta) -> &'static str {
    if let Some(metadata) = parse_state_degraded_review_note(chapter.review_note.as_deref()) {
        return match metadata.base_status.as_str() {
            "audit-failed" => "audit-failed",
            _ => "ready-for-review",
        };
    }
    if chapter
        .audit_issues
        .iter()
        .any(|issue| issue.starts_with("[critical]"))
    {
        "audit-failed"
    } else {
        "ready-for-review"
    }
}

fn severity_label(severity: AuditSeverity) -> &'static str {
    match severity {
        AuditSeverity::Critical => "critical",
        AuditSeverity::Warning => "warning",
        AuditSeverity::Info => "info",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(severity: AuditSeverity, description: &str) -> AuditIssue {
        AuditIssue {
            severity,
            category: "state-validation".into(),
            description: description.into(),
            suggestion: String::new(),
            repair_scope: None,
        }
    }

    #[test]
    fn note_roundtrip_and_rejects_bad_payloads() {
        let note = build_state_degraded_review_note(
            "audit-failed",
            &[issue(AuditSeverity::Warning, "状态卡与正文矛盾")],
        );
        assert!(note.contains("\"kind\":\"state-degraded\""));
        assert!(note.contains("[warning] 状态卡与正文矛盾"));

        let parsed = parse_state_degraded_review_note(Some(&note)).unwrap();
        assert_eq!(parsed.base_status, "audit-failed");
        assert_eq!(parsed.injected_issues.len(), 1);

        assert!(parse_state_degraded_review_note(None).is_none());
        assert!(parse_state_degraded_review_note(Some("not json")).is_none());
        assert!(parse_state_degraded_review_note(Some("{\"kind\":\"other\"}")).is_none());
        assert!(parse_state_degraded_review_note(Some("{\"kind\":\"state-degraded\",\"baseStatus\":\"weird\",\"injectedIssues\":[]}")).is_none());
    }

    #[test]
    fn resolve_base_status_falls_back_to_critical_audit_lines() {
        use crate::models::chapter::ChapterStatus;
        let mut meta = ChapterMeta {
            number: 3,
            title: "t".into(),
            status: ChapterStatus::StateDegraded,
            word_count: 100,
            created_at: "now".into(),
            updated_at: "now".into(),
            audit_issues: vec!["[critical] 主线偏离".into()],
            length_warnings: vec![],
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        };
        assert_eq!(resolve_state_degraded_base_status(&meta), "audit-failed");

        meta.audit_issues = vec!["[warning] 小问题".into()];
        assert_eq!(resolve_state_degraded_base_status(&meta), "ready-for-review");

        meta.review_note = Some(build_state_degraded_review_note("audit-failed", &[]));
        assert_eq!(resolve_state_degraded_base_status(&meta), "audit-failed");
    }
}
