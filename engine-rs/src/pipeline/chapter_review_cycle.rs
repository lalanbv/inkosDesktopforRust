//! chapter-review-cycle —— write-next 的审计→修订评分环。
//!
//! 移植自 `packages/core/src/pipeline/chapter-review-cycle.ts`（352 行）。
//! 评估（LLM 审计 + AI 味 + 敏感词 + 确定性后写检查 + 字数）→ 修订 → 再评估，
//! 默认一轮自动修复；快照择优（区间内优先，分数 ε 净提升阈值），必要时回退
//! 最高分版本。所有外部依赖经 trait/闭包注入（测试可全 mock）。
//!
//! ## 移植纪律
//! - 通过线 = `passed && score >= 85 && lengthInRange`；净提升阈值 ε = 3
//! - 长度**不进** reviser 问题清单——normalize 是专用步骤（环外/环首），
//!   `lengthInRange` 只作硬门控
//! - `postReviseCount = revisedWordCount`（TS 怪癖：存字数非轮次，逐字保留）
//! - 快照 reduce 无初值（首元素起步）；`revised = snapshots.len() > 1 &&
//!   finalContent !== initialContent`

use std::sync::Arc;

use async_trait::async_trait;

use crate::agents::continuity::{AuditIssue, AuditResult, AuditSeverity, AuditTokenUsage};
use crate::agents::reviser::{ReviseMode, ReviseOutput};
use crate::models::input_governance::{
    ChapterIntent, ChapterMemo, ContextPackage, RuleStack,
};
use crate::models::length_governance::LengthSpec;
use crate::utils::length_metrics::{count_chapter_length, is_outside_hard_range};

/// 默认自动修复轮次。对齐 TS `DEFAULT_MAX_REVIEW_ITERATIONS`。
pub const DEFAULT_MAX_REVIEW_ITERATIONS: usize = 1;
/// 通过线分数。对齐 TS `PASS_SCORE_THRESHOLD`。
pub const PASS_SCORE_THRESHOLD: u32 = 85;
/// 净提升阈值。对齐 TS `NET_IMPROVEMENT_EPSILON`。
pub const NET_IMPROVEMENT_EPSILON: u32 = 3;

/// 治理控制入参（planner/composer 产物子集）。
pub struct ChapterReviewCycleControlInput<'a> {
    pub chapter_intent: &'a str,
    pub chapter_memo: Option<&'a ChapterMemo>,
    pub chapter_intent_data: Option<&'a ChapterIntent>,
    pub context_package: &'a ContextPackage,
    pub rule_stack: &'a RuleStack,
}

/// 环结果。对齐 TS `ChapterReviewCycleResult`。
#[derive(Debug, Clone)]
pub struct ChapterReviewCycleResult {
    pub final_content: String,
    pub final_word_count: u32,
    pub pre_audit_normalized_word_count: u32,
    pub revised: bool,
    pub audit_result: AuditResult,
    pub total_usage: AuditTokenUsage,
    pub post_revise_count: u32,
    pub normalize_applied: bool,
}

/// 修订器端口（reviser::revise_chapter 的环内形态）。
#[async_trait]
pub trait CycleReviser: Send + Sync {
    async fn revise_chapter(
        &self,
        chapter_content: &str,
        issues: &[AuditIssue],
        control_input: Option<&ChapterReviewCycleControlInput<'_>>,
        length_spec: &LengthSpec,
    ) -> Result<ReviseOutput, String>;
}

/// 审计器端口（continuity audit 的环内形态）。
#[async_trait]
pub trait CycleAuditor: Send + Sync {
    async fn audit_chapter(
        &self,
        chapter_content: &str,
        control_input: Option<&ChapterReviewCycleControlInput<'_>>,
        temperature: Option<f64>,
    ) -> Result<AuditResult, String>;
}

/// 长度归一化端口（length-normalizer 的环内形态）。
pub struct NormalizeStepResult {
    pub content: String,
    pub word_count: u32,
    pub applied: bool,
    pub token_usage: Option<AuditTokenUsage>,
}

#[async_trait]
pub trait DraftLengthNormalizer: Send + Sync {
    async fn normalize(&self, chapter_content: &str) -> Result<NormalizeStepResult, String>;
}

/// 敏感词结果的最小消费面。
pub struct SensitiveScanResult {
    pub blocked: bool,
    pub issues: Vec<AuditIssue>,
}

/// 同步回调的类型别名（消除复杂类型告警）。
pub type SurfaceFn = Arc<dyn Fn(&str) -> String + Send + Sync>;
pub type AssertFn = Arc<dyn Fn(&str, &str) -> Result<(), String> + Send + Sync>;
pub type IssuesFn = Arc<dyn Fn(&str) -> Vec<AuditIssue> + Send + Sync>;
pub type ScanFn = Arc<dyn Fn(&str) -> SensitiveScanResult + Send + Sync>;
pub type LogFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// 同步回调集合（纯函数注入）。
pub struct ReviewCycleCallbacks {
    /// 表面净化（normalizePostWriteSurface；可选）。
    pub normalize_post_write_surface: Option<SurfaceFn>,
    /// 空内容断言（抛错语义 → Err）。
    pub assert_chapter_content_not_empty: AssertFn,
    pub analyze_ai_tells: IssuesFn,
    pub analyze_sensitive_words: ScanFn,
    /// 确定性后写检查（每轮重跑；不提供则回退初始 postWriteErrors）。
    pub run_post_write_checks: Option<IssuesFn>,
    pub log_warn: LogFn,
    pub log_stage: LogFn,
}

/// 环入参。
pub struct ReviewCycleParams<'a> {
    pub book_dir: &'a std::path::Path,
    pub chapter_number: u32,
    pub initial_content: &'a str,
    pub initial_word_count: u32,
    pub initial_post_write_errors: &'a [crate::agents::post_write_validator::PostWriteViolation],
    pub reduced_control_input: Option<ChapterReviewCycleControlInput<'a>>,
    pub length_spec: &'a LengthSpec,
    pub initial_usage: AuditTokenUsage,
    pub reviser: &'a dyn CycleReviser,
    pub auditor: &'a dyn CycleAuditor,
    pub normalizer: &'a dyn DraftLengthNormalizer,
    pub callbacks: ReviewCycleCallbacks,
    pub max_review_iterations: Option<usize>,
}

fn add_usage(left: &AuditTokenUsage, right: Option<&AuditTokenUsage>) -> AuditTokenUsage {
    match right {
        None => *left,
        Some(right) => AuditTokenUsage {
            prompt_tokens: left.prompt_tokens + right.prompt_tokens,
            completion_tokens: left.completion_tokens + right.completion_tokens,
            total_tokens: left.total_tokens + right.total_tokens,
        },
    }
}

struct Assessment {
    audit_result: AuditResult,
    score: u32,
    length_in_range: bool,
}

struct ReviewSnapshot {
    content: String,
    word_count: u32,
    audit_result: AuditResult,
    score: u32,
    length_in_range: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ReviewCycleError {
    #[error("reviser failed: {0}")]
    Reviser(String),
    #[error("auditor failed: {0}")]
    Auditor(String),
    #[error("length normalizer failed: {0}")]
    Normalizer(String),
    #[error("chapter content check failed at `{stage}`: {message}")]
    EmptyContent { stage: String, message: String },
}

fn is_passed(assessment: &Assessment) -> bool {
    assessment.audit_result.passed
        && assessment.score >= PASS_SCORE_THRESHOLD
        && assessment.length_in_range
}

/// 审计→修订评分环主入口。
pub async fn run_chapter_review_cycle(
    params: ReviewCycleParams<'_>,
) -> Result<ChapterReviewCycleResult, ReviewCycleError> {
    let mut total_usage = params.initial_usage;
    let mut normalize_applied = false;
    let mut final_content = params.initial_content.to_string();
    let mut final_word_count;

    // 初始 postWriteErrors 转 AuditIssue（runPostWriteChecks 缺席时的回退源）。
    let initial_post_write_issues: Vec<AuditIssue> = params
        .initial_post_write_errors
        .iter()
        .map(|violation| AuditIssue {
            severity: AuditSeverity::Critical,
            category: violation.rule.clone(),
            description: violation.description.clone(),
            suggestion: violation.suggestion.clone(),
            repair_scope: None,
        })
        .collect();

    let counting_mode = params.length_spec.counting_mode;

    // 长度归一化：专用步骤，仅明显硬区间漂移时触发；不混入 reviser 问题。
    let word_count_now = count_chapter_length(&final_content, counting_mode);
    let normalized_before_audit = if !is_outside_hard_range(
        word_count_now,
        params.length_spec.hard_min,
        params.length_spec.hard_max,
    ) {
        (
            final_content.clone(),
            count_chapter_length(&final_content, counting_mode),
            false,
        )
    } else {
        let result = params
            .normalizer
            .normalize(&final_content)
            .await
            .map_err(ReviewCycleError::Normalizer)?;
        total_usage = add_usage(&total_usage, result.token_usage.as_ref());
        (result.content, result.word_count, result.applied)
    };
    final_content = match &params.callbacks.normalize_post_write_surface {
        Some(surface) => surface(&normalized_before_audit.0),
        None => normalized_before_audit.0,
    };
    final_word_count = count_chapter_length(&final_content, counting_mode);
    normalize_applied = normalize_applied || normalized_before_audit.2;
    (params.callbacks.assert_chapter_content_not_empty)(&final_content, "draft generation")
        .map_err(|message| ReviewCycleError::EmptyContent {
            stage: "draft generation".to_string(),
            message,
        })?;

    // 评估：审计 + 确定性检查 + 字数 + 分数。
    async fn assess_content(
        params: &ReviewCycleParams<'_>,
        content: &str,
        temperature: Option<f64>,
        total_usage: &mut AuditTokenUsage,
        initial_post_write_issues: &[AuditIssue],
    ) -> Result<Assessment, ReviewCycleError> {
        let llm_audit = params
            .auditor
            .audit_chapter(content, params.reduced_control_input.as_ref(), temperature)
            .await
            .map_err(ReviewCycleError::Auditor)?;
        *total_usage = add_usage(total_usage, llm_audit.token_usage.as_ref());
        let ai_tells_issues = (params.callbacks.analyze_ai_tells)(content);
        let sensitive_result = (params.callbacks.analyze_sensitive_words)(content);
        let has_blocked_words = sensitive_result.blocked;
        let word_count = count_chapter_length(content, params.length_spec.counting_mode);
        let length_in_range = !is_outside_hard_range(
            word_count,
            params.length_spec.hard_min,
            params.length_spec.hard_max,
        );

        // 确定性后写检查：每轮重跑（提供时）；否则回退初始 postWriteErrors。
        let post_write_issues: Vec<AuditIssue> = match &params.callbacks.run_post_write_checks {
            Some(checks) => checks(content),
            None => initial_post_write_issues.to_vec(),
        };

        let mut all_issues: Vec<AuditIssue> = llm_audit.issues.clone();
        all_issues.extend(ai_tells_issues);
        all_issues.extend(sensitive_result.issues);
        all_issues.extend(post_write_issues.clone());

        // 长度不进 reviser 问题——normalize 专用；lengthInRange 只作硬门控。
        let has_post_write_critical = post_write_issues
            .iter()
            .any(|issue| issue.severity == AuditSeverity::Critical);
        let audit_result = AuditResult {
            passed: if has_blocked_words || has_post_write_critical {
                false
            } else {
                llm_audit.passed
            },
            issues: all_issues,
            summary: llm_audit.summary.clone(),
            parse_failed: llm_audit.parse_failed,
            overall_score: llm_audit.overall_score,
            token_usage: None,
        };

        let score = llm_audit.overall_score.unwrap_or(0);
        Ok(Assessment { audit_result, score, length_in_range })
    }

    (params.callbacks.log_stage)("审计草稿", "auditing draft");
    let initial = assess_content(
        &params,
        &final_content,
        None,
        &mut total_usage,
        &initial_post_write_issues,
    )
    .await?;

    let mut snapshots: Vec<ReviewSnapshot> = vec![ReviewSnapshot {
        content: final_content.clone(),
        word_count: final_word_count,
        audit_result: initial.audit_result.clone(),
        score: initial.score,
        length_in_range: initial.length_in_range,
    }];

    let mut current_audit = initial;
    let mut post_revise_count: u32 = 0;

    if current_audit
        .audit_result
        .parse_failed
        .unwrap_or(false)
    {
        (params.callbacks.log_warn)(
            "审稿输出解析失败，跳过自动修稿以避免误改正文",
            "Audit output parsing failed; skipping automatic repair to avoid rewriting valid prose from an unreliable audit.",
        );
        return Ok(ChapterReviewCycleResult {
            final_content,
            final_word_count,
            pre_audit_normalized_word_count: final_word_count,
            revised: false,
            audit_result: current_audit.audit_result,
            total_usage,
            post_revise_count,
            normalize_applied,
        });
    }

    let max_review_iterations = params
        .max_review_iterations
        .unwrap_or(DEFAULT_MAX_REVIEW_ITERATIONS);

    if !is_passed(&current_audit) {
        for iteration in 0..max_review_iterations {
            (params.callbacks.log_stage)(
                &format!(
                    "修复轮次 {}/{}（当前 {} 分）",
                    iteration + 1,
                    max_review_iterations,
                    current_audit.score
                ),
                &format!(
                    "repair iteration {}/{} (current score: {})",
                    iteration + 1,
                    max_review_iterations,
                    current_audit.score
                ),
            );

            let revise_output = params
                .reviser
                .revise_chapter(
                    &final_content,
                    &current_audit.audit_result.issues,
                    params.reduced_control_input.as_ref(),
                    params.length_spec,
                )
                .await
                .map_err(ReviewCycleError::Reviser)?;
            total_usage = add_usage(&total_usage, revise_output.token_usage.as_ref());

            if revise_output.revised_content.is_empty()
                || revise_output.revised_content == final_content
            {
                (params.callbacks.log_warn)(
                    &format!("修复轮次 {} 未产出新内容，退出循环", iteration + 1),
                    &format!(
                        "repair iteration {} produced no new content, exiting loop",
                        iteration + 1
                    ),
                );
                break;
            }

            (params
                .callbacks
                .assert_chapter_content_not_empty)(&revise_output.revised_content, "repair")
                .map_err(|message| ReviewCycleError::EmptyContent {
                    stage: format!("repair iteration {}", iteration + 1),
                    message,
                })?;
            let revised_content = match &params.callbacks.normalize_post_write_surface {
                Some(surface) => surface(&revise_output.revised_content),
                None => revise_output.revised_content.clone(),
            };
            let revised_word_count = count_chapter_length(&revised_content, counting_mode);

            // 复评修订内容。REVISED_CONTENT 字数漂移 → lengthInRange=false →
            // isPassed 失败 → bestSnapshot 选回区间内旧版。环内不再 normalize。
            let next_assessment = assess_content(
                &params,
                &revised_content,
                Some(0.0),
                &mut total_usage,
                &initial_post_write_issues,
            )
            .await?;

            snapshots.push(ReviewSnapshot {
                content: revised_content.clone(),
                word_count: revised_word_count,
                audit_result: next_assessment.audit_result.clone(),
                score: next_assessment.score,
                length_in_range: next_assessment.length_in_range,
            });

            if is_passed(&next_assessment) {
                (params.callbacks.log_stage)(
                    &format!("修复后达到通过线（{} 分），退出循环", next_assessment.score),
                    &format!(
                        "repair reached pass threshold ({}), exiting loop",
                        next_assessment.score
                    ),
                );
                final_content = revised_content;
                final_word_count = revised_word_count;
                post_revise_count = revised_word_count;
                current_audit = next_assessment;
                break;
            }

            if next_assessment.score >= current_audit.score + NET_IMPROVEMENT_EPSILON {
                final_content = revised_content;
                final_word_count = revised_word_count;
                post_revise_count = revised_word_count;
                current_audit = next_assessment;
                // 继续下一轮。
            } else {
                (params.callbacks.log_warn)(
                    &format!(
                        "修复轮次 {} 未净提升（{} → {}），退出循环",
                        iteration + 1,
                        current_audit.score,
                        next_assessment.score
                    ),
                    &format!(
                        "repair iteration {} no net improvement ({} → {}), exiting loop",
                        iteration + 1,
                        current_audit.score,
                        next_assessment.score
                    ),
                );
                break;
            }
        }
    }

    // 快照择优：区间内优先；同区间分数 ε 净提升才换。
    let best_snapshot = snapshots
        .iter()
        .skip(1)
        .fold(&snapshots[0], |best, snap| {
            if snap.length_in_range != best.length_in_range {
                return if snap.length_in_range { snap } else { best };
            }
            if snap.score >= best.score + NET_IMPROVEMENT_EPSILON {
                snap
            } else {
                best
            }
        });

    // 最优快照与当前不同（修复变差但旧版更好）→ 回退。
    let should_restore = best_snapshot.content != final_content
        && ((best_snapshot.length_in_range && !current_audit.length_in_range)
            || best_snapshot.score >= current_audit.score + NET_IMPROVEMENT_EPSILON);
    if should_restore {
        (params.callbacks.log_warn)(
            &format!(
                "回退到最高分版本（{} 分 vs 当前 {} 分）",
                best_snapshot.score, current_audit.score
            ),
            &format!(
                "rolling back to highest-scoring version ({} vs current {})",
                best_snapshot.score, current_audit.score
            ),
        );
        final_content = best_snapshot.content.clone();
        final_word_count = best_snapshot.word_count;
        current_audit = Assessment {
            audit_result: best_snapshot.audit_result.clone(),
            score: best_snapshot.score,
            length_in_range: best_snapshot.length_in_range,
        };
    }

    Ok(ChapterReviewCycleResult {
        pre_audit_normalized_word_count: final_word_count,
        revised: snapshots.len() > 1 && final_content != params.initial_content,
        final_content,
        final_word_count,
        audit_result: current_audit.audit_result,
        total_usage,
        post_revise_count,
        normalize_applied,
    })
}

/// 长度检查辅助：暴露给调用方复用的边界判断。
pub fn length_in_range(count: u32, spec: &LengthSpec) -> bool {
    !is_outside_hard_range(count, spec.hard_min, spec.hard_max)
}

#[allow(unused)]
fn _revise_mode_default() -> ReviseMode {
    ReviseMode::Auto
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::length_governance::{
        LengthCountingMode, LengthNormalizeMode, LengthSpec,
    };
    use std::sync::Mutex;

    fn spec() -> LengthSpec {
        // 测试内容是短句——硬区间放宽到 1-10000 使 lengthInRange 恒真，
        // 聚焦环逻辑而非字数门控。
        LengthSpec {
            target: 30,
            soft_min: 20,
            soft_max: 40,
            hard_min: 1,
            hard_max: 10_000,
            counting_mode: LengthCountingMode::ZhChars,
            normalize_mode: LengthNormalizeMode::None,
        }
    }

    fn audit(passed: bool, score: u32, issues: Vec<AuditIssue>) -> AuditResult {
        AuditResult {
            passed,
            issues,
            summary: "summary".into(),
            parse_failed: None,
            overall_score: Some(score),
            token_usage: None,
        }
    }

    fn violation(rule: &str) -> crate::agents::post_write_validator::PostWriteViolation {
        crate::agents::post_write_validator::PostWriteViolation {
            rule: rule.into(),
            severity: crate::agents::post_write_validator::ViolationSeverity::Error,
            description: format!("{rule} 违规"),
            suggestion: "改掉".into(),
        }
    }

    /// 审计脚本：按调用序返回预置结果。
    struct ScriptAuditor {
        results: Mutex<Vec<AuditResult>>,
    }

    #[async_trait]
    impl CycleAuditor for ScriptAuditor {
        async fn audit_chapter(
            &self,
            _content: &str,
            _control: Option<&ChapterReviewCycleControlInput<'_>>,
            _temperature: Option<f64>,
        ) -> Result<AuditResult, String> {
            Ok(self.results.lock().unwrap().remove(0))
        }
    }

    /// 修订器脚本：返回固定新内容，记录收到的 issues。
    struct ScriptReviser {
        revised: String,
        seen_issue_count: Mutex<Vec<usize>>,
    }

    #[async_trait]
    impl CycleReviser for ScriptReviser {
        async fn revise_chapter(
            &self,
            _content: &str,
            issues: &[AuditIssue],
            _control: Option<&ChapterReviewCycleControlInput<'_>>,
            _spec: &LengthSpec,
        ) -> Result<ReviseOutput, String> {
            self.seen_issue_count.lock().unwrap().push(issues.len());
            Ok(ReviseOutput {
                revised_content: self.revised.clone(),
                ..Default::default()
            })
        }
    }

    struct NoopNormalizer {
        applied: bool,
    }

    #[async_trait]
    impl DraftLengthNormalizer for NoopNormalizer {
        async fn normalize(&self, content: &str) -> Result<NormalizeStepResult, String> {
            Ok(NormalizeStepResult {
                content: content.to_string(),
                word_count: count_chapter_length(content, LengthCountingMode::ZhChars),
                applied: self.applied,
                token_usage: None,
            })
        }
    }

    fn callbacks() -> ReviewCycleCallbacks {
        ReviewCycleCallbacks {
            normalize_post_write_surface: None,
            assert_chapter_content_not_empty: Arc::new(|content, _stage| {
                if content.trim().is_empty() {
                    Err("chapter content is empty".to_string())
                } else {
                    Ok(())
                }
            }),
            analyze_ai_tells: Arc::new(|_| Vec::new()),
            analyze_sensitive_words: Arc::new(|_| SensitiveScanResult {
                blocked: false,
                issues: Vec::new(),
            }),
            run_post_write_checks: None,
            log_warn: Arc::new(|_zh, _en| {}),
            log_stage: Arc::new(|_zh, _en| {}),
        }
    }

    fn params<'a>(
        content: &'a str,
        post_write: &'a [crate::agents::post_write_validator::PostWriteViolation],
        auditor: &'a ScriptAuditor,
        reviser: &'a ScriptReviser,
        normalizer: &'a NoopNormalizer,
        spec: &'a LengthSpec,
        max_iterations: Option<usize>,
    ) -> ReviewCycleParams<'a> {
        ReviewCycleParams {
            book_dir: std::path::Path::new("/tmp"),
            chapter_number: 3,
            initial_content: content,
            initial_word_count: count_chapter_length(content, LengthCountingMode::ZhChars),
            initial_post_write_errors: post_write,
            reduced_control_input: None,
            length_spec: spec,
            initial_usage: AuditTokenUsage { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
            reviser,
            auditor,
            normalizer,
            callbacks: callbacks(),
            max_review_iterations: max_iterations,
        }
    }

    const PASS_CONTENT: &str = "他推门而入，烛火摇曳。真相在案上的账册里静静躺着，等待被翻开的那一刻。";

    #[tokio::test]
    async fn passes_immediately_when_score_meets_threshold() {
        let auditor = ScriptAuditor {
            results: Mutex::new(vec![audit(true, 90, vec![])]),
        };
        let reviser = ScriptReviser {
            revised: "不该被用到".into(),
            seen_issue_count: Mutex::new(Vec::new()),
        };
        let normalizer = NoopNormalizer { applied: false };
        let sp = spec();
        let result = run_chapter_review_cycle(params(
            PASS_CONTENT,
            &[],
            &auditor,
            &reviser,
            &normalizer,
            &sp,
            None,
        ))
        .await
        .unwrap();
        assert!(!result.revised);
        assert_eq!(result.final_content, PASS_CONTENT);
        assert_eq!(result.post_revise_count, 0);
        assert!(reviser.seen_issue_count.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn parse_failure_skips_auto_repair() {
        let mut failed = audit(false, 40, vec![]);
        failed.parse_failed = Some(true);
        let auditor = ScriptAuditor {
            results: Mutex::new(vec![failed]),
        };
        let reviser = ScriptReviser {
            revised: "x".into(),
            seen_issue_count: Mutex::new(Vec::new()),
        };
        let normalizer = NoopNormalizer { applied: false };
        let sp = spec();
        let result = run_chapter_review_cycle(params(
            PASS_CONTENT,
            &[],
            &auditor,
            &reviser,
            &normalizer,
            &sp,
            Some(3),
        ))
        .await
        .unwrap();
        assert!(!result.revised);
        assert_eq!(result.final_content, PASS_CONTENT);
        assert!(result.audit_result.parse_failed.unwrap_or(false));
        assert!(reviser.seen_issue_count.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn post_write_errors_feed_first_assessment_as_critical() {
        // 初始 postWriteErrors 含 error 级违规 → 首评 passed 被压为 false。
        let auditor = ScriptAuditor {
            results: Mutex::new(vec![audit(true, 90, vec![]), audit(true, 95, vec![])]),
        };
        let reviser = ScriptReviser {
            revised: format!("{PASS_CONTENT}修复后追加的一句收尾。"),
            seen_issue_count: Mutex::new(Vec::new()),
        };
        let normalizer = NoopNormalizer { applied: false };
        let sp = spec();
        let result = run_chapter_review_cycle(params(
            PASS_CONTENT,
            &[violation("chapter-ref")],
            &auditor,
            &reviser,
            &normalizer,
            &sp,
            None,
        ))
        .await
        .unwrap();
        // 修订器收到 postWriteErrors 转来的 critical issue。
        assert_eq!(*reviser.seen_issue_count.lock().unwrap(), vec![1]);
        assert!(result.revised);
        assert!(result.final_content.contains("修复后追加"));
        // 环结果汇总里包含该 critical。
        assert!(result
            .audit_result
            .issues
            .iter()
            .any(|issue| issue.severity == AuditSeverity::Critical));
    }

    #[tokio::test]
    async fn repair_reaches_threshold_and_exits() {
        let auditor = ScriptAuditor {
            results: Mutex::new(vec![audit(false, 60, vec![]), audit(true, 88, vec![])]),
        };
        let reviser = ScriptReviser {
            revised: format!("{PASS_CONTENT}第二轮修复了主线偏移，并补上钩子兑现。"),
            seen_issue_count: Mutex::new(Vec::new()),
        };
        let normalizer = NoopNormalizer { applied: false };
        let sp = spec();
        let result = run_chapter_review_cycle(params(
            PASS_CONTENT,
            &[],
            &auditor,
            &reviser,
            &normalizer,
            &sp,
            None,
        ))
        .await
        .unwrap();
        assert!(result.revised);
        assert!(result.audit_result.passed);
        assert!(result.audit_result.overall_score.unwrap() >= PASS_SCORE_THRESHOLD);
        // TS 怪癖：postReviseCount = 修订稿字数。
        assert_eq!(
            result.post_revise_count,
            count_chapter_length(&result.final_content, LengthCountingMode::ZhChars)
        );
    }

    #[tokio::test]
    async fn no_net_improvement_exits_and_keeps_current() {
        let auditor = ScriptAuditor {
            results: Mutex::new(vec![audit(false, 60, vec![]), audit(false, 61, vec![])]),
        };
        let reviser = ScriptReviser {
            revised: format!("{PASS_CONTENT}小幅改动但分数没怎么动。"),
            seen_issue_count: Mutex::new(Vec::new()),
        };
        let normalizer = NoopNormalizer { applied: false };
        let sp = spec();
        let result = run_chapter_review_cycle(params(
            PASS_CONTENT,
            &[],
            &auditor,
            &reviser,
            &normalizer,
            &sp,
            Some(5),
        ))
        .await
        .unwrap();
        // 61 < 60 + 3 → 无净提升退出；修订稿因 >= 当前被保留为 final（同区间无 ε 提升
        // 不触发快照回退——快照择优仅当 best >= current + ε）。
        assert_eq!(*reviser.seen_issue_count.lock().unwrap(), vec![0]);
        // 无净提升退出：final 保持当前版（60），61 的快照不足 ε 不置换。
        assert_eq!(result.audit_result.overall_score, Some(60));
    }

    #[tokio::test]
    async fn reviser_no_new_content_exits_loop() {
        let auditor = ScriptAuditor {
            results: Mutex::new(vec![audit(false, 60, vec![])]),
        };
        let reviser = ScriptReviser {
            revised: PASS_CONTENT.to_string(), // 与原文相同 → 无新内容
            seen_issue_count: Mutex::new(Vec::new()),
        };
        let normalizer = NoopNormalizer { applied: false };
        let sp = spec();
        let result = run_chapter_review_cycle(params(
            PASS_CONTENT,
            &[],
            &auditor,
            &reviser,
            &normalizer,
            &sp,
            Some(5),
        ))
        .await
        .unwrap();
        assert!(!result.revised);
        assert_eq!(result.final_content, PASS_CONTENT);
    }

    #[tokio::test]
    async fn empty_content_assertion_propagates() {
        // 空串走「无新内容退出」分支（TS 先判空再断言）；纯空白才触发断言。
        let auditor = ScriptAuditor {
            results: Mutex::new(vec![audit(false, 60, vec![])]),
        };
        let reviser = ScriptReviser {
            revised: "   \n  ".to_string(),
            seen_issue_count: Mutex::new(Vec::new()),
        };
        let normalizer = NoopNormalizer { applied: false };
        let sp = spec();
        let result = run_chapter_review_cycle(params(
            PASS_CONTENT,
            &[],
            &auditor,
            &reviser,
            &normalizer,
            &sp,
            None,
        ))
        .await;
        assert!(matches!(
            result,
            Err(ReviewCycleError::EmptyContent { stage, .. }) if stage == "repair iteration 1"
        ));
    }

    #[tokio::test]
    async fn surface_normalizer_applies_before_audit() {
        let auditor = ScriptAuditor {
            results: Mutex::new(vec![audit(true, 90, vec![])]),
        };
        let reviser = ScriptReviser {
            revised: "x".into(),
            seen_issue_count: Mutex::new(Vec::new()),
        };
        let normalizer = NoopNormalizer { applied: true };
        let sp = spec();
        let params = ReviewCycleParams {
            book_dir: std::path::Path::new("/tmp"),
            chapter_number: 3,
            initial_content: PASS_CONTENT,
            initial_word_count: 40,
            initial_post_write_errors: &[],
            reduced_control_input: None,
            length_spec: &sp,
            initial_usage: AuditTokenUsage { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
            reviser: &reviser,
            auditor: &auditor,
            normalizer: &normalizer,
            callbacks: ReviewCycleCallbacks {
                normalize_post_write_surface: Some(Arc::new(|content| {
                    format!("【净化】{content}")
                })),
                assert_chapter_content_not_empty: Arc::new(|_, _| Ok(())),
                analyze_ai_tells: Arc::new(|_| Vec::new()),
                analyze_sensitive_words: Arc::new(|_| SensitiveScanResult {
                    blocked: false,
                    issues: Vec::new(),
                }),
                run_post_write_checks: None,
                log_warn: Arc::new(|_zh, _en| {}),
                log_stage: Arc::new(|_zh, _en| {}),
            },
            max_review_iterations: None,
        };
        let result = run_chapter_review_cycle(params).await.unwrap();
        assert!(result.final_content.starts_with("【净化】"));
        // normalize_applied 只反映长度归一化（宽硬区间未触发）；
        // 表面净化是独立回调，不计入该标志（TS 同语义）。
        assert!(!result.normalize_applied);
    }
}
