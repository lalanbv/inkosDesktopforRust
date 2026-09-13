//! chapter-truth-validation —— 落盘前的真相文件一致性校验编排。
//!
//! 移植自 `packages/core/src/pipeline/chapter-truth-validation.ts`（145 行）。
//! validate → 失败时重试结算（recovered 换产物 / degraded 回退旧真相 + 注入
//! 降级问题）；validator 本身抛错（网络/解析）→ 视为可用性降级而非内容失败
//! （保留旧真相 + 状态标记 state-degraded）。

use crate::agents::continuity::{AuditIssue, AuditResult, AuditSeverity};
use crate::agents::state_validator::{
    StateValidationAuthorityContext, ValidationResult,
};
use crate::agents::writer::WriteChapterOutput;
use crate::pipeline::chapter_state_recovery::{
    build_state_degraded_persistence_output, retry_settlement_after_validation_failure,
    ControlInput, SettlePort, ValidatePort,
};
use crate::utils::language::WritingLanguage;

/// 校验编排入参。
pub struct TruthValidationParams<'a> {
    /// 快照基准章（写新章链为 None——settle 基于当前 story；修订链待接）。
    pub baseline_chapter: Option<u32>,
    /// TS `allowNewHooks`（resync 链透传）。
    pub allow_new_hooks: Option<bool>,
    pub writer: &'a dyn SettlePort,
    pub validator: &'a dyn ValidatePort,
    pub book: &'a crate::models::book::BookConfig,
    pub book_dir: &'a std::path::Path,
    pub chapter_number: u32,
    pub title: &'a str,
    pub content: &'a str,
    pub persistence_output: WriteChapterOutput,
    pub audit_result: AuditResult,
    pub old_state: &'a str,
    pub old_hooks: &'a str,
    pub old_ledger: &'a str,
    pub authority_context: Option<&'a StateValidationAuthorityContext>,
    pub control: Option<ControlInput<'a>>,
    pub language: WritingLanguage,
    pub log_warn: &'a (dyn Fn(&str, &str) + Send + Sync),
}

/// 校验编排出参。
pub struct TruthValidationOutcome {
    pub validation: ValidationResult,
    /// state-degraded / None。
    pub chapter_status: Option<&'static str>,
    pub degraded_issues: Vec<AuditIssue>,
    pub persistence_output: WriteChapterOutput,
    pub audit_result: AuditResult,
}

/// 落盘前真相校验主入口。
pub async fn validate_chapter_truth_persistence(
    params: TruthValidationParams<'_>,
) -> Result<TruthValidationOutcome, String> {
    let mut persistence_output = params.persistence_output;
    let mut audit_result = params.audit_result;

    let validation = match params
        .validator
        .validate(crate::pipeline::chapter_state_recovery::ValidateRequest {
            content: params.content,
            chapter_number: params.chapter_number,
            old_state: params.old_state,
            new_state: &persistence_output.updated_state,
            old_hooks: params.old_hooks,
            new_hooks: &persistence_output.updated_hooks,
            language: params.language,
            authority_context: params.authority_context,
        })
        .await
    {
        Ok(validation) => validation,
        Err(error) => {
            // validator 抛错（网络/解析）→ 可用性降级：保留旧真相继续。
            tracing::warn!(
                "State validation error for chapter {}: {error}",
                params.chapter_number
            );
            let error_description = if params.language == WritingLanguage::En {
                format!("State validation unavailable: {error}")
            } else {
                format!("状态校验不可用：{error}")
            };
            let suggestion = if params.language == WritingLanguage::En {
                "Repair chapter state from the persisted body before continuing."
            } else {
                "请先基于已保存正文修复本章 state，再继续后续章节。"
            };
            let error_issue = AuditIssue {
                severity: AuditSeverity::Warning,
                category: "state-validation".to_string(),
                description: error_description,
                suggestion: suggestion.to_string(),
                repair_scope: None,
            };
            let mut issues = audit_result.issues.clone();
            issues.push(error_issue.clone());
            return Ok(TruthValidationOutcome {
                validation: ValidationResult { passed: true, warnings: Vec::new(), repair_required: false },
                chapter_status: Some("state-degraded"),
                degraded_issues: vec![error_issue],
                persistence_output: build_state_degraded_persistence_output(
                    &persistence_output,
                    params.old_state,
                    (params.old_hooks, params.old_ledger),
                ),
                audit_result: AuditResult { issues, ..audit_result },
            });
        }
    };

    if !validation.warnings.is_empty() {
        (params.log_warn)(
            &format!(
                "状态校验：第{}章发现 {} 条警告",
                params.chapter_number,
                validation.warnings.len()
            ),
            &format!(
                "State validation: {} warning(s) for chapter {}",
                validation.warnings.len(),
                params.chapter_number
            ),
        );
        for warning in &validation.warnings {
            tracing::warn!("  [{}] {}", warning.category, warning.description);
        }
    }

    if !validation.passed {
        let recovery = retry_settlement_after_validation_failure(
            crate::pipeline::chapter_state_recovery::SettlementRetryParams {
                writer: params.writer,
                validator: params.validator,
                book: params.book,
                book_dir: params.book_dir,
                chapter_number: params.chapter_number,
                baseline_chapter: params.baseline_chapter,
                allow_new_hooks: params.allow_new_hooks,
                title: params.title,
                content: params.content,
                control: params.control.clone(),
                old_state: params.old_state,
                old_hooks: params.old_hooks,
                original_validation: &validation,
                language: params.language,
                log_warn: params.log_warn,
            },
        )
        .await?;

        match recovery {
            crate::pipeline::chapter_state_recovery::SettlementRetryResult::Recovered {
                output,
                validation: recovered_validation,
            } => {
                persistence_output = *output;
                let _ = recovered_validation;
                // TS：恢复时 validation 替换为重试验证结果——recovered 已 moved，
                // 重新以 recovered_validation 为准（此处直接沿用其 passed/warnings）。
                return Ok(TruthValidationOutcome {
                    validation: recovered_validation,
                    chapter_status: None,
                    degraded_issues: Vec::new(),
                    persistence_output,
                    audit_result,
                });
            }
            crate::pipeline::chapter_state_recovery::SettlementRetryResult::Degraded { issues } => {
                let mut merged_issues = audit_result.issues.clone();
                merged_issues.extend(issues.iter().cloned());
                audit_result = AuditResult { issues: merged_issues, ..audit_result };
                return Ok(TruthValidationOutcome {
                    validation,
                    chapter_status: Some("state-degraded"),
                    degraded_issues: issues,
                    persistence_output: build_state_degraded_persistence_output(
                        &persistence_output,
                        params.old_state,
                        (params.old_hooks, params.old_ledger),
                    ),
                    audit_result,
                });
            }
        }
    }

    Ok(TruthValidationOutcome {
        validation,
        chapter_status: None,
        degraded_issues: Vec::new(),
        persistence_output,
        audit_result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::state_validator::ValidationResult;
    use crate::pipeline::chapter_state_recovery::{SettleRequest, ValidateRequest};

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

    fn audit(passed: bool) -> AuditResult {
        AuditResult {
            passed,
            issues: vec![],
            summary: "s".into(),
            parse_failed: None,
            overall_score: None,
            token_usage: None,
        }
    }

    /// 验证脚本：按序返回结果；首项 Err 模拟 validator 抛错。
    struct ScriptValidate {
        results: std::sync::Mutex<Vec<Result<ValidationResult, String>>>,
        seen: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl ValidatePort for ScriptValidate {
        async fn validate(&self, params: ValidateRequest<'_>) -> Result<ValidationResult, String> {
            self.seen
                .lock()
                .unwrap()
                .push(params.new_state.to_string());
            self.results.lock().unwrap().remove(0)
        }
    }

    struct NoopSettle;

    #[async_trait::async_trait]
    impl SettlePort for NoopSettle {
        async fn settle(&self, _params: SettleRequest<'_>) -> Result<WriteChapterOutput, String> {
            Ok(WriteChapterOutput {
                updated_state: "retried-state".into(),
                updated_hooks: "retried-hooks".into(),
                ..base_output()
            })
        }
    }

    fn params<'a>(
        validator: &'a ScriptValidate,
        book: &'a crate::models::book::BookConfig,
    ) -> TruthValidationParams<'a> {
        TruthValidationParams {
            writer: &NoopSettle,
            validator,
            book,
            book_dir: std::path::Path::new("/tmp"),
            chapter_number: 3,
            baseline_chapter: None,
            allow_new_hooks: None,
            title: "t",
            content: "c",
            persistence_output: base_output(),
            audit_result: audit(true),
            old_state: "old-state",
            old_hooks: "old-hooks",
            old_ledger: "old-ledger",
            authority_context: None,
            control: None,
            language: WritingLanguage::Zh,
            log_warn: &|_zh, _en| {},
        }
    }

    #[tokio::test]
    async fn pass_through_when_validation_passes() {
        let validator = ScriptValidate {
            results: std::sync::Mutex::new(vec![Ok(ValidationResult {
                warnings: vec![],
                passed: true, repair_required: false,
            })]),
            seen: std::sync::Mutex::new(Vec::new()),
        };
        let book = test_book();
        let outcome = validate_chapter_truth_persistence(params(&validator, &book)).await.unwrap();
        assert!(outcome.chapter_status.is_none());
        assert_eq!(outcome.persistence_output.updated_state, "new-state");
        assert!(outcome.degraded_issues.is_empty());
    }

    #[tokio::test]
    async fn validator_error_degrades_with_availability_note() {
        let validator = ScriptValidate {
            results: std::sync::Mutex::new(vec![Err("network boom".to_string())]),
            seen: std::sync::Mutex::new(Vec::new()),
        };
        let book = test_book();
        let outcome = validate_chapter_truth_persistence(params(&validator, &book)).await.unwrap();
        assert_eq!(outcome.chapter_status, Some("state-degraded"));
        assert_eq!(outcome.degraded_issues.len(), 1);
        assert!(outcome.degraded_issues[0].description.contains("状态校验不可用"));
        // 旧真相回退。
        assert_eq!(outcome.persistence_output.updated_state, "old-state");
        // 审计问题注入。
        assert_eq!(outcome.audit_result.issues.len(), 1);
    }

    #[tokio::test]
    async fn fail_then_retry_recovers() {
        let validator = ScriptValidate {
            results: std::sync::Mutex::new(vec![
                Ok(ValidationResult { warnings: vec![], passed: false, repair_required: false }),
                Ok(ValidationResult { warnings: vec![], passed: true, repair_required: false }),
            ]),
            seen: std::sync::Mutex::new(Vec::new()),
        };
        let book = test_book();
        let outcome = validate_chapter_truth_persistence(params(&validator, &book)).await.unwrap();
        assert!(outcome.chapter_status.is_none());
        assert_eq!(outcome.persistence_output.updated_state, "retried-state");
        // 两次校验：初始产物 + 重试产物。
        assert_eq!(
            *validator.seen.lock().unwrap(),
            vec!["new-state".to_string(), "retried-state".to_string()]
        );
    }

    #[tokio::test]
    async fn fail_then_retry_degrades_restoring_old_truth() {
        let validator = ScriptValidate {
            results: std::sync::Mutex::new(vec![
                Ok(ValidationResult { warnings: vec![], passed: false, repair_required: false }),
                Ok(ValidationResult { warnings: vec![], passed: false, repair_required: false }),
            ]),
            seen: std::sync::Mutex::new(Vec::new()),
        };
        let book = test_book();
        let outcome = validate_chapter_truth_persistence(params(&validator, &book)).await.unwrap();
        assert_eq!(outcome.chapter_status, Some("state-degraded"));
        assert_eq!(outcome.persistence_output.updated_state, "old-state");
        assert_eq!(outcome.persistence_output.updated_ledger, "old-ledger");
        assert_eq!(outcome.degraded_issues.len(), 1);
        assert!(!outcome.audit_result.issues.is_empty());
    }
}
