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

// ---- 重试结算链（38 号补齐；依赖 state-validator 与 writer settle 端口） ----

use crate::agents::state_validator::{ValidationResult, ValidationWarning};
use crate::agents::writer::WriteChapterOutput;
use crate::utils::language::WritingLanguage;

/// writer.settleChapterState 的链内端口。
#[async_trait::async_trait]
pub trait SettlePort: Send + Sync {
    async fn settle(&self, params: SettleRequest<'_>) -> Result<WriteChapterOutput, String>;
}

/// settle 入参（治理子集 + 重放/反馈开关 + 书面上下文）。
pub struct SettleRequest<'a> {
    pub book: &'a crate::models::book::BookConfig,
    pub book_dir: &'a std::path::Path,
    pub title: &'a str,
    pub content: &'a str,
    pub allow_reapply: bool,
    /// TS `allowNewHooks`（resync 链「保持稳定 hook id」）；None = 默认放行。
    pub allow_new_hooks: Option<bool>,
    /// 132 号：快照基准章（TS retry settle 的 baselineChapter 透传——重试
    /// 结算同样走章前快照重放；None = 当前 story 目录）。
    pub baseline_chapter: Option<u32>,
    pub chapter_intent: Option<&'a str>,
    pub context_package: Option<&'a crate::models::input_governance::ContextPackage>,
    pub rule_stack: Option<&'a crate::models::input_governance::RuleStack>,
    pub validation_feedback: Option<&'a str>,
}

/// state-validator.validate 的链内端口。
#[async_trait::async_trait]
pub trait ValidatePort: Send + Sync {
    async fn validate(&self, params: ValidateRequest<'_>) -> Result<ValidationResult, String>;
}

/// validate 入参。
pub struct ValidateRequest<'a> {
    pub content: &'a str,
    pub chapter_number: u32,
    pub old_state: &'a str,
    pub new_state: &'a str,
    pub old_hooks: &'a str,
    pub new_hooks: &'a str,
    pub language: WritingLanguage,
    pub authority_context: Option<&'a crate::agents::state_validator::StateValidationAuthorityContext>,
}

/// 重试结果。对齐 TS `SettlementRetryResult`（Recovered 装箱平衡变体尺寸）。
pub enum SettlementRetryResult {
    Recovered {
        output: Box<WriteChapterOutput>,
        validation: ValidationResult,
    },
    Degraded {
        issues: Vec<crate::agents::continuity::AuditIssue>,
    },
}

/// 重试入参。
pub struct SettlementRetryParams<'a> {
    pub writer: &'a dyn SettlePort,
    pub validator: &'a dyn ValidatePort,
    pub book: &'a crate::models::book::BookConfig,
    pub book_dir: &'a std::path::Path,
    pub chapter_number: u32,
    /// 快照基准章（TS baselineChapter）。
    pub baseline_chapter: Option<u32>,
    /// TS `allowNewHooks`（resync 链透传重试结算）。
    pub allow_new_hooks: Option<bool>,
    pub title: &'a str,
    pub content: &'a str,
    pub control: Option<ControlInput<'a>>,
    pub old_state: &'a str,
    pub old_hooks: &'a str,
    pub original_validation: &'a ValidationResult,
    pub language: WritingLanguage,
    pub log_warn: &'a (dyn Fn(&str, &str) + Send + Sync),
}

/// 治理控制入参（settle 的 governed 三件）。
#[derive(Clone)]
pub struct ControlInput<'a> {
    pub chapter_intent: &'a str,
    pub context_package: &'a crate::models::input_governance::ContextPackage,
    pub rule_stack: &'a crate::models::input_governance::RuleStack,
}

/// 状态校验失败后的仅重试结算层：settle(allowReapply + 反馈) → 复验 →
/// recovered / degraded。
pub async fn retry_settlement_after_validation_failure(
    params: SettlementRetryParams<'_>,
) -> Result<SettlementRetryResult, String> {
    (params.log_warn)(
        &format!(
            "状态校验失败，正在仅重试结算层（第{}章）",
            params.chapter_number
        ),
        &format!(
            "State validation failed; retrying settlement only for chapter {}",
            params.chapter_number
        ),
    );

    let retry_output = params
        .writer
        .settle(SettleRequest {
            book: params.book,
            book_dir: params.book_dir,
            title: params.title,
            content: params.content,
            allow_reapply: true,
            allow_new_hooks: params.allow_new_hooks,
            baseline_chapter: params.baseline_chapter,
            chapter_intent: params.control.as_ref().map(|control| control.chapter_intent),
            context_package: params.control.as_ref().map(|control| control.context_package),
            rule_stack: params.control.as_ref().map(|control| control.rule_stack),
            validation_feedback: Some(&build_state_validation_feedback(
                &params.original_validation.warnings,
                params.language,
            )),
        })
        .await?;

    let retry_validation = params
        .validator
        .validate(ValidateRequest {
            content: params.content,
            chapter_number: params.chapter_number,
            old_state: params.old_state,
            new_state: &retry_output.updated_state,
            old_hooks: params.old_hooks,
            new_hooks: &retry_output.updated_hooks,
            language: params.language,
            authority_context: None,
        })
        .await
        .map_err(|error| {
            format!(
                "State validation retry failed for chapter {}: {error}",
                params.chapter_number
            )
        })?;

    if !retry_validation.warnings.is_empty() {
        (params.log_warn)(
            &format!(
                "状态校验重试后，第{}章仍有 {} 条警告",
                params.chapter_number,
                retry_validation.warnings.len()
            ),
            &format!(
                "State validation retry still reports {} warning(s) for chapter {}",
                retry_validation.warnings.len(),
                params.chapter_number
            ),
        );
        for warning in &retry_validation.warnings {
            tracing::warn!("  [{}] {}", warning.category, warning.description);
        }
    }

    if retry_validation.passed {
        return Ok(SettlementRetryResult::Recovered {
            output: Box::new(retry_output),
            validation: retry_validation,
        });
    }

    Ok(SettlementRetryResult::Degraded {
        issues: build_state_degraded_issues(&retry_validation.warnings, params.language),
    })
}

/// 重试反馈文案。golden 守门。
pub fn build_state_validation_feedback(
    warnings: &[ValidationWarning],
    language: WritingLanguage,
) -> String {
    if warnings.is_empty() {
        return if language == WritingLanguage::En {
            "The previous settlement contradicted the chapter text. Reconcile truth files strictly to the body."
                .to_string()
        } else {
            "上一次状态结算与正文矛盾。请严格以正文为准修正 truth files。".to_string()
        };
    }

    if language == WritingLanguage::En {
        let mut lines = vec![
            "The previous settlement failed validation. Fix these contradictions against the chapter body:".to_string(),
        ];
        lines.extend(
            warnings
                .iter()
                .map(|warning| format!("- [{}] {}", warning.category, warning.description)),
        );
        return lines.join("\n");
    }

    let mut lines = vec!["上一次状态结算未通过校验。请对照正文修正以下矛盾：".to_string()];
    lines.extend(
        warnings
            .iter()
            .map(|warning| format!("- [{}] {}", warning.category, warning.description)),
    );
    lines.join("\n")
}

/// 降级问题清单。golden 守门。
pub fn build_state_degraded_issues(
    warnings: &[ValidationWarning],
    language: WritingLanguage,
) -> Vec<crate::agents::continuity::AuditIssue> {
    use crate::agents::continuity::AuditSeverity;
    let suggestion = if language == WritingLanguage::En {
        "Repair chapter state from the persisted body before continuing."
    } else {
        "请先基于已保存正文修复本章 state，再继续后续章节。"
    };
    if !warnings.is_empty() {
        return warnings
            .iter()
            .map(|warning| AuditIssue {
                severity: AuditSeverity::Warning,
                category: "state-validation".to_string(),
                description: warning.description.clone(),
                suggestion: suggestion.to_string(),
                repair_scope: None,
            })
            .collect();
    }

    vec![AuditIssue {
        severity: AuditSeverity::Warning,
        category: "state-validation".to_string(),
        description: if language == WritingLanguage::En {
            "State validation still failed after settlement retry.".to_string()
        } else {
            "状态结算重试后仍未通过校验。".to_string()
        },
        suggestion: suggestion.to_string(),
        repair_scope: None,
    }]
}

/// 降级持久化产物：结算字段全部回退旧真相，保留标题/正文等展示字段。
pub fn build_state_degraded_persistence_output(
    output: &WriteChapterOutput,
    old_state: &str,
    pub_old: (&str, &str),
) -> WriteChapterOutput {
    let (old_hooks, old_ledger) = pub_old;
    WriteChapterOutput {
        runtime_state_delta: None,
        runtime_state_snapshot: None,
        updated_state: old_state.to_string(),
        updated_ledger: old_ledger.to_string(),
        updated_hooks: old_hooks.to_string(),
        updated_chapter_summaries: None,
        ..output.clone()
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use crate::agents::continuity::AuditSeverity;

    fn test_book() -> crate::models::book::BookConfig {
        crate::models::book::BookConfig {
            series_id: None,
            id: "b".to_string(),
            title: "t".to_string(),
            platform: crate::models::book::Platform::Other,
            genre: "xianxia".to_string(),
            status: crate::models::book::BookStatus::Active,
            target_chapters: 10,
            chapter_word_count: 3000,
            language: Some("zh".to_string()),
            created_at: String::new(),
            updated_at: String::new(),
            parent_book_id: None,
            fanfic_mode: None,
            series: None,
            writing: None,
            governance: None,
        }
    }
    use crate::agents::state_validator::{ValidationResult, ValidationWarning};

    fn warning(category: &str, description: &str) -> ValidationWarning {
        ValidationWarning {
            category: category.into(),
            description: description.into(),
        }
    }

    #[test]
    fn feedback_and_degraded_issues_shape() {
        let zh_empty = build_state_validation_feedback(&[], WritingLanguage::Zh);
        assert!(zh_empty.contains("请严格以正文为准修正"));
        let en_list = build_state_validation_feedback(
            &[warning("c1", "d1"), warning("c2", "d2")],
            WritingLanguage::En,
        );
        assert!(en_list.starts_with("The previous settlement failed validation."));
        assert!(en_empty_check(&en_list));

        let issues = build_state_degraded_issues(&[], WritingLanguage::Zh);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].description, "状态结算重试后仍未通过校验。");
        let issues = build_state_degraded_issues(&[warning("c", "d")], WritingLanguage::Zh);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].description, "d");
        assert_eq!(issues[0].severity, AuditSeverity::Warning);
    }

    fn en_empty_check(text: &str) -> bool {
        text.contains("- [c1] d1") && text.contains("- [c2] d2")
    }

    struct ScriptSettle {
        updated_state: String,
        updated_hooks: String,
    }

    #[async_trait::async_trait]
    impl SettlePort for ScriptSettle {
        async fn settle(&self, params: SettleRequest<'_>) -> Result<WriteChapterOutput, String> {
            let _ = (params.book, params.book_dir);
            assert!(params.allow_reapply);
            assert!(params.validation_feedback.is_some());
            Ok(WriteChapterOutput {
                updated_state: self.updated_state.clone(),
                updated_hooks: self.updated_hooks.clone(),
                ..base_output()
            })
        }
    }

    fn base_output() -> WriteChapterOutput {
        WriteChapterOutput {
            chapter_number: 3,
            title: "t".into(),
            content: "c".into(),
            word_count: 1,
            pre_write_check: String::new(),
            post_settlement: String::new(),
            runtime_state_delta: None,
            runtime_state_snapshot: None,
            updated_state: "new-state".into(),
            updated_ledger: "new-ledger".into(),
            updated_hooks: "new-hooks".into(),
            chapter_summary: String::new(),
            updated_chapter_summaries: None,
            updated_subplots: String::new(),
            updated_emotional_arcs: String::new(),
            updated_character_matrix: String::new(),
            post_write_errors: vec![],
            post_write_warnings: vec![],
            hook_health_issues: vec![],
            tension_metrics: None,
            token_usage: Default::default(),
        }
    }

    struct ScriptValidate {
        results: std::sync::Mutex<Vec<ValidationResult>>,
    }

    #[async_trait::async_trait]
    impl ValidatePort for ScriptValidate {
        async fn validate(&self, _params: ValidateRequest<'_>) -> Result<ValidationResult, String> {
            Ok(self.results.lock().unwrap().remove(0))
        }
    }

    fn retry_params<'a>(
        writer: &'a ScriptSettle,
        validator: &'a ScriptValidate,
        original: &'a ValidationResult,
        book: &'a crate::models::book::BookConfig,
    ) -> SettlementRetryParams<'a> {
        SettlementRetryParams {
            writer,
            validator,
            book,
            book_dir: std::path::Path::new("/tmp"),
            chapter_number: 3,
            baseline_chapter: None,
            allow_new_hooks: None,
            title: "t",
            content: "c",
            control: None,
            old_state: "old-state",
            old_hooks: "old-hooks",
            original_validation: original,
            language: WritingLanguage::Zh,
            log_warn: &|_zh, _en| {},
        }
    }

    #[tokio::test]
    async fn retry_recovers_when_second_validation_passes() {
        let writer = ScriptSettle {
            updated_state: "fixed-state".into(),
            updated_hooks: "fixed-hooks".into(),
        };
        let validator = ScriptValidate {
            results: std::sync::Mutex::new(vec![ValidationResult {
                warnings: vec![],
                passed: true, repair_required: false,
            }]),
        };
        let original = ValidationResult {
            warnings: vec![warning("c", "d")],
            passed: false, repair_required: false,
        };
        let book = test_book();
        match retry_settlement_after_validation_failure(retry_params(&writer, &validator, &original, &book))
            .await
            .unwrap()
        {
            SettlementRetryResult::Recovered { output, validation } => {
                assert_eq!(output.updated_state, "fixed-state");
                assert!(validation.passed);
            }
            SettlementRetryResult::Degraded { .. } => panic!("应恢复"),
        }
    }

    #[tokio::test]
    async fn retry_degrades_when_still_failing() {
        let writer = ScriptSettle {
            updated_state: "still-bad".into(),
            updated_hooks: "still-bad-hooks".into(),
        };
        let validator = ScriptValidate {
            results: std::sync::Mutex::new(vec![ValidationResult {
                warnings: vec![warning("contradiction", "硬矛盾")],
                passed: false, repair_required: false,
            }]),
        };
        let original = ValidationResult { warnings: vec![], passed: false, repair_required: false };
        let book = test_book();
        match retry_settlement_after_validation_failure(retry_params(&writer, &validator, &original, &book))
            .await
            .unwrap()
        {
            SettlementRetryResult::Degraded { issues } => {
                assert_eq!(issues.len(), 1);
                assert_eq!(issues[0].description, "硬矛盾");
            }
            SettlementRetryResult::Recovered { .. } => panic!("应降级"),
        }
    }

    #[test]
    fn degraded_persistence_output_restores_old_truth() {
        let output = base_output();
        let degraded =
            build_state_degraded_persistence_output(&output, "old-state", ("old-hooks", "old-ledger"));
        assert_eq!(degraded.updated_state, "old-state");
        assert_eq!(degraded.updated_hooks, "old-hooks");
        assert_eq!(degraded.updated_ledger, "old-ledger");
        assert!(degraded.runtime_state_delta.is_none());
        assert!(degraded.runtime_state_snapshot.is_none());
        assert!(degraded.updated_chapter_summaries.is_none());
        // 展示字段保留。
        assert_eq!(degraded.title, "t");
        assert_eq!(degraded.content, "c");
    }
}
