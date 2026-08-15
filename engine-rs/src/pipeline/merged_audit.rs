//! 合并审计评估（evaluateMergedAudit 全量移植，47 号）。
//!
//! 移植自 `packages/core/src/pipeline/runner.ts`：
//! - `evaluateMergedAudit`（L3635）：四源合并（LLM 审计 + AI 痕迹 + 敏感词 +
//!   长跨度疲劳）；revisionBlockingIssues 按构造排除疲劳（非按分类名排除，
//!   LLM 报出的同名分类问题仍计入）。
//! - `restoreLostAuditIssues` / `restoreActionableAuditIfLost`（L3593/L3605）：
//!   post 审计空响应时回退 previous 的可行动问题与计数。
//! - `REVISION_GATE_STANDARDS` + 三档门控（L269/L1520）：
//!   strict（不变差且至少改善）/ lenient（不变差）/ always（总是应用）。

use std::path::Path;

use async_trait::async_trait;

use crate::agents::ai_tells::{analyze_ai_tells, AITellIssue, AITellSeverity};
use crate::agents::continuity::{
    AuditChapterOptions, AuditIssue, AuditResult, AuditSeverity,
};
use crate::agents::sensitive_words::{
    analyze_sensitive_words, SensitiveWordSeverity,
};
use crate::utils::language::WritingLanguage;
use crate::utils::long_span_fatigue::{analyze_long_span_fatigue, AnalyzeLongSpanFatigueInput};

/// LLM 审计端口：continuity `auditChapter` 的注入面（options 全量透传，
/// 含 truthFileOverrides——post 修订审计用修稿器产出的临时真相覆盖）。
#[async_trait]
pub trait LlmAuditPort: Send + Sync {
    async fn audit(
        &self,
        chapter_content: &str,
        options: &AuditChapterOptions,
    ) -> Result<AuditResult, String>;
}

/// 合并审计评估。对齐 TS `MergedAuditEvaluation`。
#[derive(Debug, Clone)]
pub struct MergedAuditEvaluation {
    pub audit_result: AuditResult,
    pub ai_tell_count: usize,
    /// warning + critical 计数（仅 revisionBlockingIssues 口径）。
    pub blocking_count: usize,
    pub critical_count: usize,
    /// 修订阻塞问题（构造上排除长跨度疲劳）。
    pub revision_blocking_issues: Vec<AuditIssue>,
}

fn ai_tell_issue_to_audit(issue: AITellIssue) -> AuditIssue {
    AuditIssue {
        severity: match issue.severity {
            AITellSeverity::Warning => AuditSeverity::Warning,
            AITellSeverity::Info => AuditSeverity::Info,
        },
        category: issue.category,
        description: issue.description,
        suggestion: issue.suggestion,
        repair_scope: None,
    }
}

/// 四源合并审计。任一来源失败即整体失败（LLM 审计是权威源）。
pub async fn evaluate_merged_audit(
    llm: &dyn LlmAuditPort,
    book_dir: &Path,
    chapter_content: &str,
    chapter_number: u32,
    language: WritingLanguage,
    options: &AuditChapterOptions,
) -> Result<MergedAuditEvaluation, String> {
    let llm_audit = llm.audit(chapter_content, options).await?;
    let ai_tells = analyze_ai_tells(chapter_content, language);
    let sensitive = analyze_sensitive_words(chapter_content, None, language);
    let fatigue = analyze_long_span_fatigue(&AnalyzeLongSpanFatigueInput {
        book_dir,
        chapter_number,
        chapter_content,
        chapter_summary: None,
        language,
    })
    .await;

    let has_blocked_words = sensitive
        .found
        .iter()
        .any(|f| f.severity == SensitiveWordSeverity::Block);

    let ai_issues: Vec<AuditIssue> = ai_tells.issues.into_iter().map(ai_tell_issue_to_audit).collect();
    let mut issues: Vec<AuditIssue> = Vec::with_capacity(
        llm_audit.issues.len() + ai_issues.len() + sensitive.issues.len() + fatigue.len(),
    );
    issues.extend(llm_audit.issues.iter().cloned());
    issues.extend(ai_issues.iter().cloned());
    issues.extend(sensitive.issues.iter().cloned());
    // revisionBlockingIssues 按构造排除长跨度疲劳。
    let revision_blocking_issues: Vec<AuditIssue> = issues[..llm_audit.issues.len() + ai_issues.len() + sensitive.issues.len()].to_vec();
    issues.extend(fatigue.into_iter().map(Into::into));

    let blocking_count = revision_blocking_issues
        .iter()
        .filter(|i| matches!(i.severity, AuditSeverity::Warning | AuditSeverity::Critical))
        .count();
    let critical_count = revision_blocking_issues
        .iter()
        .filter(|i| i.severity == AuditSeverity::Critical)
        .count();

    Ok(MergedAuditEvaluation {
        audit_result: AuditResult {
            passed: if has_blocked_words { false } else { llm_audit.passed },
            issues,
            summary: llm_audit.summary,
            parse_failed: llm_audit.parse_failed,
            overall_score: llm_audit.overall_score,
            token_usage: llm_audit.token_usage,
        },
        ai_tell_count: ai_issues.len(),
        blocking_count,
        critical_count,
        revision_blocking_issues,
    })
}

/// post 审计空响应（未通过且零问题）时回退 previous 的 issues。
pub fn restore_lost_audit_issues(previous: &AuditResult, next: &AuditResult) -> AuditResult {
    if next.passed || !next.issues.is_empty() || previous.issues.is_empty() {
        return next.clone();
    }
    AuditResult {
        passed: next.passed,
        issues: previous.issues.clone(),
        // TS `next.summary || previous.summary`：空串也回退（JS falsy）。
        summary: if next.summary.is_empty() { previous.summary.clone() } else { next.summary.clone() },
        parse_failed: next.parse_failed,
        overall_score: next.overall_score,
        token_usage: next.token_usage,
    }
}

/// 可行动审计恢复：restore 生效时连带 revisionBlockingIssues 与计数回退 previous。
pub fn restore_actionable_audit_if_lost(
    previous: &MergedAuditEvaluation,
    next: &MergedAuditEvaluation,
) -> MergedAuditEvaluation {
    let early = next.audit_result.passed
        || !next.audit_result.issues.is_empty()
        || previous.audit_result.issues.is_empty();
    if early {
        return next.clone();
    }
    MergedAuditEvaluation {
        audit_result: restore_lost_audit_issues(&previous.audit_result, &next.audit_result),
        ai_tell_count: next.ai_tell_count,
        blocking_count: previous.blocking_count,
        critical_count: previous.critical_count,
        revision_blocking_issues: previous.revision_blocking_issues.clone(),
    }
}

/// 修订门控三档。对齐 TS `RevisionGate = "strict" | "lenient" | "always"`（默认 strict）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RevisionGate {
    #[default]
    Strict,
    Lenient,
    Always,
}

impl RevisionGate {
    /// 配置解析：未知值回退 strict（TS 侧 config 缺省即 strict）。
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("lenient") => RevisionGate::Lenient,
            Some("always") => RevisionGate::Always,
            _ => RevisionGate::Strict,
        }
    }

    /// 门控标准文案（revisionDiagnostics.standard）。逐字对齐 TS REVISION_GATE_STANDARDS。
    pub fn standard(self) -> &'static str {
        match self {
            RevisionGate::Strict => {
                "A revision is applied only when blocking, critical, and AI-tell counts do not worsen, and at least blocking or AI-tell issues improve."
            }
            RevisionGate::Lenient => {
                "A revision is applied whenever blocking, critical, and AI-tell counts do not worsen; no improvement is required (lenient gate)."
            }
            RevisionGate::Always => {
                "Manual revisions are always applied; audit counts are recorded for reference only (always gate)."
            }
        }
    }

    /// 门控判定（L1520）：always 恒真；lenient 仅要求不变差；
    /// strict 额外要求 blocking 或 AI-tell 至少一项改善。
    pub fn should_apply(self, before: &MergedAuditEvaluation, after: &MergedAuditEvaluation) -> bool {
        let improved_blocking = after.blocking_count < before.blocking_count;
        let improved_ai_tells = after.ai_tell_count < before.ai_tell_count;
        let did_not_worsen = after.blocking_count <= before.blocking_count
            && after.critical_count <= before.critical_count
            && after.ai_tell_count <= before.ai_tell_count;
        match self {
            RevisionGate::Always => true,
            RevisionGate::Lenient => did_not_worsen,
            RevisionGate::Strict => did_not_worsen && (improved_blocking || improved_ai_tells),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::continuity::AuditChapterOptions;

    struct CannedAudit(AuditResult);

    #[async_trait]
    impl LlmAuditPort for CannedAudit {
        async fn audit(&self, _content: &str, _options: &AuditChapterOptions) -> Result<AuditResult, String> {
            Ok(self.0.clone())
        }
    }

    fn issue(severity: AuditSeverity, category: &str) -> AuditIssue {
        AuditIssue {
            severity,
            category: category.to_string(),
            description: format!("{category} problem"),
            suggestion: "fix it".to_string(),
            repair_scope: None,
        }
    }

    fn evaluation(
        passed: bool,
        issues: Vec<AuditIssue>,
        ai_tell_count: usize,
        critical: usize,
    ) -> MergedAuditEvaluation {
        let blocking_count = issues
            .iter()
            .filter(|i| matches!(i.severity, AuditSeverity::Warning | AuditSeverity::Critical))
            .count();
        MergedAuditEvaluation {
            audit_result: AuditResult {
                passed,
                issues: issues.clone(),
                summary: "s".to_string(),
                parse_failed: None,
                overall_score: None,
                token_usage: None,
            },
            ai_tell_count,
            blocking_count,
            critical_count: critical,
            revision_blocking_issues: issues,
        }
    }

    #[tokio::test]
    async fn merges_four_sources_and_excludes_fatigue_from_blocking() {
        let dir = tempfile::tempdir().unwrap();
        // 三段等长 → 段落长度 CV=0，命中 AI 痕迹“段落等长”维度（必触发）。
        let content = "他推开门，环顾四周，屋里空无一人。\n\n他放下行囊，点燃油灯，坐下休息。\n\n窗外风声呼啸，夜色渐深，万籁俱寂。";
        let llm = CannedAudit(AuditResult {
            passed: true,
            issues: vec![issue(AuditSeverity::Warning, "节奏")],
            summary: "ok".to_string(),
            parse_failed: None,
            overall_score: None,
            token_usage: None,
        });
        let eval = evaluate_merged_audit(
            &llm,
            dir.path(),
            content,
            1,
            WritingLanguage::Zh,
            &AuditChapterOptions::default(),
        )
        .await
        .unwrap();
        // issues = LLM(1) + AI-tell(≥1，然而) + 敏感词(0) + 疲劳(视摘要，通常 0)
        assert!(eval.audit_result.issues.len() >= 2);
        assert!(eval.audit_result.passed);
        assert!(eval.ai_tell_count >= 1);
        assert_eq!(eval.blocking_count, eval.revision_blocking_issues.len());
        // revisionBlockingIssues 里的 warning 数 = blocking_count（LLM 节奏 + AI-tell warning）。
        assert!(eval.blocking_count >= 2);
    }

    #[tokio::test]
    async fn blocked_sensitive_word_forces_fail() {
        let dir = tempfile::tempdir().unwrap();
        let content = "他说了一句法轮功，然后离开。";
        let llm = CannedAudit(AuditResult {
            passed: true,
            issues: vec![],
            summary: "clean".to_string(),
            parse_failed: None,
            overall_score: None,
            token_usage: None,
        });
        let eval = evaluate_merged_audit(
            &llm,
            dir.path(),
            content,
            1,
            WritingLanguage::Zh,
            &AuditChapterOptions::default(),
        )
        .await
        .unwrap();
        assert!(!eval.audit_result.passed);
        assert!(!eval.revision_blocking_issues.is_empty());
        assert!(eval.blocking_count >= 1);
    }

    #[test]
    fn restore_lost_issues_when_next_empty_and_failed() {
        let previous = AuditResult {
            passed: false,
            issues: vec![issue(AuditSeverity::Critical, "设定")],
            summary: "prev".to_string(),
            parse_failed: None,
            overall_score: None,
            token_usage: None,
        };
        let next = AuditResult {
            passed: false,
            issues: vec![],
            summary: String::new(),
            parse_failed: None,
            overall_score: None,
            token_usage: None,
        };
        let restored = restore_lost_audit_issues(&previous, &next);
        assert_eq!(restored.issues.len(), 1);
        assert_eq!(restored.summary, "prev");
    }

    #[test]
    fn restore_skipped_when_next_has_issues_or_passed() {
        let previous = AuditResult {
            passed: false,
            issues: vec![issue(AuditSeverity::Critical, "设定")],
            summary: "prev".into(),
            parse_failed: None,
            overall_score: None,
            token_usage: None,
        };
        let next_has_issue = AuditResult {
            passed: false,
            issues: vec![issue(AuditSeverity::Info, "对话")],
            ..previous.clone()
        };
        assert_eq!(restore_lost_audit_issues(&previous, &next_has_issue).issues.len(), 1);

        let next_passed = AuditResult { passed: true, issues: vec![], ..previous.clone() };
        assert!(restore_lost_audit_issues(&previous, &next_passed).issues.is_empty());
    }

    #[test]
    fn restore_actionable_carries_counts_from_previous() {
        let previous = evaluation(false, vec![issue(AuditSeverity::Critical, "设定")], 2, 1);
        let next = evaluation(false, vec![], 0, 0);
        let restored = restore_actionable_audit_if_lost(&previous, &next);
        assert_eq!(restored.blocking_count, previous.blocking_count);
        assert_eq!(restored.critical_count, 1);
        assert_eq!(restored.revision_blocking_issues.len(), 1);
        // aiTellCount 保留 next（TS 展开顺序：...next 在前，仅覆盖四个字段）。
        assert_eq!(restored.ai_tell_count, 0);
    }

    #[test]
    fn restore_actionable_early_return_keeps_next() {
        let previous = evaluation(false, vec![issue(AuditSeverity::Critical, "设定")], 2, 1);
        let next = evaluation(true, vec![], 0, 0);
        let restored = restore_actionable_audit_if_lost(&previous, &next);
        assert_eq!(restored.blocking_count, 0);
        assert_eq!(restored.revision_blocking_issues.len(), 0);
    }

    #[test]
    fn gate_matrix_matches_ts_semantics() {
        let before = evaluation(false, vec![issue(AuditSeverity::Critical, "a")], 3, 1);
        // 变差：blocking 上升。
        let worse = evaluation(false, vec![issue(AuditSeverity::Critical, "a"), issue(AuditSeverity::Warning, "b")], 3, 1);
        // 持平。
        let same = evaluation(false, vec![issue(AuditSeverity::Critical, "a")], 3, 1);
        // 改善：blocking 下降。
        let improved = evaluation(false, vec![], 3, 0);
        // 持平但 AI-tell 改善。
        let ai_improved = evaluation(false, vec![issue(AuditSeverity::Critical, "a")], 1, 1);

        assert!(!RevisionGate::Strict.should_apply(&before, &worse));
        assert!(!RevisionGate::Strict.should_apply(&before, &same));
        assert!(RevisionGate::Strict.should_apply(&before, &improved));
        assert!(RevisionGate::Strict.should_apply(&before, &ai_improved));

        assert!(!RevisionGate::Lenient.should_apply(&before, &worse));
        assert!(RevisionGate::Lenient.should_apply(&before, &same));

        assert!(RevisionGate::Always.should_apply(&before, &worse));
    }

    #[test]
    fn gate_parse_defaults_strict() {
        assert_eq!(RevisionGate::parse(None), RevisionGate::Strict);
        assert_eq!(RevisionGate::parse(Some("bogus")), RevisionGate::Strict);
        assert_eq!(RevisionGate::parse(Some("lenient")), RevisionGate::Lenient);
        assert_eq!(RevisionGate::parse(Some("always")), RevisionGate::Always);
    }
}
