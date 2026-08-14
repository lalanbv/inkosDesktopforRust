//! write_next —— writeNextChapter 主体装配（write-next 链收官）。
//!
//! 移植自 `packages/core/src/pipeline/runner.ts` 的 `writeNextChapter` /
//! `_writeNextChapterLocked` / `prepareWriteInput` 主链（auto + manual 双模式）：
//! 控制文档 → book 配置 → 降级守卫 → 章号 → 输入准备（v2 治理：plan 持久化
//! 复用 + composer 三件；legacy：仅 externalContext）→ writeChapter →
//! manual 写完即停 / review-cycle → promotion pass → 标题去重双轮 →
//! buildPersistenceOutput → 长跨度疲劳 → truth-validation → 段落形态 →
//! persistChapterArtifacts。
//!
//! 通知/webhook 与 SSE 广播经回调注入（42 号 server 层接线）。
//!
//! ## 与 TS 的已备案差异
//! - **互斥**：TS acquireBookLock 文件锁 → 进程内 per-book tokio 锁
//!   （41 号备案；strangler 分域下 Node/Rust 不并发写同一书）
//! - **通知/webhook**：`notify_channels`/`emit_webhook` 回调注入，失败不阻断
//!   主链（TS dispatchNotification 异步触发语义）
//! - **normalizeDraftLengthIfNeeded**：临时构造 length-normalizer 端口适配
//!   （TS 在 runner 内联创建 LengthNormalizerAgent）

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::agents::ai_tells::analyze_ai_tells;
use crate::agents::chapter_analyzer::{ChapterAnalyzerChat, ChapterAnalyzerCtx};
use crate::agents::composer::{
    compose_governed_chapter, ComposeChapterError, ComposeChapterInput, ComposeChapterOutput,
    ComposerChat, ContextBudget, LlmContextCompiler,
    LlmOutlineSelector,
};
use crate::agents::continuity::{AuditIssue, AuditResult, AuditSeverity};
use crate::agents::length_normalizer::{normalize_chapter, LengthNormalizerChat, NormalizeLengthInput};
use crate::agents::planner::{plan_chapter, PlanChapterError, PlanChapterInput, PlannerChat};
use crate::agents::post_write_validator::{
    detect_paragraph_length_drift, normalize_post_write_surface, resolve_duplicate_title,
    validate_post_write, PostWriteViolation, ViolationSeverity,
};
use crate::agents::reviser::{revise_chapter as reviser_revise_chapter, ReviseChapterError, ReviseMode, ReviseOptions, ReviseOutput, ReviserChat, ReviserCtx};
use crate::agents::rules_reader::read_book_rules;
use crate::agents::sensitive_words::analyze_sensitive_words;
use crate::agents::state_validator::{StateValidationAuthorityContext, StateValidatorChat};
use crate::agents::writer::{
    save_chapter, save_new_truth_files, write_chapter, WriteChapterError, WriteChapterInput, WriteChapterOutput, WriterChat,
    WriterCtx,
};
use crate::models::book::BookConfig;
use crate::models::input_governance::{ChapterIntent, ChapterMemo, ContextPackage, RuleStack};
use crate::models::length_governance::{LengthCountingMode, LengthSpec};
use crate::pipeline::build_persistence_output::{
    build_persistence_output, BuildPersistenceOutputParams,
};
use crate::pipeline::chapter_persistence::{
    persist_chapter_artifacts, PersistChapterArtifactsParams, PersistenceHooks,
};
use crate::pipeline::chapter_review_cycle::{
    run_chapter_review_cycle, ChapterReviewCycleControlInput,
    CycleAuditor, CycleReviser, DraftLengthNormalizer, NormalizeStepResult, ReviewCycleCallbacks,
    ReviewCycleError, ReviewCycleParams, SensitiveScanResult,
};
use crate::pipeline::chapter_state_recovery::{ControlInput, SettlePort, ValidatePort};
use crate::pipeline::chapter_truth_validation::{
    validate_chapter_truth_persistence, TruthValidationParams,
};
use crate::pipeline::persisted_governed_plan::{
    load_persisted_plan, save_persisted_plan,
};
use crate::state::manager::StateManager;
use crate::state::store::StateStore;
use crate::utils::hook_ledger_validator::validate_hook_ledger;
use crate::utils::hook_promotion::rerun_promotion_pass;
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{
    build_length_spec, count_chapter_length, is_outside_hard_range,
};
use crate::utils::long_span_fatigue::analyze_long_span_fatigue;
use crate::utils::story_markdown::{parse_pending_hooks_markdown, render_hook_snapshot};

/// write-next 结果。对齐 TS `ChapterPipelineResult`（writeNext 面）。
#[derive(Debug, Clone)]
pub struct ChapterPipelineResult {
    pub chapter_number: u32,
    pub title: String,
    pub word_count: u32,
    pub audit_result: AuditResult,
    pub revised: bool,
    pub status: &'static str,
    pub length_warnings: Vec<String>,
    pub length_telemetry: Option<crate::models::length_governance::LengthTelemetry>,
    pub token_usage: crate::agents::writer::TokenUsage,
}

/// 章节审核模式。对齐 TS `chapterReviewMode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChapterReviewMode {
    #[default]
    Auto,
    Manual,
}

/// 输入治理模式。对齐 TS `inputGovernanceMode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputGovernanceMode {
    #[default]
    V2,
    Legacy,
}

/// runner 配置面。
pub struct WriteNextConfig {
    pub chapter_review_mode: ChapterReviewMode,
    pub writing_review_retries: usize,
    pub input_governance_mode: InputGovernanceMode,
}

impl Default for WriteNextConfig {
    fn default() -> Self {
        WriteNextConfig {
            chapter_review_mode: ChapterReviewMode::Auto,
            writing_review_retries: 1,
            input_governance_mode: InputGovernanceMode::V2,
        }
    }
}

/// write-next 的 LLM 端口聚合（生产实现按 agent 分模型；测试全 mock）。
pub struct WriteNextAgents<'a> {
    pub writer: &'a dyn WriterChat,
    pub planner: &'a dyn PlannerChat,
    /// composer 的大纲选段与压缩编译共用一个 chat 端口。
    pub composer: &'a dyn ComposerChat,
    pub reviser: &'a dyn ReviserChat,
    /// 审核器环内端口（测试 mock 或 43 号最小协议）。
    pub auditor: &'a dyn CycleAuditor,
    /// 完整审计器（Some 时为 auto 环主链——真实 audit_chapter 编排，
    /// 调用方用 book 上下文构造 FullCycleAuditor；None 回退 auditor）。
    pub full_auditor: Option<crate::llm::agent_router::FullCycleAuditor>,
    pub normalizer: &'a dyn LengthNormalizerChat,
    pub analyzer: &'a dyn ChapterAnalyzerChat,
    pub state_validator: &'a dyn StateValidatorChat,
    /// settle 端口（writer.settleChapterState 的链内形态，truth-validation 用）。
    pub settler: &'a dyn SettlePort,
}

/// write-next 环境依赖。
pub type NotifyFn = Arc<dyn Fn(&ChapterPipelineResult, &BookConfig) + Send + Sync>;

pub struct WriteNextCtx<'a> {
    pub project_root: &'a Path,
    pub builtin_genres_dir: &'a Path,
    pub prompt_store: &'a dyn StateStore,
    pub state_store: &'a dyn StateStore,
    /// 预算（None 跳过压缩控制）。
    pub context_budget: Option<ContextBudget>,
    /// 通知回调（失败不阻断）。
    pub notify: Option<NotifyFn>,
}

#[derive(Debug, thiserror::Error)]
pub enum WriteNextError {
    #[error("latest chapter {0} is state-degraded. Repair state or rewrite that chapter before continuing.")]
    PendingStateRepair(u32),
    #[error("chapter content is empty after `{stage}`")]
    EmptyChapterContent { stage: String },
    #[error(transparent)]
    State(#[from] crate::state::manager::StateManagerError),
    #[error("plan chapter failed: {0}")]
    Plan(String),
    #[error("compose failed: {0}")]
    Compose(String),
    #[error("write chapter failed: {0}")]
    Write(String),
    #[error("review cycle failed: {0}")]
    Review(String),
    #[error("build persistence output failed: {0}")]
    Analyzer(String),
    #[error("truth validation failed: {0}")]
    TruthValidation(String),
    #[error("persistence failed: {0}")]
    Persistence(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<PlanChapterError> for WriteNextError {
    fn from(error: PlanChapterError) -> Self {
        WriteNextError::Plan(error.to_string())
    }
}
impl From<ComposeChapterError> for WriteNextError {
    fn from(error: ComposeChapterError) -> Self {
        WriteNextError::Compose(error.to_string())
    }
}
impl From<WriteChapterError> for WriteNextError {
    fn from(error: WriteChapterError) -> Self {
        WriteNextError::Write(error.to_string())
    }
}
impl From<ReviewCycleError> for WriteNextError {
    fn from(error: ReviewCycleError) -> Self {
        WriteNextError::Review(error.to_string())
    }
}

/// 进程内 per-book 互斥（TS 文件锁的 41 号备案等价物）。
fn book_locks() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    static LOCKS: std::sync::OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
        std::sync::OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn acquire_book_lock(book_id: &str) -> Arc<Mutex<()>> {
    let lock = {
        let mut locks = book_locks().lock().await;
        locks.entry(book_id.to_string()).or_default().clone()
    };
    lock
}

/// prepareWriteInput 产物（v2 治理三件或 legacy 空集）。
struct PreparedWriteInput {
    chapter_intent: Option<String>,
    chapter_memo: Option<ChapterMemo>,
    chapter_intent_data: Option<ChapterIntent>,
    context_package: Option<ContextPackage>,
    rule_stack: Option<RuleStack>,
}

/// writeNextChapter 主入口（锁 + 装配）。
#[allow(clippy::too_many_arguments)]
pub async fn write_next_chapter(
    state: &StateManager,
    agents: &WriteNextAgents<'_>,
    ctx: &WriteNextCtx<'_>,
    config: &WriteNextConfig,
    book_id: &str,
    word_count: Option<u32>,
    temperature_override: Option<f64>,
    external_context: Option<&str>,
) -> Result<ChapterPipelineResult, WriteNextError> {
    let lock = acquire_book_lock(book_id).await;
    let _guard = lock.lock().await;
    write_next_chapter_locked(
        state, agents, ctx, config, book_id, word_count, temperature_override, external_context,
    )
    .await
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn write_next_chapter_locked(
    state: &StateManager,
    agents: &WriteNextAgents<'_>,
    ctx: &WriteNextCtx<'_>,
    config: &WriteNextConfig,
    book_id: &str,
    word_count: Option<u32>,
    temperature_override: Option<f64>,
    external_context: Option<&str>,
) -> Result<ChapterPipelineResult, WriteNextError> {
    state.ensure_control_documents(book_id, None).await?;
    let book = state.load_book_config(book_id).await?;
    let book_dir = state.book_dir(book_id);
    assert_no_pending_state_repair(state, book_id).await?;
    let chapter_number = state.get_next_chapter_number(book_id).await?;

    let parsed_genre = crate::agents::rules_reader::read_genre_profile(
        ctx.project_root,
        &book.genre,
        ctx.builtin_genres_dir,
    )
    .await
    .map_err(|e| WriteNextError::Write(e.to_string()))?;
    let gp = parsed_genre.profile;
    let pipeline_language = match book.language.as_deref() {
        Some("en") => WritingLanguage::En,
        _ => WritingLanguage::Zh,
    };
    let length_spec = build_length_spec(
        word_count.unwrap_or(book.chapter_word_count),
        pipeline_language,
    );

    let write_input = prepare_write_input(
        state, agents, ctx, config, &book, &book_dir, chapter_number, external_context,
    )
    .await?;

    // ── 1. 撰写草稿 ──
    tracing::info!(target: "write-next", "撰写章节草稿");
    let writer_ctx = WriterCtx {
        project_root: ctx.project_root,
        builtin_genres_dir: ctx.builtin_genres_dir,
        prompt_store: ctx.prompt_store,
        state_store: ctx.state_store,
    };
    let output = write_chapter(
        &writer_ctx,
        agents.writer,
        &WriteChapterInput {
            book: &book,
            book_dir: &book_dir,
            chapter_number,
            external_context,
            chapter_intent: write_input.chapter_intent.as_deref(),
            chapter_memo: write_input.chapter_memo.as_ref(),
            chapter_intent_data: write_input.chapter_intent_data.as_ref(),
            context_package: write_input.context_package.as_ref(),
            rule_stack: write_input.rule_stack.as_ref(),
            length_spec: Some(length_spec.clone()),
            word_count_override: word_count,
            temperature_override,
        },
    )
    .await?;
    let writer_count = count_chapter_length(&output.content, length_spec.counting_mode);

    let mut total_usage = output.token_usage;
    let final_content: String;
    let revised: bool;
    let mut audit_result: AuditResult;
    let post_revise_count: u32;
    let normalize_applied: bool;
    let pre_audit_normalized_word_count: u32;

    if config.chapter_review_mode == ChapterReviewMode::Manual {
        // C4a：写完即停——跳过自动审核环（避免静默翻倍章节耗时）。
        tracing::info!(target: "write-next", "写完即停（手动审查模式）");
        let normalized =
            normalize_post_write_surface(&output.content, Some(pipeline_language));
        if let Err(message) = assert_chapter_content_not_empty(&normalized, "manual write") {
            return Err(WriteNextError::EmptyChapterContent { stage: message });
        }
        final_content = normalized;
        revised = false;
        post_revise_count = 0;
        normalize_applied = final_content != output.content;
        pre_audit_normalized_word_count = writer_count;
        audit_result = AuditResult {
            passed: false,
            issues: Vec::new(),
            summary: if pipeline_language == WritingLanguage::En {
                "Not reviewed yet (manual mode: stopped after writing — run review when ready).".to_string()
            } else {
                "尚未审查（手动模式：写完即停，需要时点“审查”）。".to_string()
            },
            parse_failed: None,
            overall_score: None,
            token_usage: None,
        };
        let _ = post_revise_count;
        let _ = normalize_applied;
        let _ = pre_audit_normalized_word_count;
    } else {
        // ── 2. 审核环（assess → revise → assess，快照择优） ──
        let control = match (
            &write_input.chapter_intent,
            &write_input.context_package,
            &write_input.rule_stack,
        ) {
            (Some(intent), Some(package), Some(stack)) => Some(ChapterReviewCycleControlInput {
                chapter_intent: intent,
                chapter_memo: write_input.chapter_memo.as_ref(),
                chapter_intent_data: write_input.chapter_intent_data.as_ref(),
                context_package: package,
                rule_stack: stack,
            }),
            _ => None,
        };

        // length-normalizer 端口适配（normalizeDraftLengthIfNeeded）。
        struct NormalizerAdapter<'a, 'b> {
            chat: &'a dyn LengthNormalizerChat,
            spec: &'b LengthSpec,
            chapter_intent: Option<String>,
        }
        #[async_trait]
        impl DraftLengthNormalizer for NormalizerAdapter<'_, '_> {
            async fn normalize(&self, content: &str) -> Result<NormalizeStepResult, String> {
                let out = normalize_chapter(
                    self.chat,
                    &NormalizeLengthInput {
                        chapter_content: content,
                        length_spec: self.spec,
                        chapter_intent: self.chapter_intent.as_deref(),
                        reduced_control_block: None,
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(NormalizeStepResult {
                    content: out.normalized_content,
                    word_count: out.final_count,
                    applied: out.applied,
                    token_usage: out.token_usage,
                })
            }
        }
        let normalizer = NormalizerAdapter {
            chat: agents.normalizer,
            spec: &length_spec,
            chapter_intent: write_input.chapter_intent.clone(),
        };

        // 确定性后写检查（每轮重跑）：post-write error 级 + hook 账本校验。
        let parsed_rules = read_book_rules(&book_dir).await;
        let gp_checks = gp.clone();
        let rules_checks = parsed_rules.map(|parsed| parsed.rules);
        let memo_body = write_input
            .chapter_memo
            .as_ref()
            .map(|memo| memo.body.clone())
            .unwrap_or_default();
        let run_post_write_checks = move |content: &str| -> Vec<AuditIssue> {
            let mut issues: Vec<AuditIssue> = validate_post_write(
                content,
                &gp_checks,
                rules_checks.as_ref(),
                Some(pipeline_language),
            )
            .into_iter()
            .filter(|v| v.severity == ViolationSeverity::Error)
            .map(|v: PostWriteViolation| AuditIssue {
                severity: AuditSeverity::Critical,
                category: v.rule,
                description: v.description,
                suggestion: v.suggestion,
                repair_scope: None,
            })
            .collect();
            // Phase 9-3：草稿必须兑现 memo 承诺的每个 hook。
            if !memo_body.is_empty() {
                issues.extend(
                    validate_hook_ledger(&memo_body, content)
                        .into_iter()
                        .map(|violation| AuditIssue {
                            severity: if violation.severity == crate::utils::hook_ledger_validator::ViolationSeverity::Critical {
                                AuditSeverity::Critical
                            } else {
                                AuditSeverity::Warning
                            },
                            category: violation.category,
                            description: violation.description,
                            suggestion: violation.suggestion,
                            repair_scope: None,
                        }),
                );
            }
            issues
        };

        // reviser 环内端口适配（真实版：book_dir/章节号/genre 闭包捕获）。
        struct RealCycleReviser<'a> {
            chat: &'a dyn ReviserChat,
            ctx: ReviserCtx<'a>,
            book_dir: std::path::PathBuf,
            chapter_number: u32,
            genre: String,
        }
        #[async_trait]
        impl CycleReviser for RealCycleReviser<'_> {
            async fn revise_chapter(
                &self,
                content: &str,
                issues: &[AuditIssue],
                control: Option<&ChapterReviewCycleControlInput<'_>>,
                spec: &LengthSpec,
            ) -> Result<ReviseOutput, String> {
                let options = ReviseOptions {
                    chapter_intent: control.map(|c| c.chapter_intent),
                    chapter_memo: control.and_then(|c| c.chapter_memo),
                    chapter_intent_data: control.and_then(|c| c.chapter_intent_data),
                    context_package: control.map(|c| c.context_package),
                    rule_stack: control.map(|c| c.rule_stack),
                    length_spec: Some(spec),
                };
                reviser_revise_chapter(
                    self.chat,
                    &self.ctx,
                    &self.book_dir,
                    content,
                    self.chapter_number,
                    issues,
                    ReviseMode::Auto,
                    Some(&self.genre),
                    &options,
                )
                .await
                .map_err(|e: ReviseChapterError| e.to_string())
            }
        }
        let cycle_reviser = RealCycleReviser {
            chat: agents.reviser,
            ctx: ReviserCtx {
                project_root: ctx.project_root,
                builtin_genres_dir: ctx.builtin_genres_dir,
                prompt_store: ctx.prompt_store,
            },
            book_dir: book_dir.clone(),
            chapter_number,
            genre: book.genre.clone(),
        };

        // 44 号：完整审计器主链（真实 audit_chapter 编排；缺省回退最小协议）。
        let scoped_full = agents
            .full_auditor
            .as_ref()
            .map(|a| a.for_chapter(book_dir.clone(), chapter_number, &book.genre));
        let cycle_auditor: &dyn CycleAuditor = scoped_full
            .as_ref()
            .map(|a| a as &dyn CycleAuditor)
            .unwrap_or(agents.auditor);

        let callbacks = ReviewCycleCallbacks {
            normalize_post_write_surface: Some(Arc::new(move |content| {
                normalize_post_write_surface(content, Some(pipeline_language))
            })),
            assert_chapter_content_not_empty: Arc::new(|content, stage| {
                assert_chapter_content_not_empty(content, stage)
            }),
            analyze_ai_tells: Arc::new(move |content| {
                analyze_ai_tells(content, pipeline_language)
                    .issues
                    .into_iter()
                    .map(|issue| AuditIssue {
                        severity: AuditSeverity::Warning,
                        category: issue.category,
                        description: issue.description,
                        suggestion: issue.suggestion,
                        repair_scope: None,
                    })
                    .collect()
            }),
            analyze_sensitive_words: Arc::new(move |content| {
                let result = analyze_sensitive_words(content, None, pipeline_language);
                SensitiveScanResult {
                    blocked: result
                        .found
                        .iter()
                        .any(|m| m.severity == crate::agents::sensitive_words::SensitiveWordSeverity::Block),
                    issues: result.issues,
                }
            }),
            run_post_write_checks: Some(Arc::new(run_post_write_checks)),
            log_warn: Arc::new(|zh, en| tracing::warn!(target: "write-next", "{zh} / {en}")),
            log_stage: Arc::new(|zh, en| tracing::info!(target: "write-next", "{zh} / {en}")),
        };

        let review = run_chapter_review_cycle(ReviewCycleParams {
            book_dir: &book_dir,
            chapter_number,
            initial_content: &output.content,
            initial_word_count: output.word_count,
            initial_post_write_errors: &output.post_write_errors,
            reduced_control_input: control,
            length_spec: &length_spec,
            initial_usage: crate::agents::continuity::AuditTokenUsage {
                prompt_tokens: total_usage.prompt_tokens,
                completion_tokens: total_usage.completion_tokens,
                total_tokens: total_usage.total_tokens,
            },
            reviser: &cycle_reviser,
            auditor: cycle_auditor,
            normalizer: &normalizer,
            callbacks,
            max_review_iterations: Some(config.writing_review_retries),
        })
        .await?;

        total_usage = crate::agents::writer::TokenUsage {
            prompt_tokens: review.total_usage.prompt_tokens,
            completion_tokens: review.total_usage.completion_tokens,
            total_tokens: review.total_usage.total_tokens,
        };
        final_content = review.final_content;
        revised = review.revised;
        audit_result = review.audit_result;
        post_revise_count = review.post_revise_count;
        normalize_applied = review.normalize_applied;
        pre_audit_normalized_word_count = review.pre_audit_normalized_word_count;
        let _ = (post_revise_count, normalize_applied, pre_audit_normalized_word_count);
    }

    // ── 3b. 轻量晋级（落盘前，零 LLM） ──
    run_promotion_pass(&book_dir, chapter_number).await;

    // ── 4. 持久化产物装配 + 标题去重双轮 ──
    tracing::info!(target: "write-next", "落盘最终章节");
    let chapter_index_before = state.load_chapter_index(book_id).await?;
    let existing_titles: Vec<String> = chapter_index_before
        .iter()
        .map(|meta| meta.title.clone())
        .collect();
    let initial_resolution = resolve_duplicate_title(
        &output.title,
        &existing_titles,
        pipeline_language,
        Some(&final_content),
    );
    let mut persistence_output = build_persistence_output(
        agents.analyzer,
        &ChapterAnalyzerCtx {
            project_root: ctx.project_root,
            builtin_genres_dir: ctx.builtin_genres_dir,
        },
        &BuildPersistenceOutputParams {
            book: &book,
            book_dir: &book_dir,
            chapter_number,
            output: &WriteChapterOutput {
                title: initial_resolution.title.clone(),
                ..output.clone()
            },
            final_content: &final_content,
            counting_mode: length_spec.counting_mode,
            context_package: write_input.context_package.as_ref(),
            rule_stack: write_input.rule_stack.as_ref(),
            chapter_intent: write_input.chapter_intent.as_deref(),
        },
    )
    .await
    .map_err(|e| WriteNextError::Analyzer(e.to_string()))?;

    let final_title_resolution = resolve_duplicate_title(
        &persistence_output.title,
        &existing_titles,
        pipeline_language,
        Some(&final_content),
    );
    if final_title_resolution.title != persistence_output.title {
        if final_title_resolution.title != output.title {
            let description = if pipeline_language == WritingLanguage::En {
                format!(
                    "Chapter title \"{}\" was auto-adjusted to \"{}\".",
                    output.title, final_title_resolution.title
                )
            } else {
                format!(
                    "章节标题“{}”已自动调整为“{}”。",
                    output.title, final_title_resolution.title
                )
            };
            tracing::warn!(target: "write-next", "[title] {description}");
            audit_result.issues.push(AuditIssue {
                severity: AuditSeverity::Warning,
                category: "title-dedup".to_string(),
                description,
                suggestion: if pipeline_language == WritingLanguage::En {
                    "If the auto-renamed title is weak, revise the chapter title manually.".to_string()
                } else {
                    "如果自动改名不理想，可以在后续手动修订章节标题。".to_string()
                },
                repair_scope: None,
            });
        }
        persistence_output.title = final_title_resolution.title;
    }

    // ── 长跨度疲劳 + hook 健康并入审计 ──
    let fatigue_issues = analyze_long_span_fatigue(
        &crate::utils::long_span_fatigue::AnalyzeLongSpanFatigueInput {
            book_dir: &book_dir,
            chapter_number,
            chapter_content: &final_content,
            chapter_summary: Some(&persistence_output.chapter_summary),
            language: pipeline_language,
        },
    )
    .await;
    for issue in fatigue_issues {
        audit_result.issues.push(AuditIssue::from(issue));
    }
    audit_result
        .issues
        .extend(output.hook_health_issues.iter().map(|issue| AuditIssue {
            severity: issue.severity,
            category: issue.category.clone(),
            description: issue.description.clone(),
            suggestion: issue.suggestion.clone(),
            repair_scope: None,
        }));
    // TS 同款：落盘产物的字数为最终值（环内计数仅为中间量）。
    let final_word_count = persistence_output.word_count;

    let length_warnings = build_length_warnings(chapter_number, final_word_count, &length_spec);
    let length_telemetry = Some(crate::models::length_governance::LengthTelemetry {
        target: length_spec.target,
        soft_min: length_spec.soft_min,
        soft_max: length_spec.soft_max,
        hard_min: length_spec.hard_min,
        hard_max: length_spec.hard_max,
        counting_mode: length_spec.counting_mode,
        writer_count,
        post_writer_normalize_count: pre_audit_normalized_word_count,
        post_revise_count,
        final_count: final_word_count,
        normalize_applied,
        length_warning: !length_warnings.is_empty(),
    });
    for warning in &length_warnings {
        tracing::warn!(target: "write-next", "{warning}");
    }

    // ── 4.1 真相校验（validator + 重试结算） ──
    tracing::info!(target: "write-next", "校验真相文件变更");
    let story_dir = book_dir.join("story");
    let authority = StateValidationAuthorityContext {
        story_frame: tokio::fs::read_to_string(story_dir.join("outline").join("story_frame.md"))
            .await
            .ok()
            .or({
                // readStoryFrame 的 legacy 回退已由 outline-paths 承担；
                // 此处直接双路径读。
                None
            }),
        book_rules: tokio::fs::read_to_string(story_dir.join("book_rules.md"))
            .await
            .ok(),
        chapter_summaries: tokio::fs::read_to_string(story_dir.join("chapter_summaries.md"))
            .await
            .ok(),
    };
    let old_state = tokio::fs::read_to_string(story_dir.join("current_state.md"))
        .await
        .unwrap_or_default();
    let old_hooks = tokio::fs::read_to_string(story_dir.join("pending_hooks.md"))
        .await
        .unwrap_or_default();
    let old_ledger = tokio::fs::read_to_string(story_dir.join("particle_ledger.md"))
        .await
        .unwrap_or_default();

    // validator 端口适配（StateValidatorChat → ValidatePort）。
    struct ValidatorAdapter<'a> {
        chat: &'a dyn StateValidatorChat,
    }
    #[async_trait]
    impl ValidatePort for ValidatorAdapter<'_> {
        async fn validate(
            &self,
            params: crate::pipeline::chapter_state_recovery::ValidateRequest<'_>,
        ) -> Result<crate::agents::state_validator::ValidationResult, String> {
            crate::agents::state_validator::validate(
                self.chat,
                &crate::agents::state_validator::ValidateParams {
                    chapter_content: params.content,
                    chapter_number: params.chapter_number,
                    old_state: params.old_state,
                    new_state: params.new_state,
                    old_hooks: params.old_hooks,
                    new_hooks: params.new_hooks,
                    language: params.language,
                    authority_context: params.authority_context,
                },
            )
            .await
            .map_err(|e| e.to_string())
        }
    }
    let validator = ValidatorAdapter { chat: agents.state_validator };

    let truth = validate_chapter_truth_persistence(TruthValidationParams {
        writer: agents.settler,
        validator: &validator,
        book: &book,
        book_dir: &book_dir,
        chapter_number,
        title: &persistence_output.title,
        content: &final_content,
        persistence_output: persistence_output.clone(),
        audit_result: audit_result.clone(),
        old_state: &old_state,
        old_hooks: &old_hooks,
        old_ledger: &old_ledger,
        authority_context: Some(&authority),
        control: match (
            &write_input.chapter_intent,
            &write_input.context_package,
            &write_input.rule_stack,
        ) {
            (Some(intent), Some(package), Some(stack)) => Some(ControlInput {
                chapter_intent: intent,
                context_package: package,
                rule_stack: stack,
            }),
            _ => None,
        },
        language: pipeline_language,
        log_warn: &|zh, en| tracing::warn!(target: "write-next", "{zh} / {en}"),
    })
    .await
    .map_err(WriteNextError::TruthValidation)?;

    let chapter_status: &'static str = truth
        .chapter_status
        .unwrap_or(if truth.audit_result.passed { "ready-for-review" } else { "audit-failed" });
    persistence_output = truth.persistence_output;
    audit_result = truth.audit_result;
    let degraded_issues = truth.degraded_issues;

    // ── 4.2 段落形态检查（落盘后正文） ──
    let chap_dir = book_dir.join("chapters");
    let mut recent_files: Vec<String> = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&chap_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".md") && padded_re().is_match(&name) {
                recent_files.push(name);
            }
        }
    }
    recent_files.sort();
    let start = recent_files.len().saturating_sub(5);
    let mut recent_content = String::new();
    for name in &recent_files[start..] {
        if let Ok(content) = tokio::fs::read_to_string(chap_dir.join(name)).await {
            recent_content.push_str(&content);
            recent_content.push_str("\n\n");
        }
    }
    let paragraph_issues: Vec<AuditIssue> = detect_paragraph_shape_warnings_pub(
        &final_content,
        pipeline_language,
    )
    .into_iter()
    .chain(detect_paragraph_length_drift(&final_content, &recent_content, pipeline_language))
    .map(|violation: PostWriteViolation| {
        tracing::warn!(target: "write-next", "[paragraph] {}", violation.description);
        AuditIssue {
            severity: AuditSeverity::Warning,
            category: "paragraph-shape".to_string(),
            description: violation.description,
            suggestion: violation.suggestion,
            repair_scope: None,
        }
    })
    .collect();
    audit_result.issues.extend(paragraph_issues);

    // ── 5. 落盘 ──
    let status_enum = match chapter_status {
        "state-degraded" => crate::models::chapter::ChapterStatus::StateDegraded,
        "audit-failed" => crate::models::chapter::ChapterStatus::AuditFailed,
        _ => crate::models::chapter::ChapterStatus::ReadyForReview,
    };
    let mut persistence_output_for_save = persistence_output.clone();
    persistence_output_for_save.word_count = final_word_count;

    let hooks = RunnerPersistenceHooks {
        state,
        book_id,
        writer_ctx: &WriterCtx {
            project_root: ctx.project_root,
            builtin_genres_dir: ctx.builtin_genres_dir,
            prompt_store: ctx.prompt_store,
            state_store: ctx.state_store,
        },
        book: &book,
        book_dir: &book_dir,
        output: &persistence_output_for_save,
        numerical_system: gp.numerical_system,
        language: pipeline_language,
    };
    persist_chapter_artifacts(
        &hooks,
        &PersistChapterArtifactsParams {
            chapter_number,
            chapter_title: &persistence_output.title,
            status: status_enum,
            audit_passed: audit_result.passed,
            audit_issues: &audit_result.issues,
            final_word_count,
            length_warnings: &length_warnings,
            length_telemetry: length_telemetry.clone(),
            degraded_issues: &degraded_issues,
            token_usage: Some(crate::agents::continuity::AuditTokenUsage {
                prompt_tokens: total_usage.prompt_tokens,
                completion_tokens: total_usage.completion_tokens,
                total_tokens: total_usage.total_tokens,
            }),
            log_snapshot_stage: &|| {
                tracing::info!(target: "write-next", "更新章节索引与快照");
            },
            now: None,
        },
    )
    .await
    .map_err(|e| WriteNextError::Persistence(e.to_string()))?;

    let result = ChapterPipelineResult {
        chapter_number,
        title: persistence_output.title.clone(),
        word_count: final_word_count,
        audit_result,
        revised,
        status: chapter_status,
        length_warnings,
        length_telemetry,
        token_usage: total_usage,
    };

    // ── 6. 通知（失败不阻断） ──
    if let Some(notify) = &ctx.notify {
        notify(&result, &book);
    }

    Ok(result)
}

/// runner 落盘钩子（saveChapter / saveTruthFiles / 索引 / 快照 / 漂移）。
struct RunnerPersistenceHooks<'a> {
    state: &'a StateManager,
    book_id: &'a str,
    writer_ctx: &'a WriterCtx<'a>,
    book: &'a BookConfig,
    book_dir: &'a Path,
    output: &'a WriteChapterOutput,
    numerical_system: bool,
    language: WritingLanguage,
}

#[async_trait]
impl PersistenceHooks for RunnerPersistenceHooks<'_> {
    async fn load_chapter_index(&self) -> Result<Vec<crate::models::chapter::ChapterMeta>, String> {
        self.state.load_chapter_index(self.book_id).await.map_err(|e| e.to_string())
    }
    async fn save_chapter(&self) -> Result<(), String> {
        save_chapter(self.writer_ctx, self.book_dir, self.output, self.numerical_system, self.language)
            .await
            .map_err(|e| e.to_string())
    }
    async fn save_truth_files(&self) -> Result<(), String> {
        save_new_truth_files(self.book_dir, self.output, self.language)
            .await
            .map_err(|e| e.to_string())?;
        // syncLegacyStructuredStateFromMarkdown：无结构化 delta 时重写 state。
        if self.output.runtime_state_delta.is_none() && self.output.runtime_state_snapshot.is_none() {
            let _ = crate::state::state_bootstrap::bootstrap_structured_state_from_markdown(
                self.writer_ctx.state_store,
                &self.book_dir.to_string_lossy(),
                Some(self.output.chapter_number),
            )
            .await;
        }
        Ok(())
    }
    async fn save_chapter_index(
        &self,
        index: &[crate::models::chapter::ChapterMeta],
    ) -> Result<(), String> {
        self.state
            .save_chapter_index(self.book_id, index)
            .await
            .map_err(|e| e.to_string())
    }
    async fn mark_book_active_if_needed(&self) -> Result<(), String> {
        let mut book = self.book.clone();
        if book.status == crate::models::book::BookStatus::Outlining {
            book.status = crate::models::book::BookStatus::Active;
            book.updated_at = crate::utils::utc_time::utc_now_iso();
            self.state
                .save_book_config(self.book_id, &book)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    async fn persist_audit_drift_guidance(&self, issues: &[AuditIssue]) -> Result<(), String> {
        persist_audit_drift_guidance(self.book_dir, self.output.chapter_number, issues, self.language)
            .await
            .map_err(|e| e.to_string())
    }
    async fn snapshot_state(&self) -> Result<(), String> {
        self.state
            .snapshot_state(self.book_id, self.output.chapter_number)
            .await
            .map_err(|e| e.to_string())
    }
    async fn sync_current_state_fact_history(&self) -> Result<(), String> {
        // TS rebuildCurrentStateFactHistory（SQLite 记忆索引重建）——markdown
        // 回退面下为空操作，SQLite 面随记忆索引端点移植（备案）。
        Ok(())
    }
}

/// 审计漂移指引（audit_drift.md + current_state 剥旧块）。
async fn persist_audit_drift_guidance(
    book_dir: &Path,
    chapter_number: u32,
    issues: &[AuditIssue],
    language: WritingLanguage,
) -> std::io::Result<()> {
    let story_dir = book_dir.join("story");
    let drift_path = story_dir.join("audit_drift.md");
    let state_path = story_dir.join("current_state.md");
    let current_state = tokio::fs::read_to_string(&state_path).await.unwrap_or_default();
    let sanitized_state = strip_audit_drift_correction_block(&current_state)
        .trim_end()
        .to_string();
    if sanitized_state != current_state {
        tokio::fs::write(&state_path, sanitized_state).await?;
    }

    if issues.is_empty() {
        let _ = tokio::fs::remove_file(&drift_path).await;
        return Ok(());
    }

    let header = if language == WritingLanguage::En { "# Audit Drift" } else { "# 审计纠偏" };
    let section = if language == WritingLanguage::En {
        "## Audit Drift Correction"
    } else {
        "## 审计纠偏（自动生成，下一章写作前参照）"
    };
    let intro = if language == WritingLanguage::En {
        format!("> Chapter {chapter_number} audit found the following issues to avoid in the next chapter:")
    } else {
        format!("> 第{chapter_number}章审计发现以下问题，下一章写作时必须避免：")
    };
    let mut lines = vec![header.to_string(), String::new(), section.to_string(), String::new(), intro];
    lines.extend(
        issues
            .iter()
            .map(|issue| format!("> - [{}] {}: {}", severity_text(issue.severity), issue.category, issue.description)),
    );
    lines.push(String::new());
    tokio::fs::write(&drift_path, lines.join("\n")).await
}

fn severity_text(severity: AuditSeverity) -> &'static str {
    match severity {
        AuditSeverity::Critical => "critical",
        AuditSeverity::Warning => "warning",
        AuditSeverity::Info => "info",
    }
}

fn strip_audit_drift_correction_block(current_state: &str) -> String {
    const HEADERS: [&str; 4] = [
        "## 审计纠偏（自动生成，下一章写作前参照）",
        "## Audit Drift Correction",
        "# 审计纠偏",
        "# Audit Drift",
    ];
    let mut cut_index: Option<usize> = None;
    for header in HEADERS {
        if let Some(index) = current_state.find(header) {
            cut_index = Some(match cut_index {
                Some(current) => current.min(index),
                None => index,
            });
        }
    }
    match cut_index {
        None => current_state.to_string(),
        Some(index) => current_state[..index].trim_end().to_string(),
    }
}

/// 段落形态（post-write-validator 私有 → 公开适配）。
fn detect_paragraph_shape_warnings_pub(
    content: &str,
    language: WritingLanguage,
) -> Vec<PostWriteViolation> {
    crate::agents::post_write_validator::detect_paragraph_shape_warnings(content, language)
}

/// 硬区间外长度警告。
fn build_length_warnings(chapter_number: u32, final_count: u32, spec: &LengthSpec) -> Vec<String> {
    if !is_outside_hard_range(final_count, spec.hard_min, spec.hard_max) {
        return Vec::new();
    }
    let zh = format!(
        "第{chapter_number}章经过一次字数归一化后仍超出硬区间（{}-{}，实际 {final_count}）。",
        spec.hard_min, spec.hard_max
    );
    let en = format!(
        "Chapter {chapter_number} remains outside hard range ({}-{}, actual {final_count}) after a single normalization pass.",
        spec.hard_min, spec.hard_max
    );
    vec![match spec.counting_mode {
        LengthCountingMode::ZhChars => zh,
        LengthCountingMode::EnWords => en,
    }]
}

fn assert_chapter_content_not_empty(content: &str, stage: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err(format!("chapter content is empty after `{stage}`"));
    }
    Ok(())
}

async fn assert_no_pending_state_repair(state: &StateManager, book_id: &str) -> Result<(), WriteNextError> {
    let index = state.load_chapter_index(book_id).await?;
    let latest = index.iter().max_by_key(|meta| meta.number);
    if let Some(meta) = latest {
        if meta.status == crate::models::chapter::ChapterStatus::StateDegraded {
            return Err(WriteNextError::PendingStateRepair(meta.number));
        }
    }
    Ok(())
}

/// 轻量晋级落盘（hooks 表 + 摘要推导 → promoted 翻转 → 重渲染）。
async fn run_promotion_pass(book_dir: &Path, chapter_number: u32) {
    let story_dir = book_dir.join("story");
    let ledger_path = story_dir.join("pending_hooks.md");
    let Ok(ledger_raw) = tokio::fs::read_to_string(&ledger_path).await else {
        return;
    };
    if ledger_raw.trim().is_empty() {
        return;
    }
    let hooks = parse_pending_hooks_markdown(&ledger_raw);
    if hooks.is_empty() {
        return;
    }
    let summaries_raw = tokio::fs::read_to_string(story_dir.join("chapter_summaries.md"))
        .await
        .unwrap_or_default();
    let promotion = rerun_promotion_pass(&hooks, &summaries_raw);
    if promotion.updated {
        let contains_cjk = ledger_raw.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
        let ledger_language = if contains_cjk { WritingLanguage::Zh } else { WritingLanguage::En };
        let rendered = render_hook_snapshot(&promotion.hooks, ledger_language);
        if tokio::fs::write(&ledger_path, rendered).await.is_ok() {
            tracing::info!(
                "[promotion] {} hook(s) promoted after chapter {chapter_number}",
                promotion.flipped_count
            );
        }
    }
}

/// 输入准备：v2 治理（plan 持久化复用 + composer）/ legacy。
#[allow(clippy::too_many_arguments)]
async fn prepare_write_input(
    _state: &StateManager,
    agents: &WriteNextAgents<'_>,
    ctx: &WriteNextCtx<'_>,
    config: &WriteNextConfig,
    book: &BookConfig,
    book_dir: &Path,
    chapter_number: u32,
    external_context: Option<&str>,
) -> Result<PreparedWriteInput, WriteNextError> {
    if config.input_governance_mode == InputGovernanceMode::Legacy {
        return Ok(PreparedWriteInput {
            chapter_intent: None,
            chapter_memo: None,
            chapter_intent_data: None,
            context_package: None,
            rule_stack: None,
        });
    }

    // resolveGovernedPlan：无新上下文时复用持久化 plan（跳过 planner LLM）。
    let plan = match load_persisted_plan(book_dir, chapter_number).await {
        Some(plan) if external_context.map(str::trim).unwrap_or("").is_empty() => plan,
        _ => {
            let plan = plan_chapter(
                agents.planner,
                &PlanChapterInput {
                    book_language: book.language.as_deref().unwrap_or("zh"),
                    book_dir,
                    chapter_number,
                    external_context,
                },
            )
            .await?;
            save_persisted_plan(book_dir, &plan).await?;
            plan
        }
    };

    // composeGovernedChapter。
    let selector = LlmOutlineSelector { chat: agents.composer };
    let compiler = LlmContextCompiler { chat: agents.composer };
    let composed: ComposeChapterOutput = compose_governed_chapter(&ComposeChapterInput {
        book_language: book.language.as_deref(),
        book_dir,
        chapter_number,
        plan: &plan,
        context_budget: ctx.context_budget,
        compiler: Some(&compiler),
        outline_section_selector: Some(&selector),
        on_context_compression: None,
    })
    .await?;

    Ok(PreparedWriteInput {
        chapter_intent: Some(plan.intent_markdown.clone()),
        chapter_memo: Some(plan.memo.clone()),
        chapter_intent_data: Some(plan.intent.clone()),
        context_package: Some(composed.context_package),
        rule_stack: Some(composed.rule_stack),
    })
}

fn padded_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static R: OnceLock<regex::Regex> = OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^\d{4}").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::composer::ComposerChatOptions;
    use crate::agents::continuity::AuditTokenUsage;
    use crate::agents::writer::WriterChat;
    use crate::agents::continuity::ChatOutcome;
    use crate::llm::provider::LLMMessage;
    use crate::state::store::InMemoryStateStore;
    use std::sync::Mutex as StdMutex;

    // ---- 全链 mock：按 agent 角色回放脚本响应 ----

    struct ScriptChat {
        responses: Vec<(String, String)>, // (标记, 内容)——按 system prompt 首行匹配分发
        calls: StdMutex<Vec<String>>,
    }

    impl ScriptChat {
        fn dispatch(&self, messages: &[LLMMessage]) -> String {
            let system_head = messages
                .first()
                .map(|m| m.content.lines().next().unwrap_or("").to_string())
                .unwrap_or_default();
            for (marker, content) in &self.responses {
                if system_head.contains(marker.as_str()) {
                    self.calls.lock().unwrap().push(marker.clone());
                    return content.clone();
                }
            }
            // 默认：最后一个响应。
            self.responses
                .last()
                .map(|(_, content)| content.clone())
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl WriterChat for ScriptChat {
        async fn chat(
            &self,
            messages: Vec<LLMMessage>,
            _temperature: f64,
        ) -> Result<ChatOutcome, String> {
            Ok(ChatOutcome {
                content: self.dispatch(&messages),
                usage: Some(AuditTokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
            })
        }
    }

    #[async_trait]
    impl ComposerChat for ScriptChat {
        async fn chat(
            &self,
            messages: Vec<LLMMessage>,
            _options: ComposerChatOptions,
        ) -> Result<ChatOutcome, String> {
            Ok(ChatOutcome {
                content: self.dispatch(&messages),
                usage: None,
            })
        }
    }

    #[async_trait]
    impl CycleAuditor for ScriptChat {
        async fn audit_chapter(
            &self,
            _content: &str,
            _control: Option<&ChapterReviewCycleControlInput<'_>>,
            _temperature: Option<f64>,
        ) -> Result<AuditResult, String> {
            Ok(AuditResult {
                passed: true,
                issues: Vec::new(),
                summary: "审计通过".into(),
                parse_failed: None,
                overall_score: Some(92),
                token_usage: None,
            })
        }
    }

    #[async_trait]
    impl SettlePort for ScriptChat {
        async fn settle(
            &self,
            _params: crate::pipeline::chapter_state_recovery::SettleRequest<'_>,
        ) -> Result<WriteChapterOutput, String> {
            // truth-validation 重试路径（首验通过时不会被调用）。
            Err("settle 不应被调用".to_string())
        }
    }

    const PLANNER_RESPONSE: &str = "# 第 1 章 memo\n\n## 本章目标\n主角初次交锋\n\n## 关联线索\n- H01\n\n## 当前任务\n林动在坊市与人对峙，夺回被夺的玉符。\n\n## 读者此刻在等什么\n期待玉符来历揭开。\n本章部分兑现。\n\n## 该兑现的 / 暂不掀的\n- 该兑现：玉符第一步。\n\n## 日常/过渡承担什么任务\n不适用 - 本章无日常过渡。\n\n## 关键抉择过三连问\n- 主角：为什么？利益？人设？\n\n## 章尾必须发生的改变\n信息改变：玉符一角真相。\n\n## 本章 hook 账\nadvance:\n- H01 \"祖符\" → 推进（planted → pressured）\n\n## 不要做\n- 不要降智。\n\n";

    const WRITER_RESPONSE: &str = "=== CHAPTER_TITLE ===\n风起\n\n=== CHAPTER_CONTENT ===\n林动睁开双眼，灵气顺着经脉游走。他握紧拳头，多年屈辱自今日起一笔一笔讨回来。远处钟声响起，少年迈步而出。\n\n=== POST_SETTLEMENT ===\n结算完成。\n\n=== RUNTIME_STATE_DELTA ===\n```json\n{\"chapter\": 1, \"chapterSummary\": {\"chapter\": 1, \"title\": \"风起\", \"characters\": \"林动\", \"events\": \"醒来\", \"stateChanges\": \"无\", \"hookActivity\": \"H01 推进\", \"mood\": \"紧张\", \"chapterType\": \"推进章\"}}\n```\n";

    const SETTLER_DELTA_RESPONSE: &str = WRITER_RESPONSE;

    fn script() -> ScriptChat {
        ScriptChat {
            responses: vec![
                ("你是这本小说的创作总编".into(), PLANNER_RESPONSE.into()),
                ("网络小说作家".into(), WRITER_RESPONSE.into()),
                ("网文状态结算官".into(), SETTLER_DELTA_RESPONSE.into()),
                ("修稿编辑".into(), String::new()),
                ("章节长度修正器".into(), String::new()),
                ("连续性分析师".into(), String::new()),
            ],
            calls: StdMutex::new(Vec::new()),
        }
    }

    #[async_trait]
    impl PlannerChat for ScriptChat {
        async fn chat(&self, messages: Vec<LLMMessage>, _temperature: f64) -> Result<ChatOutcome, String> {
            Ok(ChatOutcome { content: self.dispatch(&messages), usage: Some(AuditTokenUsage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }) })
        }
    }

    #[async_trait]
    impl ReviserChat for ScriptChat {
        async fn chat(&self, messages: Vec<LLMMessage>, _temperature: f64) -> Result<ChatOutcome, String> {
            Ok(ChatOutcome { content: self.dispatch(&messages), usage: Some(AuditTokenUsage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }) })
        }
    }

    #[async_trait]
    impl LengthNormalizerChat for ScriptChat {
        async fn chat(&self, messages: Vec<LLMMessage>, _temperature: f64) -> Result<ChatOutcome, String> {
            Ok(ChatOutcome { content: self.dispatch(&messages), usage: None })
        }
    }

    #[async_trait]
    impl ChapterAnalyzerChat for ScriptChat {
        async fn chat(&self, messages: Vec<LLMMessage>, _temperature: f64) -> Result<ChatOutcome, String> {
            Ok(ChatOutcome { content: self.dispatch(&messages), usage: None })
        }
    }

    #[async_trait]
    impl StateValidatorChat for ScriptChat {
        async fn chat(&self, messages: Vec<LLMMessage>, _temperature: f64) -> Result<ChatOutcome, String> {
            // 首行协议：直接 PASS（零警告）。
            let _ = &messages;
            Ok(ChatOutcome { content: "PASS".to_string(), usage: None })
        }
    }

    #[tokio::test]
    async fn manual_mode_writes_draft_and_persists_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let builtin = dir.path().join("builtin");
        let books = project.join("books");
        let book = books.join("b1");
        tokio::fs::create_dir_all(book.join("story").join("runtime")).await.unwrap();
        tokio::fs::create_dir_all(book.join("chapters")).await.unwrap();
        tokio::fs::create_dir_all(project.join("genres")).await.unwrap();
        tokio::fs::create_dir_all(&builtin).await.unwrap();
        tokio::fs::write(
            builtin.join("xianxia.md"),
            "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\",\"高潮章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n",
        )
        .await
        .unwrap();
        tokio::fs::write(
            book.join("book.json"),
            serde_json::json!({
                "id": "b1", "title": "测试书", "platform": "other", "genre": "xianxia",
                "status": "active", "targetChapters": 100, "chapterWordCount": 3000,
                "language": "zh", "createdAt": "", "updatedAt": "",
            })
            .to_string(),
        )
        .await
        .unwrap();

        let state = StateManager::new(&project);
        let chat = script();
        // planner/composer 共用脚本；writer/settler 同一脚本分发。
        let agents = WriteNextAgents {
            writer: &chat,
            planner: &chat,
            composer: &chat,
            reviser: &chat,
            auditor: &chat,
            full_auditor: None,
            normalizer: &chat,
            analyzer: &chat,
            state_validator: &chat,
            settler: &chat,
        };
        let prompt_store = InMemoryStateStore::default();
        let ctx = WriteNextCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &prompt_store,
            state_store: &prompt_store,
            context_budget: None,
            notify: None,
        };
        let config = WriteNextConfig {
            chapter_review_mode: ChapterReviewMode::Manual,
            writing_review_retries: 1,
            input_governance_mode: InputGovernanceMode::V2,
        };

        let result = write_next_chapter(
            &state, &agents, &ctx, &config, "b1", None, None, None,
        )
        .await
        .expect("write-next 应成功");

        assert_eq!(result.chapter_number, 1);
        assert_eq!(result.title, "风起");
        assert!(!result.revised);
        // manual 模式 passed=false（尚未审查）→ 解析状态 audit-failed（TS 同语义：
        // chapterStatus ?? (passed ? ready-for-review : audit-failed)）。
        assert_eq!(result.status, "audit-failed");
        assert!(result.audit_result.summary.contains("尚未审查"));

        // 章节落盘 + 索引。
        let index = state.load_chapter_index("b1").await.unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].title, "风起");
        assert_eq!(index[0].status, crate::models::chapter::ChapterStatus::AuditFailed);
        assert!(book.join("chapters").join("0001_风起.md").exists()
            || std::fs::read_dir(book.join("chapters")).unwrap().any(|e| {
                e.unwrap().file_name().to_string_lossy().starts_with("0001")
            }));

        // plan 持久化 + intent 工件。
        assert!(book
            .join("story")
            .join("runtime")
            .join("chapter-0001.plan.md")
            .exists());
        assert!(book
            .join("story")
            .join("runtime")
            .join("chapter-0001.intent.md")
            .exists());

        // 真相面（settler delta 落盘：state/*.json 或 markdown 投影）。
        let summaries_md = book.join("story").join("chapter_summaries.md").exists();
        let state_json = book.join("story").join("state").join("chapter_summaries.json").exists();
        assert!(summaries_md || state_json, "章节摘要真相面应落盘（markdown 或结构化）");

        // 二次 write-next：降级守卫不触发；下一章号推进（durable 链）。
        let next = state.get_next_chapter_number("b1").await.unwrap();
        assert!(next >= 2, "durable 链应推进到 2，实际 {next}");
    }

    #[tokio::test]
    async fn degraded_latest_chapter_blocks_write_next() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let state = StateManager::new(project);
        let book = state.book_dir("b1");
        tokio::fs::create_dir_all(book.join("chapters")).await.unwrap();
        // ensureControlDocuments 需要 book.json（语言判定）。
        tokio::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"t","platform":"other","genre":"xianxia","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .await
        .unwrap();
        let degraded = crate::models::chapter::ChapterMeta {
            number: 3,
            title: "降级章".into(),
            status: crate::models::chapter::ChapterStatus::StateDegraded,
            word_count: 100,
            created_at: "t".into(),
            updated_at: "t".into(),
            audit_issues: vec![],
            length_warnings: vec![],
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        };
        state
            .save_chapter_index("b1", &[degraded])
            .await
            .unwrap();

        let chat = script();
        let agents = WriteNextAgents {
            writer: &chat,
            planner: &chat,
            composer: &chat,
            reviser: &chat,
            auditor: &chat,
            full_auditor: None,
            normalizer: &chat,
            analyzer: &chat,
            state_validator: &chat,
            settler: &chat,
        };
        let store = InMemoryStateStore::default();
        let ctx = WriteNextCtx {
            project_root: project,
            builtin_genres_dir: project,
            prompt_store: &store,
            state_store: &store,
            context_budget: None,
            notify: None,
        };
        let error = write_next_chapter(
            &state,
            &agents,
            &ctx,
            &WriteNextConfig::default(),
            "b1",
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, WriteNextError::PendingStateRepair(3)));
    }

    #[test]
    fn drift_guidance_roundtrip_and_strip() {
        let current = "# Current State\n\n| a | b |\n\n## 审计纠偏（自动生成，下一章写作前参照）\n\n> 旧内容\n";
        // strip 后 trimEnd（持久化侧再补需另加）。
        assert_eq!(
            strip_audit_drift_correction_block(current),
            "# Current State\n\n| a | b |"
        );
        let clean = "# Current State\n\n正文";
        assert_eq!(strip_audit_drift_correction_block(clean), clean);
    }

    #[tokio::test]
    async fn drift_guidance_persists_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let story = dir.path().join("story");
        tokio::fs::create_dir_all(&story).await.unwrap();
        let issues = vec![AuditIssue {
            severity: AuditSeverity::Warning,
            category: "节奏".into(),
            description: "节奏拖沓".into(),
            suggestion: String::new(),
            repair_scope: None,
        }];
        persist_audit_drift_guidance(dir.path(), 3, &issues, WritingLanguage::Zh)
            .await
            .unwrap();
        let drift = tokio::fs::read_to_string(story.join("audit_drift.md"))
            .await
            .unwrap();
        assert!(drift.starts_with("# 审计纠偏"));
        assert!(drift.contains("> 第3章审计发现以下问题"));
        assert!(drift.contains("> - [warning] 节奏: 节奏拖沓"));

        // 空问题清单 → 删除文件。
        persist_audit_drift_guidance(dir.path(), 3, &[], WritingLanguage::Zh)
            .await
            .unwrap();
        assert!(!story.join("audit_drift.md").exists());
    }

    #[test]
    fn length_warnings_hard_range_only() {
        let spec = build_length_spec(3000, WritingLanguage::Zh);
        // 硬区间 2400-3600：区间内无警告；区间外（5000/100）告警。
        assert!(build_length_warnings(3, 3000, &spec).is_empty());
        for outside in [5000u32, 100] {
            let warnings = build_length_warnings(3, outside, &spec);
            assert_eq!(warnings.len(), 1, "count={outside}");
        }
        assert!(build_length_warnings(3, 100, &spec)[0].contains("第3章"));
    }
}

