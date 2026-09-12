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
use crate::agents::planner::{plan_chapter, PlanChapterError, PlanChapterInput, PlannerChat};
use crate::agents::post_write_validator::{
    detect_paragraph_length_drift, normalize_post_write_surface, resolve_duplicate_title,
    validate_post_write, PostWriteViolation, ViolationSeverity,
};
use crate::agents::reviser::{revise_chapter as reviser_revise_chapter, ReviseChapterError, ReviseMode, ReviseOptions, ReviseOutput, ReviserChat, ReviserCtx};
use crate::agents::rules_reader::read_book_rules;
use crate::agents::sensitive_words::analyze_sensitive_words;
use crate::agents::state_validator::{StateValidationAuthorityContext, StateValidatorChat};
use crate::interaction::agent_loop::AbortHandle;
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
use crate::pipeline::chapter_review_cycle::{run_chapter_review_cycle, ChapterReviewCycleControlInput, CycleAuditor, CycleReviser, ReviewCycleCallbacks, ReviewCycleError, ReviewCycleParams, SensitiveScanResult, };
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
    /// 链内中止信号（Some 时在四个安全点轮询——TS `throwIfOperationAborted`
    /// 等价物：章首 / 草稿后 / 审查环后 / 落盘前；None 全链不可截断）。
    pub abort: Option<AbortHandle>,
    /// 通知通道（111 号：章完 dispatchNotification + pipeline-complete
    /// webhook；None/空跳过——TS config.notifyChannels）。
    pub notify_channels: Option<Vec<crate::notify::NotifyChannel>>,
    /// 上下文压缩事件回调（126 号：TS PipelineConfig.onContextCompression →
    /// SSE `context:compression`；None 跳过广播——lib 内部调用/测试）。
    pub on_context_compression: Option<crate::agents::composer::CompressionCallback>,
    /// 流水线阶段日志回调（127 号：TS PipelineConfig.logger.info → SSE
    /// `log`——{level:"info", tag:"studio", message}；None 跳过。TS 面为
    /// logStage/logInfo 双语阶段叙事，本侧同构主链阶段边界）。
    pub on_log: Option<PipelineLogFn>,
}

/// 阶段日志回调：(level, message)——tag 恒 "studio"（TS createLogger 同款）。
pub type PipelineLogFn = std::sync::Arc<dyn Fn(&str, &str) + Send + Sync>;

impl Default for WriteNextConfig {
    fn default() -> Self {
        WriteNextConfig {
            chapter_review_mode: ChapterReviewMode::Auto,
            writing_review_retries: 1,
            input_governance_mode: InputGovernanceMode::V2,
            abort: None,
            notify_channels: None,
            on_context_compression: None,
            on_log: None,
        }
    }
}

impl WriteNextConfig {
    /// 项目配置装配（111 号）：default + inkos.json `notify` 数组（非法整组
    /// 忽略——保守侧；空数组按 None 处理）。
    pub async fn from_project(root: &Path) -> Self {
        let config = crate::server::project_config_routes::load_raw_config(root).await;
        let notify_channels = config
            .as_ref()
            .map(|config| crate::notify::parse_notify_channels(config.get("notify")))
            .filter(|channels| !channels.is_empty());
        let writing = config.as_ref().and_then(|config| config.get("writing"));
        // writing.reviewRetries（TS WritingConfigSchema：0-10，缺省 1）。
        let writing_review_retries = writing
            .and_then(|writing| writing.get("reviewRetries"))
            .and_then(serde_json::Value::as_u64)
            .map(|value| (value as usize).min(10))
            .unwrap_or(1);
        // writing.reviewMode（auto/manual，缺省 auto）。
        let chapter_review_mode = match writing
            .and_then(|writing| writing.get("reviewMode"))
            .and_then(serde_json::Value::as_str)
        {
            Some("manual") => ChapterReviewMode::Manual,
            _ => ChapterReviewMode::Auto,
        };
        Self {
            notify_channels,
            writing_review_retries,
            chapter_review_mode,
            ..Default::default()
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
    /// 189 号：时间线节拍提取端口（None = 不沉淀；书籍级开关仍需打开才触发）。
    pub timeline_beats: Option<&'a dyn crate::agents::timeline_settler::TimelineBeatsChat>,
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
    #[error("Operation aborted: the user requested to stop this task.")]
    Aborted,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// 链内安全点检查（TS `throwIfOperationAborted` 等价）：置位即抛中止。
/// 只在无磁盘半成品的相位调用——检查点之间不存在部分落盘（落盘为
/// staged→backup→就位三段式原子事务），中止即全书状态保持检查点前原样。
fn check_aborted(config: &WriteNextConfig) -> Result<(), WriteNextError> {
    if config
        .abort
        .as_ref()
        .is_some_and(|flag| *flag.lock().unwrap())
    {
        return Err(WriteNextError::Aborted);
    }
    Ok(())
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
pub(crate) struct PreparedWriteInput {
    pub(crate) chapter_intent: Option<String>,
    pub(crate) chapter_memo: Option<ChapterMemo>,
    pub(crate) chapter_intent_data: Option<ChapterIntent>,
    pub(crate) context_package: Option<ContextPackage>,
    pub(crate) rule_stack: Option<RuleStack>,
}

/// 阶段日志（127 号）：TS `logStage` 同构——"阶段：{message}" / "Stage: {message}"
/// 前缀 + info 级；经 config.on_log 上抛（None 静默）。
fn stage_log(config: &WriteNextConfig, language: WritingLanguage, zh: &str, en: &str) {
    let Some(on_log) = &config.on_log else { return };
    let message = if language == WritingLanguage::En {
        format!("Stage: {en}")
    } else {
        format!("阶段：{zh}")
    };
    on_log("info", &message);
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
    // 136 号：TS writeNextChapter 的运行快照（running → 终态/失败三点）。
    // model 省略备案：Rust 装配无单值 config.model（多 agent 各自解析端点）。
    let book_dir = state.book_dir(book_id);
    let chapter_number = state.get_next_chapter_number(book_id).await?;
    let padded_chapter = format!("{chapter_number:04}");
    let run_path = format!("story/runtime/chapter-{padded_chapter}.run.json");
    let run_id = format!("{book_id}:chapter-{padded_chapter}");
    let base_stage = format!("chapter-{chapter_number}");
    crate::production::write_production_run_snapshot(
        &book_dir,
        &run_path,
        &crate::production::ProductionRunSnapshot::create(crate::production::CreateRunInput {
            kind: crate::production::ProductionKind::LongFiction,
            id: run_id.clone(),
            status: crate::production::ProductionRunStatus::Running,
            stage: base_stage.clone(),
            artifacts: Vec::new(),
            observations: Vec::new(),
            model: None,
            skill_ids: Some(vec!["inkos-long-writing".to_string()]),
            resume_cursor: Some(chapter_number.to_string()),
            error: None,
        }),
    )
    .await
    .map_err(|e| WriteNextError::Persistence(e.to_string()))?;

    let outcome =
        write_next_chapter_locked(state, agents, ctx, config, book_id, word_count, temperature_override, external_context)
            .await;
    match outcome {
        Ok(result) => {
            let chapter_file = find_persisted_chapter_file(&book_dir, &padded_chapter).await;
            let Some(chapter_file) = chapter_file else {
                let error = WriteNextError::Persistence(format!(
                    "Chapter {} completed without a persisted chapter artifact.",
                    result.chapter_number
                ));
                publish_failed_run_snapshot(
                    &book_dir, &run_path, &run_id, &base_stage, config, &error,
                )
                .await;
                return Err(error);
            };
            // TS：lengthTelemetry ?? buildLengthSpec(wordCount ?? book 基准)。
            let book = state.load_book_config(book_id).await.ok();
            let spec = result.length_telemetry.as_ref().map(|telemetry| {
                crate::models::length_governance::LengthSpec {
                    target: telemetry.target,
                    soft_min: telemetry.soft_min,
                    soft_max: telemetry.soft_max,
                    hard_min: telemetry.hard_min,
                    hard_max: telemetry.hard_max,
                    counting_mode: telemetry.counting_mode,
                }
            }).unwrap_or_else(|| {
                let language = match book
                    .as_ref()
                    .and_then(|b| b.language.as_deref())
                {
                    Some("en") => crate::utils::language::WritingLanguage::En,
                    _ => crate::utils::language::WritingLanguage::Zh,
                };
                crate::utils::length_metrics::build_length_spec(
                    word_count.unwrap_or_else(|| book.as_ref().map(|b| b.chapter_word_count).unwrap_or(3000)),
                    language,
                )
            });
            let chapter_path = format!("chapters/{chapter_file}");
            let artifacts = vec![
                chapter_path.clone(),
                "chapters/index.json".to_string(),
                "story/current_state.md".to_string(),
                "story/pending_hooks.md".to_string(),
                format!("story/snapshots/{}", result.chapter_number),
                format!("story/runtime/chapter-{padded_chapter}.trace.json"),
            ];
            let status = if result.status == "ready-for-review" {
                crate::production::ProductionRunStatus::Complete
            } else {
                crate::production::ProductionRunStatus::NeedsReview
            };
            let observation = crate::production::create_range_observation(
                "chapter-length",
                result.word_count,
                spec.target,
                spec.hard_min,
                spec.hard_max,
                match spec.counting_mode {
                    crate::models::length_governance::LengthCountingMode::ZhChars => "zh_chars",
                    crate::models::length_governance::LengthCountingMode::EnWords => "en_words",
                },
                Some(chapter_path),
                None,
            );
            crate::production::write_production_run_snapshot(
                &book_dir,
                &run_path,
                &crate::production::ProductionRunSnapshot::create(crate::production::CreateRunInput {
                    kind: crate::production::ProductionKind::LongFiction,
                    id: run_id,
                    status,
                    stage: base_stage,
                    artifacts,
                    observations: vec![observation],
                    model: None,
                    skill_ids: Some(vec!["inkos-long-writing".to_string()]),
                    resume_cursor: Some(result.chapter_number.to_string()),
                    error: None,
                }),
            )
            .await
            .map_err(|e| WriteNextError::Persistence(e.to_string()))?;
            Ok(result)
        }
        Err(error) => {
            publish_failed_run_snapshot(&book_dir, &run_path, &run_id, &base_stage, config, &error)
                .await;
            Err(error)
        }
    }
}

/// `chapters/` 下 `{padded}_*.md` 的章文件名（TS readdir + 前缀匹配）。
async fn find_persisted_chapter_file(book_dir: &std::path::Path, padded: &str) -> Option<String> {
    let prefix = format!("{padded}_");
    let mut entries = tokio::fs::read_dir(book_dir.join("chapters")).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&prefix) && name.ends_with(".md") {
            return Some(name);
        }
    }
    None
}

/// 失败/取消快照（TS catch 分支：cancelled 按 abort 信号判定；快照写失败
/// 吞掉——不覆盖原始错误）。
async fn publish_failed_run_snapshot(
    book_dir: &std::path::Path,
    run_path: &str,
    run_id: &str,
    stage: &str,
    config: &WriteNextConfig,
    error: &WriteNextError,
) {
    let cancelled = config
        .abort
        .as_ref()
        .is_some_and(|flag| *flag.lock().unwrap());
    let snapshot = crate::production::ProductionRunSnapshot::create(crate::production::CreateRunInput {
        kind: crate::production::ProductionKind::LongFiction,
        id: run_id.to_string(),
        status: if cancelled {
            crate::production::ProductionRunStatus::Cancelled
        } else {
            crate::production::ProductionRunStatus::Failed
        },
        stage: stage.to_string(),
        artifacts: Vec::new(),
        observations: Vec::new(),
        model: None,
        skill_ids: Some(vec!["inkos-long-writing".to_string()]),
        resume_cursor: None,
        error: Some(error.to_string()),
    });
    let _ = crate::production::write_production_run_snapshot(book_dir, run_path, &snapshot).await;
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
    check_aborted(config)?;
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

    stage_log(config, pipeline_language, "规划与上下文编排", "plan & context composition");
    let write_input = prepare_write_input(
        state, agents, ctx, config, &book, &book_dir, chapter_number, external_context,
    )
    .await?;

    // ── 1. 撰写草稿 ──
    stage_log(config, pipeline_language, "撰写章节草稿", "write chapter draft");
    tracing::info!(target: "write-next", "撰写章节草稿");
    let writer_ctx = WriterCtx {
        project_root: ctx.project_root,
        builtin_genres_dir: ctx.builtin_genres_dir,
        prompt_store: ctx.prompt_store,
        state_store: ctx.state_store,
    };
    let mut output = write_chapter(
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
    // 188 号：LLM 偶发返回空正文（服务抖动/中转异常/只回 reasoning 不回
    // content）——自动重试一次；仍空则由 review cycle 空内容检查兜底报错。
    if output.content.trim().is_empty() {
        tracing::warn!(target: "write-next", "writer 返回空正文，自动重试一次");
        output = write_chapter(
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
    }
    // 检查点②：草稿落定后（TS 1844——writeChapter 返回即查）。
    check_aborted(config)?;

    // G11/372 号：best-of-N 多版生成选优（治理开启时）。候选仅存内存——
    // 落盘仍由既有 persist 链承担；quick 评分走环内 auditor 端口。
    if book
        .governance
        .as_ref()
        .and_then(|g| g.best_of_n.as_ref().map(|c| c.enabled.unwrap_or(false)))
        .unwrap_or(false)
    {
        let quick_score = |audit_result: &crate::agents::continuity::AuditResult| -> Option<i64> {
            audit_result.overall_score.map(i64::from)
        };
        let first_audit = agents
            .auditor
            .audit_chapter(&output.content, None, None)
            .await
            .ok();
        let first_score = first_audit.as_ref().and_then(quick_score);
        let plan = crate::utils::best_of_n::resolve_best_of_n_plan(
            book.governance.as_ref().and_then(|g| g.best_of_n.as_ref()),
            first_score,
        );
        if plan.extra_candidates > 0 {
            let mut versions: Vec<crate::agents::writer::WriteChapterOutput> = vec![output.clone()];
            let mut scores: Vec<Option<i64>> = vec![first_score];
            for index in 0..plan.extra_candidates {
                check_aborted(config)?;
                tracing::info!(target: "write-next", "best-of-N: generating candidate {}", index + 2);
                let candidate = write_chapter(
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
                if candidate.content.trim().is_empty() {
                    scores.push(None);
                    versions.push(candidate);
                    continue;
                }
                let audit = agents
                    .auditor
                    .audit_chapter(&candidate.content, None, None)
                    .await
                    .ok();
                scores.push(audit.as_ref().and_then(quick_score));
                versions.push(candidate);
            }
            let payload: Vec<serde_json::Value> = versions
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    serde_json::json!({ "payload": index, "score": scores.get(index).cloned().flatten() })
                })
                .collect();
            let best = crate::utils::best_of_n::select_best_candidate(&payload);
            let winner = best.index;
            tracing::info!(
                target: "write-next",
                "best-of-N: {} candidate(s), adopted #{} (score {:?})",
                versions.len(),
                winner + 1,
                scores.get(winner).cloned().flatten()
            );
            output = versions.remove(winner);
            // usage 以各版累加近似（多版成本计入本章）。
            if let Some(usage) = versions.into_iter().map(|v| v.token_usage).reduce(|mut acc, usage| {
                acc.prompt_tokens += usage.prompt_tokens;
                acc.completion_tokens += usage.completion_tokens;
                acc.total_tokens += usage.total_tokens;
                acc
            }) {
                output.token_usage.prompt_tokens += usage.prompt_tokens;
                output.token_usage.completion_tokens += usage.completion_tokens;
                output.token_usage.total_tokens += usage.total_tokens;
            }
        }
    }

    let writer_count = count_chapter_length(&output.content, length_spec.counting_mode);

    let mut total_usage = output.token_usage;
    let final_content: String;
    let revised: bool;
    let mut audit_result: AuditResult;
    let post_revise_count: u32;
    let repair_applied: bool;

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
        repair_applied = false;
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
        let _ = repair_applied;
    } else {
        // ── 2. 审核环（assess → revise → assess，快照择优） ──
        stage_log(config, pipeline_language, "审核环", "review cycle");
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
        repair_applied = review.repair_applied;
    }

    // 检查点③：审查环收敛后（TS 1932——manual 模式同样经过）。
    check_aborted(config)?;

    // ── 3b. 轻量晋级（落盘前，零 LLM） ──
    run_promotion_pass(&book_dir, chapter_number).await;

    // 检查点④：落盘前（TS 1957——promotion 后、持久化装配前，中止则全书
    // 状态与本章产物完全未动，安全回滚点）。
    check_aborted(config)?;

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
    // R2/359 号：张力曲线启发式告警——只报涉及当前章的告警（区间扩展中），
    // 避免同一平坦段/弱钩段在连续各章重复入账；无分曲线不产告警。
    {
        let summaries_for_tension = tokio::fs::read_to_string(book_dir.join("story/chapter_summaries.md"))
            .await
            .unwrap_or_default();
        let tension_rows =
            crate::utils::story_markdown::parse_chapter_summaries_markdown(&summaries_for_tension);
        let tension_rows: Vec<crate::utils::tension_curve::TensionRow> = tension_rows
            .iter()
            .map(|row| crate::utils::tension_curve::TensionRow {
                chapter: row.chapter,
                conflict_level: row.conflict_level,
                reveal_level: row.reveal_level,
            })
            .collect();
        let (_, tension_warnings) = crate::utils::tension_curve::analyze_tension_curve(
            &tension_rows,
            pipeline_language.into(),
            &crate::utils::tension_curve::TENSION_WARNING_DEFAULTS,
        );
        for warning in tension_warnings
            .iter()
            .filter(|warning| warning.chapters.contains(&i64::from(chapter_number)))
        {
            tracing::warn!(target: "write-next", "[tension] {}", warning.description);
            audit_result.issues.push(AuditIssue {
                severity: AuditSeverity::Warning,
                category: "tension-curve".to_string(),
                description: warning.description.clone(),
                suggestion: warning.suggestion.clone(),
                repair_scope: None,
            });
        }
    }
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
        post_revise_count,
        final_count: final_word_count,
        repair_applied,
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

    stage_log(config, pipeline_language, "真相结算与校验", "settle & truth validation");
    let truth = validate_chapter_truth_persistence(TruthValidationParams {
        writer: agents.settler,
        validator: &validator,
        book: &book,
        book_dir: &book_dir,
        chapter_number,
        baseline_chapter: None,
        allow_new_hooks: None,
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

    // ── R3/361 号：审查指标沉淀（overallScore + severity 计数；contentHash 幂等——
    // 同章同内容重放跳过，修订后内容更新覆盖）。失败仅告警，不阻断管线。
    {
        let mut critical_count: i64 = 0;
        let mut warning_count: i64 = 0;
        let mut info_count: i64 = 0;
        for issue in &audit_result.issues {
            match issue.severity {
                AuditSeverity::Critical => critical_count += 1,
                AuditSeverity::Info => info_count += 1,
                AuditSeverity::Warning => warning_count += 1,
            }
        }
        let content_hash = {
            use crate::utils::quality_trend::review_metric_content_hash;
            review_metric_content_hash(&crate::utils::quality_trend::ReviewMetricRow {
                chapter: i64::from(chapter_number),
                overall_score: audit_result.overall_score.map(i64::from),
                passed: audit_result.passed,
                critical_count,
                warning_count,
                info_count,
                recorded_at: String::new(),
            })
        };
        let recorded = (|| -> Result<bool, crate::EngineError> {
            let memory = crate::state::memory_db::MemoryDb::open(&book_dir)?;
            memory.record_review_metric(&crate::state::memory_db::StoredReviewMetric {
                chapter: i64::from(chapter_number),
                overall_score: audit_result.overall_score.map(i64::from),
                passed: audit_result.passed,
                critical_count,
                warning_count,
                info_count,
                content_hash,
                recorded_at: crate::utils::utc_time::utc_now_iso(),
            })
        })();
        match recorded {
            Ok(false) => tracing::info!(target: "write-next", "[review-metrics] ch{chapter_number} 同内容重放，幂等跳过"),
            Ok(true) => {}
            Err(error) => tracing::warn!(target: "write-next", "[review-metrics] {error}"),
        }
    }

    // ── R5/366 号：反AI规则扫描（detect 消费）——命中并入审计问题（reviser 按
    // issue.suggestion=replacement 修复）。规则缺失/解析失败零打扰。
    {
        if let Ok(rules_raw) = tokio::fs::read_to_string(book_dir.join("story/anti_ai_rules.json")).await {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&rules_raw) {
                if let Some(rules) = parsed.get("rules").and_then(serde_json::Value::as_array) {
                    let valid: Vec<crate::utils::rule_experience_engine::AntiAiRule> = rules
                        .iter()
                        .filter_map(|rule| crate::utils::rule_experience_engine::validate_anti_ai_rule(rule).rule)
                        .collect();
                    if !valid.is_empty() {
                        let hits = crate::utils::rule_experience_engine::scan_anti_ai_rules(&final_content, &valid);
                        if !hits.is_empty() {
                            tracing::warn!(target: "write-next", "[anti-ai] {} hit(s) in ch{chapter_number}", hits.len());
                            for hit in hits {
                                audit_result.issues.push(AuditIssue {
                                    severity: match hit.severity {
                                        crate::utils::rule_experience_engine::AntiAiSeverity::Critical => AuditSeverity::Critical,
                                        crate::utils::rule_experience_engine::AntiAiSeverity::Warning => AuditSeverity::Warning,
                                        crate::utils::rule_experience_engine::AntiAiSeverity::Info => AuditSeverity::Info,
                                    },
                                    category: "anti-ai-rule".to_string(),
                                    description: format!("{}（×{}）", hit.message, hit.count),
                                    suggestion: hit.replacement.unwrap_or_else(|| "按本书反AI规则改写".to_string()),
                                    repair_scope: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

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
    stage_log(config, pipeline_language, "章节落盘", "persist chapter");
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

    // ── 6b. 通知通道派发（111 号：TS "6. Send notification" + emitWebhook
    // 逐字——emoji 标题 + 正文行 + 非 info 问题行；随后 pipeline-complete
    // 结构化 webhook）。
    if let Some(channels) = &config.notify_channels {
        if !channels.is_empty() {
            let audit_result = &result.audit_result;
            let status_emoji = if chapter_status == "state-degraded" {
                "🧯"
            } else if audit_result.passed {
                "✅"
            } else {
                "⚠️"
            };
            let chapter_length =
                crate::utils::length_metrics::format_length_count(final_word_count, length_spec.counting_mode);
            let mut body_lines: Vec<String> = vec![
                format!("**{}** | {}", persistence_output.title, chapter_length),
                if revised { "📝 已自动修正".to_string() } else { String::new() },
                if chapter_status == "state-degraded" {
                    "状态结算: 已降级保存，需先修复 state 再继续".to_string()
                } else {
                    format!("审稿: {}", if audit_result.passed { "通过" } else { "需人工审核" })
                },
            ];
            body_lines.extend(
                audit_result
                    .issues
                    .iter()
                    .filter(|issue| issue.severity != AuditSeverity::Info)
                    .map(|issue| format!("- [{}] {}", severity_text(issue.severity), issue.description)),
            );
            let body = body_lines
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            crate::notify::dispatch_notification(
                channels,
                &crate::notify::NotifyMessage {
                    title: format!("{status_emoji} {} 第{chapter_number}章", book.title),
                    body,
                },
            )
            .await;
            crate::notify::dispatch_webhook_event(
                channels,
                &crate::notify::WebhookPayload {
                    event: "pipeline-complete".to_string(),
                    book_id: book_id.to_string(),
                    chapter_number: Some(chapter_number),
                    timestamp: crate::utils::utc_time::utc_now_iso(),
                    data: Some(serde_json::json!({
                        "title": persistence_output.title,
                        "wordCount": final_word_count,
                        "passed": audit_result.passed,
                        "revised": revised,
                        "status": chapter_status,
                    })),
                },
            )
            .await;
        }
    }

    // ── 6b-2. 运行时观测工件保留策略（200 号：默认每书留最近 20 章的
    // run/trace/context/rule-stack；trace 含完整 LLM 轨迹，长书无限增长。
    // 失败/清理均为事后打扫，不阻断——cleanup 失败连日志都省了）。 ──
    {
        let keep = crate::production::runtime_retention_chapters();
        if keep > 0 {
            crate::production::prune_runtime_artifacts(&book_dir, chapter_number, keep).await;
        }
    }

    // ── 6c. 时间线节拍自动沉淀（189 号：书籍级开关默认关；失败不阻断） ──
    if book.writing.as_ref().and_then(|w| w.auto_timeline_beats).unwrap_or(false) {
        if let Some(beats_chat) = ctx.timeline_beats {
            stage_log(config, pipeline_language, "时间线节拍沉淀", "timeline beat settle");
            match crate::pipeline::timeline_settle::settle_beats_for_chapter(
                &book_dir,
                beats_chat,
                chapter_number,
                &persistence_output.title,
                &persistence_output.chapter_summary,
                pipeline_language,
            )
            .await
            {
                Ok(Some(applied)) if applied > 0 => {
                    tracing::info!(target: "write-next", "[timeline] 已自动沉淀第{chapter_number}章节拍（{applied} 条情节线）");
                }
                Ok(_) => tracing::info!(target: "write-next", "[timeline] 本章无线条被推进，跳过节拍落盘"),
                Err(error) => {
                    tracing::warn!(target: "write-next", "[timeline] 节拍自动沉淀失败（不影响章节产物）: {error}");
                }
            }
        }
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
pub(crate) async fn persist_audit_drift_guidance(
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
        "第{chapter_number}章未达到篇幅预算（{}-{}，实际 {final_count}）。",
        spec.hard_min, spec.hard_max
    );
    let en = format!(
        "Chapter {chapter_number} is outside its length budget ({}-{}, actual {final_count}).",
        spec.hard_min, spec.hard_max
    );
    vec![match spec.counting_mode {
        LengthCountingMode::ZhChars => zh,
        LengthCountingMode::EnWords => en,
    }]
}

fn assert_chapter_content_not_empty(content: &str, stage: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err(format!(
            "chapter content is empty after `{stage}`（模型未返回有效正文，请检查模型服务是否正常或更换模型后重试）"
        ));
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
pub(crate) async fn prepare_write_input(
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
                    chapter_word_count: book.chapter_word_count,
                    book_writing: book.writing.as_ref(),
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
    // 216 号：绑定素材引用选段注入（TS runner referenceContextProvider 同款）。
    let reference_selector = crate::agents::composer::LlmReferenceSelector { chat: agents.composer };
    let reference_provider = crate::references::ProductionReferenceContextProvider {
        project_root: ctx.project_root,
        book_id: &book.id,
        selector: &reference_selector,
    };
    // 244 号：记忆语义精简器（TS runner memorySemanticSelector 同款）。
    let memory_selector = crate::agents::composer::LlmMemorySelector { chat: agents.composer };
    let composed: ComposeChapterOutput = compose_governed_chapter(&ComposeChapterInput {
        book_language: book.language.as_deref(),
        book_dir,
        chapter_number,
        plan: &plan,
        context_budget: ctx.context_budget,
        compiler: Some(&compiler),
        outline_section_selector: Some(&selector),
        reference_context_provider: Some(&reference_provider),
        memory_semantic_selector: Some(&memory_selector),
        on_context_compression: config.on_context_compression.clone(),
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
            timeline_beats: None,
        };
        let config = WriteNextConfig {
            chapter_review_mode: ChapterReviewMode::Manual,
            writing_review_retries: 1,
            input_governance_mode: InputGovernanceMode::V2,
            abort: None,
            notify_channels: None,
            on_context_compression: None,
            on_log: None,
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

    /// 127 号：阶段日志回调——manual 链四阶段有序双语（zh 书）。
    #[tokio::test]
    async fn stage_logs_emit_ordered_stage_messages() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let builtin = dir.path().join("builtin");
        let book = project.join("books").join("b1");
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
        let agents = WriteNextAgents {
            writer: &chat,
            planner: &chat,
            composer: &chat,
            reviser: &chat,
            auditor: &chat,
            full_auditor: None,
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
            timeline_beats: None,
        };
        let logs = std::sync::Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
        let sink = logs.clone();
        let config = WriteNextConfig {
            chapter_review_mode: ChapterReviewMode::Manual,
            writing_review_retries: 1,
            input_governance_mode: InputGovernanceMode::V2,
            abort: None,
            notify_channels: None,
            on_context_compression: None,
            on_log: Some(std::sync::Arc::new(move |level: &str, message: &str| {
                sink.lock().unwrap().push((level.to_string(), message.to_string()));
            })),
        };

        write_next_chapter(&state, &agents, &ctx, &config, "b1", None, None, None)
            .await
            .expect("write-next 应成功");

        let logs = logs.lock().unwrap();
        // manual 链四阶段（无审核环），zh 书 → 阶段前缀 + info 级。
        let messages: Vec<&str> = logs.iter().map(|(_, message)| message.as_str()).collect();
        assert_eq!(
            messages,
            vec![
                "阶段：规划与上下文编排",
                "阶段：撰写章节草稿",
                "阶段：真相结算与校验",
                "阶段：章节落盘",
            ],
            "{logs:?}"
        );
        assert!(logs.iter().all(|(level, _)| level == "info"));
    }

    // ---- 188 号：writer 空正文自动重试一次 ----

    /// 首次返回空正文、之后委托真实脚本响应的 writer（模拟 LLM 瞬时抖动）。
    struct FlakyEmptyWriter<'a> {
        inner: &'a ScriptChat,
        calls: StdMutex<u32>,
    }

    impl<'a> FlakyEmptyWriter<'a> {
        fn new(inner: &'a ScriptChat) -> Self {
            Self { inner, calls: StdMutex::new(0) }
        }
    }

    #[async_trait]
    impl WriterChat for FlakyEmptyWriter<'_> {
        async fn chat(
            &self,
            messages: Vec<LLMMessage>,
            temperature: f64,
        ) -> Result<ChatOutcome, String> {
            let is_first = {
                let mut calls = self.calls.lock().unwrap();
                *calls += 1;
                *calls == 1
            };
            if is_first {
                // 第一次：空正文（LLM 抖动）。
                return Ok(ChatOutcome { content: String::new(), usage: None });
            }
            WriterChat::chat(self.inner, messages, temperature).await
        }
    }

    /// 空正文触发一次自动重试，重试成功后管线正常完成（章节落盘）。
    #[tokio::test]
    async fn empty_writer_output_is_retried_once_then_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let (project, builtin) = abort_fixture(dir.path()).await;
        let state = StateManager::new(&project);
        let chat = script();
        let flaky = FlakyEmptyWriter::new(&chat);
        let agents = WriteNextAgents {
            writer: &flaky,
            planner: &chat,
            composer: &chat,
            reviser: &chat,
            auditor: &chat,
            full_auditor: None,
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
            timeline_beats: None,
        };
        let config = WriteNextConfig::default();

        write_next_chapter(&state, &agents, &ctx, &config, "b1", None, None, None)
            .await
            .expect("重试后应成功");
        // 重试恰好一次后成功：writer 链上多处复用端口，调用数 ≥ 2（原 + 重试）。
        assert!(*flaky.calls.lock().unwrap() >= 2);
        assert_eq!(chapters_md_count(&project.join("books").join("b1")), 1);
    }

    // ---- 101 号：链内安全点中止（TS throwIfOperationAborted 检查点） ----

    /// 中止测试夹具：与 manual 测试同构的最小项目。
    async fn abort_fixture(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let project = dir.join("project");
        let builtin = dir.join("builtin");
        let book = project.join("books").join("b1");
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
            r#"{"id":"b1","title":"测试书","platform":"other","genre":"xianxia","status":"active","targetChapters":100,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .await
        .unwrap();
        (project, builtin)
    }

    /// 草稿 LLM 调用进行中置位中止标志的 writer 端口（模拟用户在草稿
    /// 生成期间点击停止——检查点②在草稿返回后立即命中）。
    struct AbortingWriter<'a> {
        script: &'a ScriptChat,
        flag: AbortHandle,
    }

    #[async_trait]
    impl WriterChat for AbortingWriter<'_> {
        async fn chat(
            &self,
            messages: Vec<LLMMessage>,
            _temperature: f64,
        ) -> Result<ChatOutcome, String> {
            *self.flag.lock().unwrap() = true;
            Ok(ChatOutcome {
                content: self.script.dispatch(&messages),
                usage: Some(AuditTokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
            })
        }
    }

    fn chapters_md_count(book: &std::path::Path) -> usize {
        std::fs::read_dir(book.join("chapters"))
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .ends_with(".md")
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn abort_before_entry_stops_without_any_llm_call() {
        let dir = tempfile::tempdir().unwrap();
        let (project, builtin) = abort_fixture(dir.path()).await;
        let state = StateManager::new(&project);
        let chat = script();
        let agents = WriteNextAgents {
            writer: &chat,
            planner: &chat,
            composer: &chat,
            reviser: &chat,
            auditor: &chat,
            full_auditor: None,
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
            timeline_beats: None,
        };
        let flag: AbortHandle = std::sync::Arc::new(StdMutex::new(true));
        let config = WriteNextConfig {
            abort: Some(flag),
            ..Default::default()
        };

        let error = write_next_chapter(&state, &agents, &ctx, &config, "b1", None, None, None)
            .await
            .expect_err("入口检查点应立即中止");

        assert!(matches!(error, WriteNextError::Aborted));
        // 入口即停：零 LLM 调用、零章节落盘。
        assert!(chat.calls.lock().unwrap().is_empty(), "不应发起任何 LLM 调用");
        assert_eq!(chapters_md_count(&project.join("books").join("b1")), 0);
        assert_eq!(state.get_next_chapter_number("b1").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn abort_during_draft_stops_at_safe_point_without_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let (project, builtin) = abort_fixture(dir.path()).await;
        let state = StateManager::new(&project);
        let chat = script();
        let flag: AbortHandle = std::sync::Arc::new(StdMutex::new(false));
        let aborting = AbortingWriter { script: &chat, flag: flag.clone() };
        let agents = WriteNextAgents {
            writer: &aborting,
            planner: &chat,
            composer: &chat,
            reviser: &chat,
            auditor: &chat,
            full_auditor: None,
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
            timeline_beats: None,
        };
        let config = WriteNextConfig {
            abort: Some(flag),
            ..Default::default()
        };

        let error = write_next_chapter(&state, &agents, &ctx, &config, "b1", None, None, None)
            .await
            .expect_err("草稿返回后检查点②应中止");

        assert!(matches!(error, WriteNextError::Aborted));
        // 草稿已生成（planner/composer/writer 均被调用）但零落盘——安全点
        // 语义：检查点之间不存在部分写盘，中止即全书保持原样。
        let calls = chat.calls.lock().unwrap().clone();
        assert!(
            calls.iter().any(|c| c.contains("作家")),
            "草稿调用应已发生：{calls:?}"
        );
        assert_eq!(chapters_md_count(&project.join("books").join("b1")), 0);
        assert_eq!(state.get_next_chapter_number("b1").await.unwrap(), 1);
        // 真相面同样零落盘（chapter_summaries 无 markdown 投影）。
        let book = project.join("books").join("b1");
        assert!(!book.join("story").join("chapter_summaries.md").exists());
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
            timeline_beats: None,
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

    // ---- 189 号：时间线节拍自动沉淀（书籍级开关，默认关） ----

    struct FixedBeats {
        result: Result<Vec<crate::models::timeline::PlotlineBeat>, String>,
        calls: StdMutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl crate::agents::timeline_settler::TimelineBeatsChat for FixedBeats {
        async fn beats(
            &self,
            req: crate::agents::timeline_settler::BeatsRequest<'_>,
        ) -> Result<Vec<crate::models::timeline::PlotlineBeat>, String> {
            self.calls.lock().unwrap().push(req.chapter_title.to_string());
            self.result.clone()
        }
    }

    /// 全管线环境（同 manual_mode 用例）：返回 (state, project, builtin, book_dir)。
    async fn setup_timeline_beats_env(
        dir: &tempfile::TempDir,
        writing: serde_json::Value,
    ) -> (StateManager, std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let project = dir.path().join("project");
        let builtin = dir.path().join("builtin");
        let book = project.join("books").join("b1");
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
        let mut book_json = serde_json::json!({
            "id": "b1", "title": "测试书", "platform": "other", "genre": "xianxia",
            "status": "active", "targetChapters": 100, "chapterWordCount": 3000,
            "language": "zh", "createdAt": "", "updatedAt": "",
        });
        book_json["writing"] = writing;
        tokio::fs::write(book.join("book.json"), book_json.to_string()).await.unwrap();
        (StateManager::new(&project), project, builtin, book)
    }

    fn beats_agents<'a>(chat: &'a ScriptChat) -> WriteNextAgents<'a> {
        WriteNextAgents {
            writer: chat,
            planner: chat,
            composer: chat,
            reviser: chat,
            auditor: chat,
            full_auditor: None,
            analyzer: chat,
            state_validator: chat,
            settler: chat,
        }
    }

    fn beats_config() -> WriteNextConfig {
        WriteNextConfig {
            chapter_review_mode: ChapterReviewMode::Manual,
            writing_review_retries: 1,
            input_governance_mode: InputGovernanceMode::V2,
            abort: None,
            notify_channels: None,
            on_context_compression: None,
            on_log: None,
        }
    }

    fn seeded_timeline_json() -> String {
        serde_json::json!({
            "version": 1,
            "bookId": "b1",
            "updatedAt": "2026-01-01T00:00:00.000Z",
            "plotlines": [{ "id": "main", "name": "主线", "cells": [] }],
        })
        .to_string()
    }

    #[tokio::test]
    async fn auto_timeline_beats_settle_after_persist() {
        let dir = tempfile::tempdir().unwrap();
        let (state, project, builtin, book) =
            setup_timeline_beats_env(&dir, serde_json::json!({ "autoTimelineBeats": true })).await;
        tokio::fs::write(book.join("story").join("timeline.json"), seeded_timeline_json())
            .await
            .unwrap();

        let chat = script();
        let agents = beats_agents(&chat);
        let prompt_store = InMemoryStateStore::default();
        let beats = FixedBeats {
            result: Ok(vec![crate::models::timeline::PlotlineBeat {
                plotline_id: "main".into(),
                title: Some("风起·沉淀".into()),
                note: Some("少年入场".into()),
            }]),
            calls: StdMutex::new(Vec::new()),
        };
        let ctx = WriteNextCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &prompt_store,
            state_store: &prompt_store,
            context_budget: None,
            notify: None,
            timeline_beats: Some(&beats),
        };

        let result = write_next_chapter(&state, &agents, &ctx, &beats_config(), "b1", None, None, None)
            .await
            .expect("write-next 应成功");
        assert_eq!(result.chapter_number, 1);
        // 端口拿到落盘后的章节标题（传参 = 生产语义）。
        assert_eq!(beats.calls.lock().unwrap().as_slice(), ["风起"]);

        // 节拍合并进时间线并落盘。
        let raw = tokio::fs::read_to_string(book.join("story").join("timeline.json")).await.unwrap();
        let timeline: crate::models::timeline::Timeline = serde_json::from_str(&raw).unwrap();
        assert_eq!(timeline.plotlines[0].cells.len(), 1);
        assert_eq!(timeline.plotlines[0].cells[0].chapter, 1);
        assert_eq!(timeline.plotlines[0].cells[0].title.as_deref(), Some("风起·沉淀"));
    }

    #[tokio::test]
    async fn timeline_beats_failure_does_not_fail_pipeline() {
        let dir = tempfile::tempdir().unwrap();
        let (state, project, builtin, book) =
            setup_timeline_beats_env(&dir, serde_json::json!({ "autoTimelineBeats": true })).await;
        tokio::fs::write(book.join("story").join("timeline.json"), seeded_timeline_json())
            .await
            .unwrap();

        let chat = script();
        let agents = beats_agents(&chat);
        let prompt_store = InMemoryStateStore::default();
        let beats = FixedBeats { result: Err("llm down".into()), calls: StdMutex::new(Vec::new()) };
        let ctx = WriteNextCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &prompt_store,
            state_store: &prompt_store,
            context_budget: None,
            notify: None,
            timeline_beats: Some(&beats),
        };

        let result = write_next_chapter(&state, &agents, &ctx, &beats_config(), "b1", None, None, None)
            .await
            .expect("沉淀失败不应拖垮管线");
        assert_eq!(result.chapter_number, 1);
        // 时间线保持原样（空 cells）。
        let raw = tokio::fs::read_to_string(book.join("story").join("timeline.json")).await.unwrap();
        let timeline: crate::models::timeline::Timeline = serde_json::from_str(&raw).unwrap();
        assert!(timeline.plotlines[0].cells.is_empty());
    }

    #[tokio::test]
    async fn timeline_beats_default_off_skips_port() {
        let dir = tempfile::tempdir().unwrap();
        let (state, project, builtin, book) =
            setup_timeline_beats_env(&dir, serde_json::json!({})).await;
        tokio::fs::write(book.join("story").join("timeline.json"), seeded_timeline_json())
            .await
            .unwrap();

        let chat = script();
        let agents = beats_agents(&chat);
        let prompt_store = InMemoryStateStore::default();
        let beats = FixedBeats { result: Ok(vec![]), calls: StdMutex::new(Vec::new()) };
        let ctx = WriteNextCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &prompt_store,
            state_store: &prompt_store,
            context_budget: None,
            notify: None,
            timeline_beats: Some(&beats),
        };

        let result = write_next_chapter(&state, &agents, &ctx, &beats_config(), "b1", None, None, None)
            .await
            .expect("write-next 应成功");
        assert_eq!(result.chapter_number, 1);
        // 默认关：端口从未被调用，时间线未被改写。
        assert!(beats.calls.lock().unwrap().is_empty());
        let raw = tokio::fs::read_to_string(book.join("story").join("timeline.json")).await.unwrap();
        let timeline: crate::models::timeline::Timeline = serde_json::from_str(&raw).unwrap();
        assert!(timeline.plotlines[0].cells.is_empty());
    }
}

