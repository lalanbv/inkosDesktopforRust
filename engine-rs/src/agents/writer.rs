//! writer 编排 —— 章节创作 + 状态结算 + 后写校验 + 原子落盘。
//!
//! 移植自 `packages/core/src/agents/writer.ts`（1502 行）。三阶段流水线：
//! - **Phase 1 创作**（temp 0.7）：真相文件/治理上下文装配 → system/user prompt →
//!   LLM → [`parse_creative_output`]
//! - **Phase 2 结算**（observer temp 0.5 → settler temp 0.3）：观察 → 回写，
//!   delta 输出优先，legacy 输出经 governed 表格合并
//! - **Phase 3 后写校验**（零 LLM）：[`normalize_post_write_surface`] +
//!   [`validate_post_write`] + 跨章重复/段落漂移 + AI 味 + hook 健康
//!
//! LLM 调用经 [`WriterChat`] trait 注入（复用 AuditorChat 模式：测试 mock /
//! 生产 BaseAgent 实现）；落盘经 [`commit_atomic_file_set`] 事务提交。
//!
//! ## 移植纪律
//! prompt 模板（zh/en 双语逐字）、上下文预算（LEGACY_WRITER_CONTEXT_BUDGET）、
//! 结算工作集裁剪条件、legacy 回退链（delta 解析失败 → 全量解析 + 表格合并）
//! 均为 load-bearing，与 TS 逐字对齐。

use std::collections::HashSet;
use std::path::Path;

use regex::Regex;
use std::sync::OnceLock;

use crate::agents::ai_tells::analyze_ai_tells;
use crate::agents::continuity::{AuditSeverity, AuditTokenUsage, ChatOutcome};
use crate::agents::observer_prompts::{
    build_observer_system_prompt, build_observer_user_prompt,
};
use crate::agents::post_write_validator::{
    detect_cross_chapter_repetition, detect_paragraph_length_drift, normalize_post_write_surface,
    validate_post_write, PostWriteViolation,
};
use crate::agents::rules_reader::{read_book_rules, read_genre_profile};
use crate::agents::settler_delta_parser::parse_settler_delta_output;
use crate::agents::settler_parser::{parse_settlement_output, SettlementOutput};
use crate::agents::settler_prompts::{
    build_settler_system_prompt, build_settler_user_prompt, SettlerUserPromptInput,
};
use crate::agents::writer_parser::parse_creative_output;
use crate::agents::writer_prompts::{
    build_writer_system_prompt, FanficContext, InputProfile, WriterPromptMode,
    WriterSystemPromptInput,
};
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::book::BookConfig;
use crate::models::book_rules::BookRules;
use crate::models::genre_profile::GenreProfile;
use crate::models::input_governance::{
    ChapterIntent, ChapterMemo, ContextPackage, RuleStack,
};
use crate::models::length_governance::{LengthCountingMode, LengthSpec};
use crate::models::runtime_state::{HookOps, RuntimeStateDelta};
use crate::prompts::prompt_pack::{
    append_prompt_pack_guidance, LoadPromptPackPromptInput, PromptPackPromptNotFoundError,
};
use crate::state::reducer::RuntimeStateSnapshot;
use crate::state::runtime_state_store::{
    build_runtime_state_artifacts, RuntimeStateArtifacts,
};
use crate::state::store::StateStore;
use crate::utils::atomic_file_set::{
    commit_atomic_file_set, AtomicFileSet, AtomicFileWrite, FileContent,
};
use crate::utils::context_filter::{
    cap_context_block, filter_character_matrix, filter_emotional_arcs, filter_hooks,
    filter_subplots, filter_summaries,
};
use crate::utils::governed_context::{
    build_governed_memory_evidence_blocks, GovernedMemoryEvidenceBlocks,
};
use crate::utils::governed_working_set::{
    build_governed_character_matrix_working_set, build_governed_hook_working_set,
    merge_character_matrix_markdown, merge_table_markdown_by_key, GovernedHookWorkingSetInput,
    GovernedMatrixWorkingSetInput,
};
use crate::utils::hook_health::analyze_hook_health;
use crate::utils::language::{utf16_len, WritingLanguage};
use crate::utils::length_metrics::{build_length_spec, count_chapter_length};
use crate::utils::long_span_fatigue::build_english_variance_brief;
use crate::utils::narrative_control::{
    build_narrative_intent_brief, render_memo_as_narrative_block,
    render_narrative_selected_context, sanitize_narrative_evidence_block,
};
use crate::utils::outline_paths::{
    read_character_context, read_current_state_with_fallback, read_story_frame, read_volume_map,
};
use crate::utils::pov_filter::{extract_pov_from_outline, filter_hooks_by_pov, filter_matrix_by_pov};
use crate::utils::story_markdown::parse_pending_hooks_markdown;

/// agent 名。对齐 TS `WriterAgent.name`。
pub const WRITER_NAME: &str = "writer";

/// readFileOrDefault 的统一 fallback（对齐 TS 硬编码 "(文件尚未创建)"）。
pub(crate) const MISSING_FILE: &str = "(文件尚未创建)";

/// legacy 上下文预算（字符）。逐字移植 TS `LEGACY_WRITER_CONTEXT_BUDGET`。
mod budget {
    pub const STORY_BIBLE: usize = 14_000;
    pub const CURRENT_STATE: usize = 7_000;
    pub const LEDGER: usize = 6_000;
    pub const HOOKS: usize = 9_000;
    pub const CHAPTER_SUMMARIES: usize = 9_000;
    pub const SUBPLOT_BOARD: usize = 7_000;
    pub const EMOTIONAL_ARCS: usize = 7_000;
    pub const CHARACTER_MATRIX: usize = 12_000;
    pub const PARENT_CANON: usize = 12_000;
    pub const VOLUME_OUTLINE: usize = 12_000;
}

// ---- 输入 / 输出 ----

/// writeChapter 入参。对齐 TS `WriteChapterInput`。
pub struct WriteChapterInput<'a> {
    pub book: &'a BookConfig,
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    pub external_context: Option<&'a str>,
    pub chapter_intent: Option<&'a str>,
    pub chapter_memo: Option<&'a ChapterMemo>,
    pub chapter_intent_data: Option<&'a ChapterIntent>,
    pub context_package: Option<&'a ContextPackage>,
    pub rule_stack: Option<&'a RuleStack>,
    pub length_spec: Option<LengthSpec>,
    pub word_count_override: Option<u32>,
    pub temperature_override: Option<f64>,
}

/// settleChapterState 入参。对齐 TS `SettleChapterStateInput`。
pub struct SettleChapterStateInput<'a> {
    pub book: &'a BookConfig,
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    /// 131 号：快照基准章（TS baselineChapter——修订链重放语义：从章前快照
    /// 的 truth 重新结算；None = 当前 story 目录（写新章语义））。
    pub baseline_chapter: Option<u32>,
    pub title: &'a str,
    pub content: &'a str,
    pub allow_reapply: Option<bool>,
    /// TS `allowNewHooks`（resync 工具「保持稳定 hook id」语义）：false 时
    /// 仲裁器拒绝全部新 hook 候选；None = 默认放行。
    pub allow_new_hooks: Option<bool>,
    pub chapter_intent: Option<&'a str>,
    pub context_package: Option<&'a ContextPackage>,
    pub rule_stack: Option<&'a RuleStack>,
    pub validation_feedback: Option<&'a str>,
}

/// token 用量。对齐 TS `TokenUsage`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// hook 健康问题视图。对齐 TS `WriteChapterOutput.hookHealthIssues` 元素。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct HookHealthIssue {
    pub severity: AuditSeverity,
    pub category: String,
    pub description: String,
    pub suggestion: String,
}

/// writeChapter 出参。对齐 TS `WriteChapterOutput`。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct WriteChapterOutput {
    pub chapter_number: u32,
    pub title: String,
    pub content: String,
    pub word_count: u32,
    pub pre_write_check: String,
    pub post_settlement: String,
    pub runtime_state_delta: Option<RuntimeStateDelta>,
    pub runtime_state_snapshot: Option<RuntimeStateSnapshot>,
    pub updated_state: String,
    pub updated_ledger: String,
    pub updated_hooks: String,
    pub chapter_summary: String,
    pub updated_chapter_summaries: Option<String>,
    pub updated_subplots: String,
    pub updated_emotional_arcs: String,
    pub updated_character_matrix: String,
    pub post_write_errors: Vec<PostWriteViolation>,
    pub post_write_warnings: Vec<PostWriteViolation>,
    pub hook_health_issues: Vec<HookHealthIssue>,
    /// R2/359 号：本章张力评分（TENSION_METRICS 节解析产物；缺省=LLM 未产分）。
    pub tension_metrics: Option<crate::utils::tension_curve::TensionMetrics>,
    pub token_usage: TokenUsage,
}

/// LLM 聊天端口（复用 AuditorChat 模式）。
#[async_trait::async_trait]
pub trait WriterChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
    ) -> Result<ChatOutcome, String>;
}

/// writer 环境依赖（路径与存储注入）。
pub struct WriterCtx<'a> {
    pub project_root: &'a Path,
    pub builtin_genres_dir: &'a Path,
    /// prompt pack 三级加载的存储。
    pub prompt_store: &'a dyn StateStore,
    /// runtime state 快照加载/归约的存储（ FsStateStore 即项目磁盘）。
    pub state_store: &'a dyn StateStore,
}

#[derive(Debug, thiserror::Error)]
pub enum WriteChapterError {
    #[error(transparent)]
    Genre(#[from] crate::agents::rules_reader::ReadGenreProfileError),
    #[error("prompt pack guidance: {0}")]
    PromptPack(#[from] PromptPackPromptNotFoundError),
    #[error("LLM chat failed: {0}")]
    Chat(String),
    #[error(transparent)]
    Engine(#[from] crate::EngineError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    AtomicFileSet(#[from] crate::utils::atomic_file_set::AtomicFileSetError),
}

fn lang_from_str(value: &str) -> WritingLanguage {
    if value == "en" {
        WritingLanguage::En
    } else {
        WritingLanguage::Zh
    }
}

fn log_info(language: WritingLanguage, zh: &str, en: &str) {
    if language == WritingLanguage::En {
        tracing::info!("{en}");
    } else {
        tracing::info!("{zh}");
    }
}

fn log_warn(language: WritingLanguage, zh: &str, en: &str) {
    if language == WritingLanguage::En {
        tracing::warn!("{en}");
    } else {
        tracing::warn!("{zh}");
    }
}

fn usage_or_zero(usage: &Option<AuditTokenUsage>) -> AuditTokenUsage {
    (*usage).unwrap_or(AuditTokenUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    })
}

/// 读文件，失败 → 占位文案。对齐 TS `readFileOrDefault`。
async fn read_file_or_default(path: &Path) -> String {
    tokio::fs::read_to_string(path)
        .await
        .unwrap_or_else(|_| MISSING_FILE.to_string())
}

/// 加载最近章节正文。对齐 TS `loadRecentChapters`：`*.md` 且非 `index*`，
/// 文件名排序取末 count 篇，`\n\n---\n\n` 连接；目录缺失 → 空串。
pub(crate) async fn load_recent_chapters(
    book_dir: &Path,
    current_chapter: u32,
    count: usize,
) -> String {
    let _ = current_chapter;
    let chapters_dir = book_dir.join("chapters");
    let Ok(mut entries) = tokio::fs::read_dir(&chapters_dir).await else {
        return String::new();
    };

    let mut md_files: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") && !name.starts_with("index") {
            md_files.push(name);
        }
    }
    md_files.sort();
    let start = md_files.len().saturating_sub(count);

    let mut contents: Vec<String> = Vec::new();
    for name in &md_files[start..] {
        if let Ok(content) = tokio::fs::read_to_string(chapters_dir.join(name)).await {
            contents.push(content);
        }
    }
    contents.join("\n\n---\n\n")
}

// ---- 纯函数（golden 差分守门） ----

/// legacy user prompt 入参。对齐 TS `buildUserPrompt` 的参数对象。
pub struct UserPromptInput<'a> {
    pub chapter_number: u32,
    pub story_bible: &'a str,
    pub current_state: &'a str,
    pub ledger: &'a str,
    pub hooks: &'a str,
    pub recent_chapters: &'a str,
    pub length_spec: &'a LengthSpec,
    pub external_context: Option<&'a str>,
    pub chapter_summaries: &'a str,
    pub subplot_board: &'a str,
    pub emotional_arcs: &'a str,
    pub character_matrix: &'a str,
    pub dialogue_fingerprints: Option<&'a str>,
    pub relevant_summaries: Option<&'a str>,
    pub parent_canon: Option<&'a str>,
    pub language: Option<WritingLanguage>,
}

fn cap_legacy(label: &str, content: &str, max_chars: usize) -> String {
    cap_context_block(
        content,
        crate::utils::context_filter::ContextCapOptions {
            label,
            max_chars,
            head_ratio: None,
        },
    )
}

/// legacy（非 governed）创作 user prompt。逐字移植 TS `buildUserPrompt`：
/// 各真相块独立预算裁剪 + `!== "(文件尚未创建)"` 门控 + zh/en 双模板。
pub fn build_user_prompt(params: &UserPromptInput<'_>) -> String {
    let current_state = cap_legacy("current_state", params.current_state, budget::CURRENT_STATE);
    let ledger = cap_legacy("particle_ledger", params.ledger, budget::LEDGER);
    let hooks = cap_legacy("pending_hooks", params.hooks, budget::HOOKS);
    let chapter_summaries = cap_legacy(
        "chapter_summaries",
        params.chapter_summaries,
        budget::CHAPTER_SUMMARIES,
    );
    let subplot_board = cap_legacy("subplot_board", params.subplot_board, budget::SUBPLOT_BOARD);
    let emotional_arcs = cap_legacy("emotional_arcs", params.emotional_arcs, budget::EMOTIONAL_ARCS);
    let character_matrix = cap_legacy(
        "character_matrix",
        params.character_matrix,
        budget::CHARACTER_MATRIX,
    );
    let story_bible = cap_legacy("story_bible", params.story_bible, budget::STORY_BIBLE);
    let parent_canon = params
        .parent_canon
        .map(|canon| cap_legacy("parent_canon", canon, budget::PARENT_CANON));

    let context_block = params
        .external_context
        .filter(|ctx| !ctx.is_empty())
        .map(|ctx| format!("\n## 外部指令\n以下是来自外部系统的创作指令，请在本章中融入：\n\n{ctx}\n"))
        .unwrap_or_default();

    let ledger_block = if !ledger.is_empty() {
        format!("\n## 资源账本\n{ledger}\n")
    } else {
        String::new()
    };

    let summaries_block = if chapter_summaries != MISSING_FILE {
        format!("\n## 章节摘要（全部历史章节压缩上下文）\n{chapter_summaries}\n")
    } else {
        String::new()
    };

    let subplot_block = if subplot_board != MISSING_FILE {
        format!("\n## 支线进度板\n{subplot_board}\n")
    } else {
        String::new()
    };

    let emotional_block = if emotional_arcs != MISSING_FILE {
        format!("\n## 情感弧线\n{emotional_arcs}\n")
    } else {
        String::new()
    };

    let matrix_block = if character_matrix != MISSING_FILE {
        format!("\n## 角色交互矩阵\n{character_matrix}\n")
    } else {
        String::new()
    };

    let fingerprint_block = params
        .dialogue_fingerprints
        .filter(|fp| !fp.is_empty())
        .map(|fp| format!("\n## 角色对话指纹\n{fp}\n"))
        .unwrap_or_default();

    let relevant_block = params
        .relevant_summaries
        .filter(|s| !s.is_empty())
        .map(|s| format!("\n## 相关历史章节摘要\n{s}\n"))
        .unwrap_or_default();

    let canon_block = parent_canon
        .as_deref()
        .map(|canon| {
            format!("\n## 正传正典参照（番外写作专用）\n本书是番外作品。以下正典约束不可违反，角色不得引用超出其信息边界的信息。\n{canon}\n")
        })
        .unwrap_or_default();

    let language = params.language.unwrap_or(WritingLanguage::Zh);
    let length_requirement_block =
        build_length_requirement_block(params.length_spec, Some(language));

    if language == WritingLanguage::En {
        let recent = if params.recent_chapters.is_empty() {
            "(This is the first chapter, no previous text)"
        } else {
            params.recent_chapters
        };
        return format!(
            "Write chapter {chapter_number}.\n{context_block}\n## Current State\n{current_state}\n{ledger_block}\n## Plot Threads\n{hooks}\n{summaries_block}{subplot_block}{emotional_block}{matrix_block}{fingerprint_block}{relevant_block}{canon_block}\n## Recent Chapters\n{recent}\n\n## Worldbuilding\n{story_bible}\n\n{length_requirement_block}\n- Output PRE_WRITE_CHECK first, then the chapter\n- Output only PRE_WRITE_CHECK, CHAPTER_TITLE, and CHAPTER_CONTENT blocks",
            chapter_number = params.chapter_number,
        );
    }

    let recent = if params.recent_chapters.is_empty() {
        "(这是第一章，无前文)"
    } else {
        params.recent_chapters
    };
    format!(
        "请续写第{chapter_number}章。\n{context_block}\n## 当前状态卡\n{current_state}\n{ledger_block}\n## 伏笔池\n{hooks}\n{summaries_block}{subplot_block}{emotional_block}{matrix_block}{fingerprint_block}{relevant_block}{canon_block}\n## 最近章节\n{recent}\n\n## 世界观设定\n{story_bible}\n\n{length_requirement_block}\n- 先输出写作自检表，再写正文\n      - 只需输出 PRE_WRITE_CHECK、CHAPTER_TITLE、CHAPTER_CONTENT 三个区块",
        chapter_number = params.chapter_number,
    )
}

/// governed 创作 user prompt 入参。对齐 TS `buildGovernedUserPrompt` 的参数对象。
pub struct GovernedUserPromptInput<'a> {
    pub chapter_number: u32,
    pub chapter_memo: &'a ChapterMemo,
    pub chapter_intent_data: Option<&'a ChapterIntent>,
    pub context_package: &'a ContextPackage,
    pub rule_stack: &'a RuleStack,
    pub external_context: Option<&'a str>,
    pub length_spec: &'a LengthSpec,
    pub language: Option<WritingLanguage>,
    pub variance_brief: Option<&'a str>,
    pub selected_evidence_block: Option<&'a str>,
}

/// 用户方向源（author_intent / current_focus）。对齐 TS `DIRECTION_SOURCES`。
const DIRECTION_SOURCES: [&str; 2] = ["story/author_intent.md", "story/current_focus.md"];

/// governed（planner 治理）创作 user prompt。逐字移植 TS `buildGovernedUserPrompt`：
/// 用户方向块置顶（优先于模型默认）→ memo 叙事块 → 已选上下文 → 规则栈 →
/// 方差简报 → 长度要求。
pub fn build_governed_user_prompt(params: &GovernedUserPromptInput<'_>) -> String {
    let language = params.language.unwrap_or(WritingLanguage::Zh);
    fn to_refs<'a>(
        entries: &[&'a crate::models::input_governance::ContextSource],
    ) -> Vec<crate::utils::narrative_control::ContextSourceRef<'a>> {
        entries
            .iter()
            .map(|entry| crate::utils::narrative_control::ContextSourceRef {
                reason: entry.reason.as_str(),
                excerpt: entry.excerpt.as_deref(),
            })
            .collect()
    }
    let direction_entries: Vec<&crate::models::input_governance::ContextSource> = params
        .context_package
        .selected_context
        .iter()
        .filter(|entry| DIRECTION_SOURCES.contains(&entry.source.as_str()))
        .collect();
    let other_entries: Vec<&crate::models::input_governance::ContextSource> = params
        .context_package
        .selected_context
        .iter()
        .filter(|entry| !DIRECTION_SOURCES.contains(&entry.source.as_str()))
        .collect();

    let context_sections =
        render_narrative_selected_context(&to_refs(&other_entries), language);
    let user_direction_block = if !direction_entries.is_empty() {
        let rendered = render_narrative_selected_context(&to_refs(&direction_entries), language);
        if language == WritingLanguage::En {
            format!("## User direction (overrides model defaults — must follow)\n{rendered}\n")
        } else {
            format!("## 用户方向（优先于模型默认，必须遵循）\n{rendered}\n")
        }
    } else {
        String::new()
    };

    let diagnostic_lines = if params.rule_stack.sections.diagnostic.is_empty() {
        "none".to_string()
    } else {
        params.rule_stack.sections.diagnostic.join(", ")
    };

    let length_requirement_block =
        build_length_requirement_block(params.length_spec, Some(language));
    let variance_block = params
        .variance_brief
        .filter(|brief| !brief.is_empty())
        .map(|brief| format!("\n{brief}\n"))
        .unwrap_or_default();
    let selected_evidence_block = sanitize_narrative_evidence_block(
        params.selected_evidence_block,
        language,
    )
    .map(|block| format!("\n{block}\n"))
    .unwrap_or_default();
    let chapter_context_block =
        build_chapter_context_block(params.external_context, language);
    let brief_narrative = render_memo_as_narrative_block(
        params.chapter_memo,
        params
            .chapter_intent_data
            .and_then(|intent| intent.arc_context.as_deref()),
        language,
    );

    let hard_join = |joiner: &str, fallback: &str, values: &[String]| -> String {
        let joined = values.join(joiner);
        if joined.is_empty() {
            fallback.to_string()
        } else {
            joined
        }
    };

    if language == WritingLanguage::En {
        let context_sections = if context_sections.is_empty() {
            "(none)".to_string()
        } else {
            context_sections
        };
        return format!(
            "Write chapter {chapter_number}.\n\n{chapter_context_block}\n\n{user_direction_block}\n{brief_narrative}\n\n## Selected Context\n{context_sections}\n{selected_evidence_block}\n\n## Rule Stack\n- Hard: {hard}\n- Soft: {soft}\n- Diagnostic: {diagnostic}\n\n{variance_block}\n{length_requirement_block}\n- Output PRE_WRITE_CHECK first, then the chapter\n- Output only PRE_WRITE_CHECK, CHAPTER_TITLE, and CHAPTER_CONTENT blocks",
            chapter_number = params.chapter_number,
            hard = hard_join(", ", "(none)", &params.rule_stack.sections.hard),
            soft = hard_join(", ", "(none)", &params.rule_stack.sections.soft),
            diagnostic = diagnostic_lines,
        );
    }

    let context_sections = if context_sections.is_empty() {
        "(无)".to_string()
    } else {
        context_sections
    };
    format!(
        "请续写第{chapter_number}章。\n\n{chapter_context_block}\n\n{user_direction_block}\n{brief_narrative}\n\n## 已选上下文\n{context_sections}\n{selected_evidence_block}\n\n## 规则栈\n- 硬护栏：{hard}\n- 软约束：{soft}\n- 诊断规则：{diagnostic}\n\n{variance_block}\n{length_requirement_block}\n- 先输出写作自检表，再写正文\n- 只需输出 PRE_WRITE_CHECK、CHAPTER_TITLE、CHAPTER_CONTENT 三个区块",
        chapter_number = params.chapter_number,
        hard = hard_join("、", "(无)", &params.rule_stack.sections.hard),
        soft = hard_join("、", "(无)", &params.rule_stack.sections.soft),
        diagnostic = diagnostic_lines,
    )
}

/// 本章用户指令块。对齐 TS `buildChapterContextBlock`（trim 后为空 → 空串）。
pub fn build_chapter_context_block(
    external_context: Option<&str>,
    language: WritingLanguage,
) -> String {
    let Some(trimmed) = external_context.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    if language == WritingLanguage::En {
        format!(
            "## Per-chapter user instruction (highest priority)\n{trimmed}\n\nObey this direct instruction for the current chapter. If it specifies a chapter title, use that title exactly in CHAPTER_TITLE. Keep continuity, but do not replace this instruction with the outline fallback."
        )
    } else {
        format!(
            "## 本章用户指令（最高优先级）\n{trimmed}\n\n这是用户对当前章节的直接指令。若其中指定章节标题，CHAPTER_TITLE 必须原样使用该标题。保持连续性，但不要用卷纲兜底替换这条指令。"
        )
    }
}

/// 证据块拼接。对齐 TS `joinGovernedEvidenceBlocks`：固定顺序过滤空块、
/// `\n` 连接，全空 → None。
pub fn join_governed_evidence_blocks(
    blocks: Option<&GovernedMemoryEvidenceBlocks>,
) -> Option<String> {
    let blocks = blocks?;
    let ordered: [Option<&String>; 7] = [
        blocks.title_history_block.as_ref(),
        blocks.mood_trail_block.as_ref(),
        blocks.canon_block.as_ref(),
        blocks.hook_debt_block.as_ref(),
        blocks.hooks_block.as_ref(),
        blocks.summaries_block.as_ref(),
        blocks.volume_summaries_block.as_ref(),
    ];
    let joined: Vec<&str> = ordered
        .into_iter()
        .flatten()
        .map(|block| block.as_str())
        .filter(|block| !block.is_empty())
        .collect();
    if joined.is_empty() {
        None
    } else {
        Some(joined.join("\n"))
    }
}

/// settler 治理控制块。逐字移植 TS `buildSettlerGovernedControlBlock`：
/// 本章控制输入 + 已选上下文（`^### ` 降级为 `- `）+ 规则栈 + 覆盖边。
pub fn build_settler_governed_control_block(
    chapter_intent: &str,
    context_package: &ContextPackage,
    rule_stack: &RuleStack,
    language: WritingLanguage,
) -> String {
    let selected_context = demote_section_headings(&render_narrative_selected_context(
        &context_package
            .selected_context
            .iter()
            .map(|entry| crate::utils::narrative_control::ContextSourceRef {
                reason: entry.reason.as_str(),
                excerpt: entry.excerpt.as_deref(),
            })
            .collect::<Vec<_>>(),
        language,
    ));
    let overrides = if rule_stack.active_overrides.is_empty() {
        "- none".to_string()
    } else {
        rule_stack
            .active_overrides
            .iter()
            .map(|override_rule| {
                format!(
                    "- {} -> {}: {} ({})",
                    override_rule.from, override_rule.to, override_rule.reason, override_rule.target
                )
            })
            .collect::<Vec<String>>()
            .join("\n")
    };
    let narrative_intent = build_narrative_intent_brief(chapter_intent, language);

    let joiner = |values: &[String], join: &str, fallback: &str| -> String {
        let joined = values.join(join);
        if joined.is_empty() {
            fallback.to_string()
        } else {
            joined
        }
    };

    if language == WritingLanguage::En {
        let intent = if narrative_intent.is_empty() {
            "(none)".to_string()
        } else {
            narrative_intent
        };
        let context = if selected_context.is_empty() {
            "- none".to_string()
        } else {
            selected_context
        };
        return format!(
            "\n## Chapter Control Inputs\n{intent}\n\n### Selected Context\n{context}\n\n### Rule Stack\n- Hard guardrails: {hard}\n- Soft constraints: {soft}\n- Diagnostic rules: {diagnostic}\n\n### Active Overrides\n{overrides}\n",
            hard = joiner(&rule_stack.sections.hard, ", ", "(none)"),
            soft = joiner(&rule_stack.sections.soft, ", ", "(none)"),
            diagnostic = joiner(&rule_stack.sections.diagnostic, ", ", "(none)"),
        );
    }

    let intent = if narrative_intent.is_empty() {
        "(无)".to_string()
    } else {
        narrative_intent
    };
    let context = if selected_context.is_empty() {
        "- none".to_string()
    } else {
        selected_context
    };
    format!(
        "\n## 本章控制输入\n{intent}\n\n### 已选上下文\n{context}\n\n### 规则栈\n- 硬护栏：{hard}\n- 软约束：{soft}\n- 诊断规则：{diagnostic}\n\n### 当前覆盖\n{overrides}\n",
        hard = joiner(&rule_stack.sections.hard, "、", "(无)"),
        soft = joiner(&rule_stack.sections.soft, "、", "(无)"),
        diagnostic = joiner(&rule_stack.sections.diagnostic, "、", "(无)"),
    )
}

/// `^### ` 行首降级 `- `。对齐 TS `.replace(/^### /gm, "- ")`。
fn demote_section_headings(content: &str) -> String {
    heading_demote_re()
        .replace_all(content, "- ")
        .into_owned()
}

fn heading_demote_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^### ").expect("heading demote regex"))
}

/// 软校验 PRE_WRITE_CHECK 是否覆盖 memo 三要素。对齐 TS
/// `verifyPreWriteCheckAlignsWithMemo`——返回告警文案（0-1 条），非硬门禁。
pub fn verify_pre_write_check_aligns_with_memo(
    pre_write_check: &str,
    chapter_number: u32,
    language: WritingLanguage,
) -> Option<String> {
    if pre_write_check.trim().is_empty() {
        return Some(if language == WritingLanguage::En {
            format!("Chapter {chapter_number} PRE_WRITE_CHECK is empty; cannot verify memo alignment")
        } else {
            format!("第{chapter_number}章 PRE_WRITE_CHECK 为空，无法对齐 chapter_memo")
        });
    }

    let required: [(&str, &str); 3] = if language == WritingLanguage::En {
        [("Current task", "Current task"), ("Do not", "Do not"), ("end-of-chapter", "Required end-of-chapter change")]
    } else {
        [("当前任务", "当前任务"), ("不要做", "不要做"), ("章尾", "章尾必须发生的改变")]
    };
    let missing: Vec<&str> = required
        .iter()
        .filter(|(needle, _)| !pre_write_check.contains(needle))
        .map(|(_, label)| *label)
        .collect();

    if missing.is_empty() {
        None
    } else if language == WritingLanguage::En {
        Some(format!(
            "Chapter {chapter_number} PRE_WRITE_CHECK missing memo sections: {}",
            missing.join(", ")
        ))
    } else {
        Some(format!(
            "第{chapter_number}章 PRE_WRITE_CHECK 缺少 memo 章节检查：{}",
            missing.join("、")
        ))
    }
}

/// 长度要求块。对齐 TS `buildLengthRequirementBlock`。
pub fn build_length_requirement_block(
    length_spec: &LengthSpec,
    language: Option<WritingLanguage>,
) -> String {
    if language == Some(WritingLanguage::En) {
        format!(
            "Requirements:\n- Target length: {} words\n- Acceptable range: {}-{} words",
            length_spec.target, length_spec.soft_min, length_spec.soft_max
        )
    } else {
        format!(
            "要求：\n- 目标字数：{}字\n- 允许区间：{}-{}字",
            length_spec.target, length_spec.soft_min, length_spec.soft_max
        )
    }
}

/// delta 摘要行渲染。对齐 TS `renderDeltaSummaryRow`：8 列 `|` 转义 + trim，
/// 空 chapterSummary → 空串。
pub fn render_delta_summary_row(delta: &RuntimeStateDelta) -> String {
    let Some(summary) = &delta.chapter_summary else {
        return String::new();
    };
    let row = [
        summary.chapter.to_string(),
        summary.title.clone(),
        summary.characters.clone(),
        summary.events.clone(),
        summary.state_changes.clone(),
        summary.hook_activity.clone(),
        summary.mood.clone(),
        summary.chapter_type.clone(),
    ]
    .iter()
    .map(|value| value.replace('|', "\\|").trim().to_string())
    .collect::<Vec<String>>()
    .join(" | ");

    format!("| {row} |")
}

/// 归一 delta 的章节标注（hook 起始/最近推进/摘要行章节均不超过权威章节号）。
/// 逐字移植 TS `normalizeRuntimeStateDeltaChapter`；无变化时返回原值相等副本。
pub fn normalize_runtime_state_delta_chapter(
    delta: &RuntimeStateDelta,
    authoritative_chapter_number: u32,
) -> RuntimeStateDelta {
    let mut hook_ops: HookOps = delta.hook_ops.clone();
    let mut changed = delta.chapter != authoritative_chapter_number;
    let normalized_upserts: Vec<crate::models::runtime_state::HookRecord> = hook_ops
        .upsert
        .iter()
        .map(|hook| {
            let start_chapter = hook.start_chapter.min(authoritative_chapter_number);
            let last_advanced_chapter = hook
                .last_advanced_chapter
                .min(authoritative_chapter_number);
            if start_chapter != hook.start_chapter
                || last_advanced_chapter != hook.last_advanced_chapter
            {
                changed = true;
            }
            if start_chapter == hook.start_chapter
                && last_advanced_chapter == hook.last_advanced_chapter
            {
                return hook.clone();
            }
            let mut normalized = hook.clone();
            normalized.start_chapter = start_chapter;
            normalized.last_advanced_chapter = last_advanced_chapter;
            normalized
        })
        .collect();
    hook_ops.upsert = normalized_upserts;

    if let Some(summary) = &delta.chapter_summary {
        if summary.chapter != authoritative_chapter_number {
            changed = true;
        }
    }
    if !changed {
        return delta.clone();
    }

    let mut normalized = delta.clone();
    normalized.chapter = authoritative_chapter_number;
    normalized.hook_ops = hook_ops;
    if let Some(summary) = &mut normalized.chapter_summary {
        summary.chapter = authoritative_chapter_number;
    }
    normalized
}

/// 风格指纹提取。对齐 TS `buildStyleFingerprint`：宽容 JSON 解析（失败 →
/// None），JS truthiness 门控（0/NaN/空 → 跳过）。
pub fn build_style_fingerprint(style_profile_raw: &str) -> Option<String> {
    if style_profile_raw.is_empty() || style_profile_raw == MISSING_FILE {
        return None;
    }
    let Ok(profile) = serde_json::from_str::<serde_json::Value>(style_profile_raw) else {
        return None;
    };

    let truthy_number = |value: &serde_json::Value| -> Option<f64> {
        let number = value.as_f64()?;
        if number != 0.0 && !number.is_nan() {
            Some(number)
        } else {
            None
        }
    };
    let format_number = |value: f64| -> String {
        if value.fract() == 0.0 {
            format!("{}", value as i64)
        } else {
            format!("{value}")
        }
    };

    let mut lines: Vec<String> = Vec::new();
    if let Some(v) = profile.get("avgSentenceLength").and_then(truthy_number) {
        lines.push(format!("- 平均句长：{}字", format_number(v)));
    }
    if let Some(v) = profile.get("sentenceLengthStdDev").and_then(truthy_number) {
        lines.push(format!("- 句长标准差：{}", format_number(v)));
    }
    if let Some(v) = profile.get("avgParagraphLength").and_then(truthy_number) {
        lines.push(format!("- 平均段落长度：{}字", format_number(v)));
    }
    if let Some(range) = profile.get("paragraphLengthRange") {
        let min = range.get("min").and_then(truthy_number);
        let max = range.get("max").and_then(truthy_number);
        if let (Some(min), Some(max)) = (min, max) {
            lines.push(format!(
                "- 段落长度范围：{}-{}字",
                format_number(min),
                format_number(max)
            ));
        }
    }
    if let Some(v) = profile.get("vocabularyDiversity").and_then(truthy_number) {
        lines.push(format!("- 词汇多样性(TTR)：{}", format_number(v)));
    }
    let top_patterns = profile
        .get("topPatterns")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<&str>>()
        });
    if let Some(patterns) = top_patterns.filter(|p| !p.is_empty()) {
        lines.push(format!("- 高频句式：{}", patterns.join("、")));
    }
    let rhetorical = profile
        .get("rhetoricalFeatures")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<&str>>()
        });
    if let Some(features) = rhetorical.filter(|f| !f.is_empty()) {
        lines.push(format!("- 修辞特征：{}", features.join("、")));
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn dialogue_re() -> &'static Regex {
    // 逐字移植 TS dialogueRegex 的三分支结构（外层 (?:...) 只包分支 1）。
    // 引号类为半角直引号 " 与角引号 「」——TS 源正则不含弯引号（U+201C/D）。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r#"(?:(.{1,6})(?:说道|道|喝道|冷声道|笑道|怒道|低声道|大声道|喝骂道|冷笑道|沉声道|喊道|叫道|问道|答道)\s*[：:]\s*["「]([^"」]+)["」])|["「]([^"」]{2,})["」]|"([^"]{2,})""#,
        )
        .expect("dialogue regex")
    })
}

/// 角色对话指纹提取。对齐 TS `extractDialogueFingerprints`：≥2 条对白的角色
/// 生成「短句/长句 + 反问多 + 常用双字」指纹，`；` 连接。
pub fn extract_dialogue_fingerprints(recent_chapters: &str) -> String {
    if recent_chapters.is_empty() {
        return String::new();
    }

    // 说话人 → 台词列表（保持首次出现顺序，对齐 JS Map 迭代序）。
    let mut character_dialogues: Vec<(String, Vec<String>)> = Vec::new();
    let mut character_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for captures in dialogue_re().captures_iter(recent_chapters) {
        let speaker = captures.get(1).map(|m| m.as_str().trim().to_string());
        let line = captures
            .get(2)
            .or(captures.get(3))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        if let Some(speaker) = speaker {
            if !speaker.is_empty() && utf16_len(&line) > 1 {
                let index = *character_index
                    .entry(speaker.clone())
                    .or_insert_with(|| {
                        character_dialogues.push((speaker.clone(), Vec::new()));
                        character_dialogues.len() - 1
                    });
                character_dialogues[index].1.push(line);
            }
        }
    }

    let mut fingerprints: Vec<String> = Vec::new();
    for (_character, lines) in &character_dialogues {
        if lines.len() < 2 {
            continue;
        }

        let total: usize = lines.iter().map(|line| utf16_len(line)).sum();
        let avg_len = (total as f64 / lines.len() as f64).round();
        let is_short = avg_len < 15.0;

        // 双字 bigram 频次（UTF-16 切分，对齐 JS slice(i, i+2)）。
        let mut word_counts: Vec<(String, u32)> = Vec::new();
        let mut word_index: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for line in lines {
            let units: Vec<u16> = line.encode_utf16().collect();
            if units.len() < 2 {
                continue;
            }
            for window in units.windows(2) {
                let bigram = String::from_utf16_lossy(window);
                let index = *word_index.entry(bigram.clone()).or_insert_with(|| {
                    word_counts.push((bigram.clone(), 0));
                    word_counts.len() - 1
                });
                word_counts[index].1 += 1;
            }
        }
        let mut frequent: Vec<(String, u32)> = word_counts
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        frequent.sort_by(|(_, left), (_, right)| right.cmp(left));
        let frequent_words: Vec<String> = frequent
            .into_iter()
            .take(3)
            .map(|(word, _)| format!("「{word}」"))
            .collect();

        let mut markers: Vec<&str> = Vec::new();
        if is_short {
            markers.push("短句为主");
        } else {
            markers.push("长句为主");
        }

        let question_count = lines
            .iter()
            .filter(|line| line.contains('？') || line.contains('?'))
            .count();
        if question_count as f64 > lines.len() as f64 * 0.3 {
            markers.push("反问多");
        }

        let mut fingerprint = markers.join("，");
        if !frequent_words.is_empty() {
            fingerprint.push_str(&format!("，常用{}", frequent_words.join("")));
        }
        fingerprints.push(format!("{_character}：{fingerprint}"));
    }

    if fingerprints.is_empty() {
        String::new()
    } else {
        fingerprints.join("；")
    }
}

/// 卷纲中的 CJK 名字提取。对齐 TS `/[\u4e00-\u9fff]{2,4}(?=[，、。：]|$)/g`：
/// 2-4 字 CJK 前缀、后随分隔标点或串尾；贪心长度、非重叠推进。
fn extract_outline_names(text: &str) -> HashSet<String> {
    let chars: Vec<char> = text.chars().collect();
    let is_cjk = |c: char| ('\u{4e00}'..='\u{9fff}').contains(&c);
    let is_delim = |c: char| matches!(c, '，' | '、' | '。' | '：');

    let mut names: HashSet<String> = HashSet::new();
    let mut i = 0usize;
    while i < chars.len() {
        if !is_cjk(chars[i]) {
            i += 1;
            continue;
        }
        let mut matched = false;
        for length in (2..=4).rev() {
            if i + length > chars.len() {
                continue;
            }
            if (i..i + length).all(|k| is_cjk(chars[k])) {
                let after = i + length;
                if after == chars.len() || is_delim(chars[after]) {
                    names.insert(chars[i..after].iter().collect());
                    i = after;
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            i += 1;
        }
    }
    names
}

fn chapter_num_in_row_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\|\s*(\d+)\s*\|").expect("chapter num regex"))
}

fn hook_id_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"H\d{2,}").expect("hook id regex"))
}

/// 相关历史摘要筛选。对齐 TS `findRelevantSummaries`：卷纲提取 CJK 名 +
/// hook ID → 匹配摘要行（跳过最近两章）→ 行连接。
pub fn find_relevant_summaries(
    chapter_summaries: &str,
    volume_outline: &str,
    chapter_number: u32,
) -> String {
    if chapter_summaries.is_empty() || chapter_summaries == MISSING_FILE {
        return String::new();
    }
    if volume_outline.is_empty() || volume_outline == MISSING_FILE {
        return String::new();
    }

    let outline_names = extract_outline_names(volume_outline);
    let hook_ids: HashSet<String> = hook_id_re()
        .find_iter(volume_outline)
        .map(|m| m.as_str().to_string())
        .collect();

    if outline_names.is_empty() && hook_ids.is_empty() {
        return String::new();
    }

    let rows: Vec<&str> = chapter_summaries
        .split('\n')
        .filter(|line| {
            line.starts_with('|')
                && !line.starts_with("| 章节")
                && !line.starts_with("|--")
                && !line.starts_with("| -")
        })
        .collect();

    let matched_rows: Vec<&str> = rows
        .iter()
        .copied()
        .filter(|row| {
            outline_names
                .iter()
                .any(|name| row.contains(name.as_str()))
                || hook_ids.iter().any(|hook_id| row.contains(hook_id.as_str()))
        })
        .collect();

    let filtered_rows: Vec<&str> = matched_rows
        .iter()
        .copied()
        .filter(|row| {
            let Some(found) = chapter_num_in_row_re().find(row) else {
                return true;
            };
            let num: i64 = found
                .as_str()
                .trim_matches('|')
                .trim()
                .parse::<i64>()
                .unwrap_or(i64::MAX);
            num < chapter_number as i64 - 1
        })
        .collect();

    if filtered_rows.is_empty() {
        String::new()
    } else {
        filtered_rows.join("\n")
    }
}

/// 文件名净化。对齐 TS `sanitizeFilename`：去非法字符、空白折叠 `_`、
/// 50 UTF-16 单位截断。
pub fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | '?' | '%' | '*' | ':' | '|' | '"' | '<' | '>'))
        .collect();
    let underscored = whitespace_run_re().replace_all(&cleaned, "_").into_owned();
    let truncated_units: Vec<u16> = underscored.encode_utf16().take(50).collect();
    String::from_utf16_lossy(&truncated_units)
}

fn whitespace_run_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\s+").expect("whitespace run regex"))
}

// ---- Phase 2 内部：结算 ----

/// 结算中间产物（delta 或 legacy 合并路径）。
struct MergedSettlement {
    post_settlement: String,
    runtime_state_delta: Option<RuntimeStateDelta>,
    updated_state: String,
    updated_ledger: String,
    updated_hooks: String,
    chapter_summary: String,
    updated_subplots: String,
    updated_emotional_arcs: String,
    updated_character_matrix: String,
    /// R2/359 号：本章张力评分（TENSION_METRICS 节解析产物；缺省=LLM 未产分）。
    tension_metrics: Option<crate::utils::tension_curve::TensionMetrics>,
}

struct SettleParams<'a> {
    book: &'a BookConfig,
    genre_profile: &'a GenreProfile,
    book_rules: Option<&'a BookRules>,
    chapter_number: u32,
    title: &'a str,
    content: &'a str,
    current_state: &'a str,
    ledger: &'a str,
    hooks: &'a str,
    chapter_summaries: &'a str,
    subplot_board: &'a str,
    emotional_arcs: &'a str,
    character_matrix: &'a str,
    volume_outline: &'a str,
    selected_evidence_block: Option<&'a str>,
    chapter_intent: Option<&'a str>,
    context_package: Option<&'a ContextPackage>,
    rule_stack: Option<&'a RuleStack>,
    validation_feedback: Option<&'a str>,
    original_hooks: &'a str,
    original_subplots: &'a str,
    original_emotional_arcs: &'a str,
    original_character_matrix: &'a str,
}

/// Phase 2 结算（observer → settler）。逐字移植 TS `settle`：
/// 2a 观察（temp 0.5）→ 2b 回写（temp 0.3）→ delta 解析优先，legacy 解析
/// + governed 表格合并回退。
async fn settle(
    chat: &dyn WriterChat,
    params: &SettleParams<'_>,
) -> Result<(MergedSettlement, AuditTokenUsage), WriteChapterError> {
    let resolved_language =
        lang_from_str(params.book.language.as_deref().unwrap_or(&params.genre_profile.language));

    // Phase 2a: Observer — 提取本章全部事实。
    let observer_system = build_observer_system_prompt(
        params.book,
        params.genre_profile,
        Some(resolved_language),
    );
    let observer_user = build_observer_user_prompt(
        params.chapter_number,
        params.title,
        params.content,
        Some(resolved_language),
    );

    log_info(
        resolved_language,
        &format!("阶段 2a：提取第{}章事实", params.chapter_number),
        &format!("Phase 2a: observing facts for chapter {}", params.chapter_number),
    );
    let observer_response = chat
        .chat(
            vec![
                LLMMessage {
                    role: LLMRole::System,
                    content: observer_system,
                    tool_calls: None, tool_call_id: None,
                },
                LLMMessage {
                    role: LLMRole::User,
                    content: observer_user,
                    tool_calls: None, tool_call_id: None,
                },
            ],
            0.5,
        )
        .await
        .map_err(WriteChapterError::Chat)?;
    let observations = observer_response.content;

    // Phase 2b: settler — 把观察回写真相文件。
    log_info(
        resolved_language,
        "阶段 2b：把观察结果回写到真相文件",
        "Phase 2b: reflecting observations into truth files",
    );
    let settler_system = build_settler_system_prompt(
        params.book,
        params.genre_profile,
        params.book_rules,
        Some(resolved_language),
    );
    let governed_control_block = match (params.chapter_intent, params.context_package, params.rule_stack) {
        (Some(intent), Some(package), Some(stack))
            if !intent.trim().is_empty() =>
        {
            Some(build_settler_governed_control_block(
                intent,
                package,
                stack,
                resolved_language,
            ))
        }
        _ => None,
    };

    let settler_user = build_settler_user_prompt(&SettlerUserPromptInput {
        chapter_number: params.chapter_number,
        title: params.title,
        content: params.content,
        current_state: &cap_legacy("current_state", params.current_state, budget::CURRENT_STATE),
        ledger: &cap_legacy("particle_ledger", params.ledger, budget::LEDGER),
        hooks: &cap_legacy("pending_hooks", params.hooks, budget::HOOKS),
        chapter_summaries: &cap_legacy(
            "chapter_summaries",
            params.chapter_summaries,
            budget::CHAPTER_SUMMARIES,
        ),
        subplot_board: &cap_legacy("subplot_board", params.subplot_board, budget::SUBPLOT_BOARD),
        emotional_arcs: &cap_legacy("emotional_arcs", params.emotional_arcs, budget::EMOTIONAL_ARCS),
        character_matrix: &cap_legacy(
            "character_matrix",
            params.character_matrix,
            budget::CHARACTER_MATRIX,
        ),
        volume_outline: &cap_legacy("volume_outline", params.volume_outline, budget::VOLUME_OUTLINE),
        observations: Some(&observations),
        selected_evidence_block: params.selected_evidence_block,
        governed_control_block: governed_control_block.as_deref(),
        validation_feedback: params.validation_feedback,
    });

    let response = chat
        .chat(
            vec![
                LLMMessage {
                    role: LLMRole::System,
                    content: settler_system,
                    tool_calls: None, tool_call_id: None,
                },
                LLMMessage {
                    role: LLMRole::User,
                    content: settler_user,
                    tool_calls: None, tool_call_id: None,
                },
            ],
            0.3,
        )
        .await
        .map_err(WriteChapterError::Chat)?;

    // delta 输出优先；解析失败回退 legacy 全量输出（governed 时表格合并）。
    // R2/359 号：TENSION_METRICS 节一次性解析（delta/fallback 两路共用）；
    // 分数由代码注入表行，不进 LLM 的 delta JSON 模板。
    let tension_metrics = crate::utils::tension_curve::parse_tension_metrics(&response.content);
    let merged: MergedSettlement = match parse_settler_delta_output(&response.content) {
        Ok(delta_output) => {
            let mut runtime_state_delta = delta_output.runtime_state_delta;
            if let (Some(metrics), Some(summary)) = (
                tension_metrics.as_ref(),
                runtime_state_delta.chapter_summary.as_mut(),
            ) {
                summary.conflict_level = Some(metrics.conflict_level as u32);
                summary.reveal_level = Some(metrics.reveal_level as u32);
            }
            MergedSettlement {
                post_settlement: delta_output.post_settlement,
                runtime_state_delta: Some(runtime_state_delta),
                updated_state: String::new(),
                updated_ledger: String::new(),
                updated_hooks: String::new(),
                chapter_summary: String::new(),
                updated_subplots: String::new(),
                updated_emotional_arcs: String::new(),
                updated_character_matrix: String::new(),
                tension_metrics,
            }
        }
        Err(_) => {
            let settlement: SettlementOutput =
                parse_settlement_output(&response.content, params.genre_profile);
            if governed_control_block.is_some() {
                MergedSettlement {
                    post_settlement: settlement.post_settlement,
                    runtime_state_delta: None,
                    updated_state: settlement.updated_state,
                    updated_ledger: settlement.updated_ledger,
                    updated_hooks: merge_table_markdown_by_key(
                        params.original_hooks,
                        &settlement.updated_hooks,
                        &[0],
                    ),
                    chapter_summary: settlement.chapter_summary,
                    updated_subplots: if settlement.updated_subplots.is_empty() {
                        settlement.updated_subplots.clone()
                    } else {
                        merge_table_markdown_by_key(
                            params.original_subplots,
                            &settlement.updated_subplots,
                            &[0],
                        )
                    },
                    updated_emotional_arcs: if settlement.updated_emotional_arcs.is_empty() {
                        settlement.updated_emotional_arcs.clone()
                    } else {
                        merge_table_markdown_by_key(
                            params.original_emotional_arcs,
                            &settlement.updated_emotional_arcs,
                            &[0, 1],
                        )
                    },
                    updated_character_matrix: if settlement.updated_character_matrix.is_empty() {
                        settlement.updated_character_matrix.clone()
                    } else {
                        merge_character_matrix_markdown(
                            params.original_character_matrix,
                            &settlement.updated_character_matrix,
                        )
                    },
                    tension_metrics,
                }
            } else {
                MergedSettlement {
                    post_settlement: settlement.post_settlement,
                    runtime_state_delta: None,
                    updated_state: settlement.updated_state,
                    updated_ledger: settlement.updated_ledger,
                    updated_hooks: settlement.updated_hooks,
                    chapter_summary: settlement.chapter_summary,
                    updated_subplots: settlement.updated_subplots,
                    updated_emotional_arcs: settlement.updated_emotional_arcs,
                    updated_character_matrix: settlement.updated_character_matrix,
                    tension_metrics,
                }
            }
        }
    };

    Ok((merged, usage_or_zero(&response.usage)))
}

// ---- runtime state artifacts 辅助 ----

/// 对齐 TS `buildRuntimeStateArtifactsIfPresent`：无 delta → None；有权威
/// 章节号先归一再构建。
#[allow(clippy::too_many_arguments)]
async fn build_runtime_state_artifacts_if_present(
    state_store: &dyn StateStore,
    book_dir: &str,
    delta: Option<&RuntimeStateDelta>,
    language: WritingLanguage,
    authoritative_chapter_number: Option<u32>,
    allow_reapply: Option<bool>,
    allow_new_hooks: Option<bool>,
    baseline_chapter: Option<u32>,
) -> crate::Result<Option<RuntimeStateArtifacts>> {
    let Some(delta) = delta else {
        return Ok(None);
    };
    let safe_delta = match authoritative_chapter_number {
        Some(authority) => normalize_runtime_state_delta_chapter(delta, authority),
        None => delta.clone(),
    };
    // 131 号：修订链重放语义——有基准章时从章前快照归约（TS
    // buildRuntimeStateArtifactsIfPresent 的 baselineChapter 分支）。
    match baseline_chapter {
        Some(baseline) => {
            let snapshot = crate::state::runtime_state_store::load_runtime_state_snapshot_at_chapter(
                state_store,
                book_dir,
                baseline,
                language,
            )
            .await?;
            crate::state::runtime_state_store::build_runtime_state_artifacts_from_snapshot(
                &snapshot,
                &safe_delta,
                language,
                allow_reapply,
                allow_new_hooks,
            )
            .await
            .map(Some)
        }
        None => {
            build_runtime_state_artifacts(state_store, book_dir, &safe_delta, language, allow_reapply, allow_new_hooks)
                .await
                .map(Some)
        }
    }
}

/// 对齐 TS `resolveRuntimeStateArtifactsForOutput`：输出已带完整 artifacts
/// （快照 + 三投影且 delta 无需归一）时复用，否则按 delta 重建。
async fn resolve_runtime_state_artifacts_for_output(
    state_store: &dyn StateStore,
    book_dir: &str,
    output: &WriteChapterOutput,
    language: WritingLanguage,
) -> crate::Result<Option<RuntimeStateArtifacts>> {
    let Some(delta) = &output.runtime_state_delta else {
        return Ok(None);
    };
    let safe_delta = normalize_runtime_state_delta_chapter(delta, output.chapter_number);
    if safe_delta == *delta
        && output.runtime_state_snapshot.is_some()
        && output.updated_chapter_summaries.is_some()
        && !output.updated_state.is_empty()
        && !output.updated_hooks.is_empty()
    {
        return Ok(Some(RuntimeStateArtifacts {
            snapshot: output.runtime_state_snapshot.clone().expect("已判 Some"),
            resolved_delta: safe_delta,
            current_state_markdown: output.updated_state.clone(),
            hooks_markdown: output.updated_hooks.clone(),
            chapter_summaries_markdown: output
                .updated_chapter_summaries
                .clone()
                .expect("已判 Some"),
        }));
    }

    build_runtime_state_artifacts(state_store, book_dir, &safe_delta, language, None, None)
        .await
        .map(Some)
}

/// 追加章节摘要行。对齐 TS `appendChapterSummary`：提取数据行 → 按章节号
/// 去重已有行 → 追加。
async fn append_chapter_summary(
    story_dir: &Path,
    summary: &str,
    language: WritingLanguage,
    tension_metrics: Option<&crate::utils::tension_curve::TensionMetrics>,
) -> std::io::Result<()> {
    let summary_path = story_dir.join("chapter_summaries.md");
    let existing = match tokio::fs::read_to_string(&summary_path).await {
        Ok(existing) => existing,
        Err(_) => {
            if language == WritingLanguage::En {
                "# Chapter Summaries\n\n| Chapter | Title | Characters | Key Events | State Changes | Hook Activity | Mood | Chapter Type | Conflict | Reveal |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n".to_string()
            } else {
                "# 章节摘要\n\n| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 | 冲突强度 | 揭示强度 |\n|------|------|----------|----------|----------|----------|----------|----------|--------|--------|\n".to_string()
            }
        }
    };

    // R2/359 号：LLM 表行仍按 8 列产出，张力分由代码在行尾补两列。
    let data_rows: Vec<String> = summary
        .split('\n')
        .filter(|line| {
            line.starts_with('|')
                && !line.starts_with("| 章节")
                && !line.starts_with("| Chapter")
                && !line.starts_with("|--")
                && !line.starts_with("| ---")
        })
        .map(|line| match tension_metrics {
            Some(metrics) => format!(
                "{} | {} | {} |",
                line.trim_end().trim_end_matches('|').trim_end(),
                metrics.conflict_level,
                metrics.reveal_level
            ),
            None => line.trim_end().to_string(),
        })
        .collect();

    if data_rows.is_empty() {
        return Ok(());
    }
    // 新行的章节号集合（`split("|")[1]` 且为纯数字）。
    let first_cell = |line: &str| -> Option<String> {
        line.split('|')
            .nth(1)
            .map(|cell| cell.trim().to_string())
    };
    let is_digits = |value: &str| !value.is_empty() && value.chars().all(|c| c.is_ascii_digit());
    let new_chapter_nums: HashSet<String> = data_rows
        .iter()
        .filter_map(|line| first_cell(line))
        .filter(|cell| is_digits(cell))
        .collect();

    let deduped: Vec<&str> = existing
        .split('\n')
        .filter(|line| {
            if !line.starts_with('|') {
                return true;
            }
            match first_cell(line) {
                Some(ch_num) => !new_chapter_nums.contains(&ch_num),
                None => true,
            }
        })
        .collect();

    tokio::fs::write(
        &summary_path,
        format!("{}\n{}\n", deduped.join("\n").trim_end(), data_rows.join("\n")),
    )
    .await
}

// ---- 编排入口 ----

/// 章节创作 + 结算 + 后写校验。逐字移植 TS `writeChapter`。
pub async fn write_chapter(
    ctx: &WriterCtx<'_>,
    chat: &dyn WriterChat,
    input: &WriteChapterInput<'_>,
) -> Result<WriteChapterOutput, WriteChapterError> {
    let book = input.book;
    let book_dir = input.book_dir;
    let chapter_number = input.chapter_number;
    let story_dir = book_dir.join("story");

    // 13 路真相文件加载（Phase 5：current_state 仅种子时派生回退）。
    let story_bible = read_story_frame(book_dir, MISSING_FILE).await;
    let volume_outline = read_volume_map(book_dir, MISSING_FILE).await;
    let style_guide = read_file_or_default(&story_dir.join("style_guide.md")).await;
    let current_state = read_current_state_with_fallback(book_dir, MISSING_FILE).await;
    let ledger = read_file_or_default(&story_dir.join("particle_ledger.md")).await;
    let hooks = read_file_or_default(&story_dir.join("pending_hooks.md")).await;
    let chapter_summaries = read_file_or_default(&story_dir.join("chapter_summaries.md")).await;
    let subplot_board = read_file_or_default(&story_dir.join("subplot_board.md")).await;
    let emotional_arcs = read_file_or_default(&story_dir.join("emotional_arcs.md")).await;
    let character_matrix = read_character_context(book_dir, MISSING_FILE).await;
    let style_profile_raw = read_file_or_default(&story_dir.join("style_profile.json")).await;
    let parent_canon = read_file_or_default(&story_dir.join("parent_canon.md")).await;
    let fanfic_canon_raw = read_file_or_default(&story_dir.join("fanfic_canon.md")).await;

    let recent_chapters = load_recent_chapters(book_dir, chapter_number, 1).await;
    // 对话指纹取更长窗口（voice consistency over longer span）。
    let fingerprint_chapters = load_recent_chapters(book_dir, chapter_number, 5).await;

    let parsed_genre = read_genre_profile(ctx.project_root, &book.genre, ctx.builtin_genres_dir).await?;
    let genre_profile = &parsed_genre.profile;
    let parsed_book_rules = read_book_rules(book_dir).await;
    let book_rules = parsed_book_rules.as_ref().map(|parsed| &parsed.rules);
    let book_rules_body = parsed_book_rules
        .as_ref()
        .map(|parsed| parsed.body.as_str())
        .unwrap_or("");

    let style_fingerprint = build_style_fingerprint(&style_profile_raw);
    let dialogue_fingerprints = extract_dialogue_fingerprints(&fingerprint_chapters);
    let relevant_summaries =
        find_relevant_summaries(&chapter_summaries, &volume_outline, chapter_number);

    let has_parent_canon = parent_canon != MISSING_FILE;
    let has_fanfic_canon = fanfic_canon_raw != MISSING_FILE;
    let resolved_language = lang_from_str(
        book.language
            .as_deref()
            .unwrap_or(&genre_profile.language),
    );
    let target_words = input
        .length_spec
        .as_ref()
        .map(|spec| spec.target)
        .or(input.word_count_override)
        .unwrap_or(book.chapter_word_count);
    let resolved_length_spec = input
        .length_spec
        .clone()
        .unwrap_or_else(|| build_length_spec(target_words, resolved_language));
    let governed_memory_blocks = input
        .context_package
        .map(|package| build_governed_memory_evidence_blocks(package, Some(resolved_language)));
    let english_variance_brief = if resolved_language == WritingLanguage::En {
        build_english_variance_brief(book_dir, chapter_number).await
    } else {
        None
    };

    let fanfic_context: Option<FanficContext> = match (has_fanfic_canon, book_rules) {
        (true, Some(rules)) if rules.fanfic_mode.is_some() => Some(FanficContext {
            fanfic_canon: fanfic_canon_raw,
            fanfic_mode: rules.fanfic_mode.expect("已判 Some"),
            allowed_deviations: rules.allowed_deviations.clone(),
        }),
        _ => None,
    };

    // ── Phase 1: 创作正文（temp 0.7）──
    let creative_system_prompt = append_prompt_pack_guidance(
        ctx.prompt_store,
        &build_writer_system_prompt(&WriterSystemPromptInput {
            book: Some(book),
            genre_profile: Some(genre_profile),
            book_rules,
            book_rules_body,
            genre_body: &parsed_genre.body,
            style_guide: &style_guide,
            style_fingerprint: style_fingerprint.as_deref(),
            chapter_number: Some(chapter_number),
            mode: Some(WriterPromptMode::Creative),
            fanfic_context: fanfic_context.as_ref(),
            language_override: Some(resolved_language),
            input_profile: Some(if input.chapter_memo.is_some() {
                InputProfile::Governed
            } else {
                InputProfile::Legacy
            }),
            length_spec: Some(resolved_length_spec.clone()),
        }),
        &LoadPromptPackPromptInput {
            prompt_id: "longform.writer".to_string(),
            project_root: Some(ctx.project_root.to_string_lossy().into_owned()),
            user_root: None,
        },
    )
    .await?;

    let is_governed_creative = input.chapter_memo.is_some()
        && input.context_package.is_some()
        && input.rule_stack.is_some();
    let creative_user_prompt = if is_governed_creative {
        build_governed_user_prompt(&GovernedUserPromptInput {
            chapter_number,
            chapter_memo: input.chapter_memo.expect("已判 Some"),
            chapter_intent_data: input.chapter_intent_data,
            context_package: input.context_package.expect("已判 Some"),
            rule_stack: input.rule_stack.expect("已判 Some"),
            external_context: input.external_context,
            length_spec: &resolved_length_spec,
            language: Some(resolved_language),
            variance_brief: english_variance_brief.as_ref().map(|brief| brief.text.as_str()),
            selected_evidence_block: join_governed_evidence_blocks(governed_memory_blocks.as_ref())
                .as_deref(),
        })
    } else {
        // 智能上下文裁剪：只注入真相文件的相关切片。
        let filtered_hooks = filter_hooks(&hooks);
        let filtered_summaries = filter_summaries(&chapter_summaries, chapter_number, None);
        let filtered_subplots = filter_subplots(&subplot_board);
        let filtered_arcs = filter_emotional_arcs(&emotional_arcs, chapter_number, None);
        let filtered_matrix = filter_character_matrix(
            &character_matrix,
            &volume_outline,
            book_rules
                .and_then(|rules| rules.protagonist.as_ref())
                .map(|p| p.name.as_str()),
        );

        // POV 感知裁剪：限制到 POV 角色的信息边界内。
        let pov_character = extract_pov_from_outline(&volume_outline, chapter_number);
        let pov_filtered_matrix = match &pov_character {
            Some(pov) => filter_matrix_by_pov(&filtered_matrix, pov),
            None => filtered_matrix,
        };
        let pov_filtered_hooks = match &pov_character {
            Some(pov) => filter_hooks_by_pov(&filtered_hooks, pov, &chapter_summaries),
            None => filtered_hooks,
        };

        build_user_prompt(&UserPromptInput {
            chapter_number,
            story_bible: &story_bible,
            current_state: &current_state,
            ledger: if genre_profile.numerical_system {
                &ledger
            } else {
                ""
            },
            hooks: &pov_filtered_hooks,
            recent_chapters: &recent_chapters,
            length_spec: &resolved_length_spec,
            external_context: input.external_context,
            chapter_summaries: &filtered_summaries,
            subplot_board: &filtered_subplots,
            emotional_arcs: &filtered_arcs,
            character_matrix: &pov_filtered_matrix,
            dialogue_fingerprints: Some(&dialogue_fingerprints),
            relevant_summaries: Some(&relevant_summaries),
            parent_canon: if has_parent_canon {
                Some(&parent_canon)
            } else {
                None
            },
            language: Some(resolved_language),
        })
    };

    let creative_temperature = input.temperature_override.unwrap_or(0.7);

    log_info(
        resolved_language,
        &format!("阶段 1：创作正文（第{chapter_number}章）"),
        &format!("Phase 1: creative writing for chapter {chapter_number}"),
    );

    let creative_response = chat
        .chat(
            vec![
                LLMMessage {
                    role: LLMRole::System,
                    content: creative_system_prompt,
                    tool_calls: None, tool_call_id: None,
                },
                LLMMessage {
                    role: LLMRole::User,
                    content: creative_user_prompt,
                    tool_calls: None, tool_call_id: None,
                },
            ],
            creative_temperature,
        )
        .await
        .map_err(WriteChapterError::Chat)?;
    let creative_usage = usage_or_zero(&creative_response.usage);

    let creative = parse_creative_output(
        chapter_number,
        &creative_response.content,
        resolved_length_spec.counting_mode,
    );

    // Phase 4 软校验：PRE_WRITE_CHECK 对齐 memo（仅告警，不拦截）。
    if input.chapter_memo.is_some() {
        if let Some(warning) = verify_pre_write_check_aligns_with_memo(
            &creative.pre_write_check,
            chapter_number,
            resolved_language,
        ) {
            if resolved_language == WritingLanguage::En {
                tracing::warn!("{warning}");
            } else {
                tracing::warn!("{warning}");
            }
        }
    }

    // ── Phase 2: 状态结算（temp 0.3）──
    log_info(
        resolved_language,
        &format!("阶段 2：状态结算（第{chapter_number}章，{}字）", creative.word_count),
        &format!(
            "Phase 2: state settlement for chapter {chapter_number} ({} words)",
            creative.word_count
        ),
    );

    let is_governed_settlement = input.chapter_intent.is_some_and(|intent| !intent.is_empty())
        && input.context_package.is_some()
        && input.rule_stack.is_some();

    let filtered_hooks_for_settlement = match (
        is_governed_settlement,
        input.context_package,
    ) {
        (true, Some(package)) => build_governed_hook_working_set(&GovernedHookWorkingSetInput {
            hooks_markdown: &hooks,
            context_package: package,
            chapter_intent: input.chapter_intent,
            chapter_number,
            language: resolved_language,
            keep_recent: None,
        }),
        _ => hooks.clone(),
    };
    let filtered_subplots_for_settlement = if is_governed_settlement {
        filter_subplots(&subplot_board)
    } else {
        subplot_board.clone()
    };
    let filtered_arcs_for_settlement = if is_governed_settlement {
        filter_emotional_arcs(&emotional_arcs, chapter_number, None)
    } else {
        emotional_arcs.clone()
    };
    let filtered_matrix_for_settlement = if is_governed_settlement {
        build_governed_character_matrix_working_set(&GovernedMatrixWorkingSetInput {
            matrix_markdown: &character_matrix,
            chapter_intent: input
                .chapter_intent
                .filter(|intent| !intent.is_empty())
                .unwrap_or(&volume_outline),
            context_package: input.context_package.expect("已判 Some"),
            protagonist_name: book_rules
                .and_then(|rules| rules.protagonist.as_ref())
                .map(|p| p.name.as_str()),
        })
    } else {
        character_matrix.clone()
    };

    let (settlement, settle_usage) = {
        let settle_chapter_summaries = if input.context_package.is_some() {
            filter_summaries(&chapter_summaries, chapter_number, None)
        } else {
            chapter_summaries.clone()
        };
        settle(
            chat,
            &SettleParams {
                book,
                genre_profile,
                book_rules,
                chapter_number,
                title: &creative.title,
                content: &creative.content,
                current_state: &current_state,
                ledger: if genre_profile.numerical_system {
                    &ledger
                } else {
                    ""
                },
                hooks: &filtered_hooks_for_settlement,
                chapter_summaries: &settle_chapter_summaries,
                subplot_board: &filtered_subplots_for_settlement,
                emotional_arcs: &filtered_arcs_for_settlement,
                character_matrix: &filtered_matrix_for_settlement,
                volume_outline: &volume_outline,
                selected_evidence_block: join_governed_evidence_blocks(
                    governed_memory_blocks.as_ref(),
                )
                .as_deref(),
                chapter_intent: input.chapter_intent,
                context_package: input.context_package,
                rule_stack: input.rule_stack,
                validation_feedback: None,
                original_hooks: &hooks,
                original_subplots: &subplot_board,
                original_emotional_arcs: &emotional_arcs,
                original_character_matrix: &character_matrix,
            },
        )
        .await?
    };

    let book_dir_str = book_dir.to_string_lossy().into_owned();
    let runtime_state_artifacts = build_runtime_state_artifacts_if_present(
        ctx.state_store,
        &book_dir_str,
        settlement.runtime_state_delta.as_ref(),
        resolved_language,
        Some(chapter_number),
        None,
        None,
        None,
    )
    .await
    .map_err(WriteChapterError::Engine)?;

    let resolved_runtime_state_delta = runtime_state_artifacts
        .as_ref()
        .map(|artifacts| artifacts.resolved_delta.clone())
        .or_else(|| settlement.runtime_state_delta.clone());
    let resolved_snapshot: Option<RuntimeStateSnapshot> = runtime_state_artifacts
        .as_ref()
        .map(|artifacts| artifacts.snapshot.clone());

    let prior_hook_ids: Vec<String> = parse_pending_hooks_markdown(&hooks)
        .into_iter()
        .map(|hook| hook.hook_id)
        .collect();
    let hook_health_issues: Vec<HookHealthIssue> = match (
        &resolved_runtime_state_delta,
        &resolved_snapshot,
    ) {
        (Some(delta), Some(snapshot)) => analyze_hook_health(&crate::utils::hook_health::HookHealthParams {
            language: resolved_language,
            chapter_number,
            target_chapters: Some(book.target_chapters),
            hooks: &snapshot.hooks.hooks,
            delta: Some(delta),
            existing_hook_ids: Some(&prior_hook_ids),
            max_active_hooks: None,
            stale_after_chapters: None,
            no_advance_window: None,
            new_hook_burst_threshold: None,
        })
        .into_iter()
        .map(|issue| HookHealthIssue {
            severity: issue.severity,
            category: issue.category,
            description: issue.description,
            suggestion: issue.suggestion,
        })
        .collect(),
        _ => Vec::new(),
    };

    // ── 后写校验（regex + 规则，零 LLM 成本）──
    let surface_normalized_content =
        normalize_post_write_surface(&creative.content, Some(resolved_language));
    let surface_normalized_word_count = count_chapter_length(
        &surface_normalized_content,
        resolved_length_spec.counting_mode,
    );
    let mut rule_violations =
        validate_post_write(&surface_normalized_content, genre_profile, book_rules, Some(resolved_language));
    rule_violations.extend(detect_cross_chapter_repetition(
        &surface_normalized_content,
        &fingerprint_chapters,
        resolved_language,
    ));
    rule_violations.extend(detect_paragraph_length_drift(
        &surface_normalized_content,
        &fingerprint_chapters,
        resolved_language,
    ));
    let ai_tell_issues = analyze_ai_tells(&surface_normalized_content, resolved_language).issues;

    let post_write_errors: Vec<PostWriteViolation> = rule_violations
        .iter()
        .filter(|v| v.severity == crate::agents::post_write_validator::ViolationSeverity::Error)
        .cloned()
        .collect();
    let post_write_warnings: Vec<PostWriteViolation> = rule_violations
        .iter()
        .filter(|v| v.severity == crate::agents::post_write_validator::ViolationSeverity::Warning)
        .cloned()
        .collect();

    if !rule_violations.is_empty() {
        log_warn(
            resolved_language,
            &format!(
                "后写校验：第{chapter_number}章 {} 个错误，{} 个警告",
                post_write_errors.len(),
                post_write_warnings.len()
            ),
            &format!(
                "Post-write: {} errors, {} warnings in chapter {chapter_number}",
                post_write_errors.len(),
                post_write_warnings.len()
            ),
        );
        for violation in &rule_violations {
            tracing::warn!("[{:?}] {}: {}", violation.severity, violation.rule, violation.description);
        }
    }
    if !ai_tell_issues.is_empty() {
        log_warn(
            resolved_language,
            &format!("AI 味检查：第{chapter_number}章发现 {} 个问题", ai_tell_issues.len()),
            &format!("AI-tell check: {} issues in chapter {chapter_number}", ai_tell_issues.len()),
        );
        for issue in &ai_tell_issues {
            tracing::warn!("[{:?}] {}: {}", issue.severity, issue.category, issue.description);
        }
    }
    if !hook_health_issues.is_empty() {
        log_warn(
            resolved_language,
            &format!("伏笔健康：第{chapter_number}章发现 {} 条警告", hook_health_issues.len()),
            &format!(
                "Hook health: {} warning(s) in chapter {chapter_number}",
                hook_health_issues.len()
            ),
        );
        for issue in &hook_health_issues {
            tracing::warn!("[{:?}] {}: {}", issue.severity, issue.category, issue.description);
        }
    }

    // ── 汇总输出 ──
    let token_usage = TokenUsage {
        prompt_tokens: creative_usage.prompt_tokens + settle_usage.prompt_tokens,
        completion_tokens: creative_usage.completion_tokens + settle_usage.completion_tokens,
        total_tokens: creative_usage.total_tokens + settle_usage.total_tokens,
    };

    Ok(WriteChapterOutput {
        chapter_number,
        title: creative.title,
        content: surface_normalized_content,
        word_count: surface_normalized_word_count,
        pre_write_check: creative.pre_write_check,
        post_settlement: settlement.post_settlement,
        chapter_summary: match &resolved_runtime_state_delta {
            Some(delta) => render_delta_summary_row(delta),
            None => settlement.chapter_summary,
        },
        runtime_state_delta: resolved_runtime_state_delta,
        runtime_state_snapshot: resolved_snapshot,
        updated_state: runtime_state_artifacts
            .as_ref()
            .map(|artifacts| artifacts.current_state_markdown.clone())
            .unwrap_or(settlement.updated_state),
        updated_ledger: settlement.updated_ledger,
        updated_hooks: runtime_state_artifacts
            .as_ref()
            .map(|artifacts| artifacts.hooks_markdown.clone())
            .unwrap_or(settlement.updated_hooks),
        updated_chapter_summaries: runtime_state_artifacts
            .as_ref()
            .map(|artifacts| artifacts.chapter_summaries_markdown.clone()),
        updated_subplots: settlement.updated_subplots,
        updated_emotional_arcs: settlement.updated_emotional_arcs,
        updated_character_matrix: settlement.updated_character_matrix,
        post_write_errors,
        post_write_warnings,
        hook_health_issues,
        tension_metrics: settlement.tension_metrics,
        token_usage,
    })
}

/// 仅结算（已有正文，重结算真相文件）。逐字移植 TS `settleChapterState`。
pub async fn settle_chapter_state(
    ctx: &WriterCtx<'_>,
    chat: &dyn WriterChat,
    input: &SettleChapterStateInput<'_>,
) -> Result<WriteChapterOutput, WriteChapterError> {
    let book = input.book;
    let book_dir = input.book_dir;
    let story_dir = match input.baseline_chapter {
        Some(baseline) => book_dir.join("story").join("snapshots").join(baseline.to_string()),
        None => book_dir.join("story"),
    };

    let current_state = read_current_state_with_fallback(book_dir, MISSING_FILE).await;
    let ledger = read_file_or_default(&story_dir.join("particle_ledger.md")).await;
    let hooks = read_file_or_default(&story_dir.join("pending_hooks.md")).await;
    let chapter_summaries = read_file_or_default(&story_dir.join("chapter_summaries.md")).await;
    let subplot_board = read_file_or_default(&story_dir.join("subplot_board.md")).await;
    let emotional_arcs = read_file_or_default(&story_dir.join("emotional_arcs.md")).await;
    let character_matrix = read_character_context(book_dir, MISSING_FILE).await;
    let volume_outline = read_volume_map(book_dir, MISSING_FILE).await;

    let parsed_genre = read_genre_profile(ctx.project_root, &book.genre, ctx.builtin_genres_dir).await?;
    let genre_profile = &parsed_genre.profile;
    let parsed_book_rules = read_book_rules(book_dir).await;
    let book_rules = parsed_book_rules.as_ref().map(|parsed| &parsed.rules);
    let resolved_language = lang_from_str(
        book.language
            .as_deref()
            .unwrap_or(&genre_profile.language),
    );
    let governed_memory_blocks = input
        .context_package
        .map(|package| build_governed_memory_evidence_blocks(package, Some(resolved_language)));

    let (settlement, settle_usage) = settle(
        chat,
        &SettleParams {
            book,
            genre_profile,
            book_rules,
            chapter_number: input.chapter_number,
            title: input.title,
            content: input.content,
            current_state: &current_state,
            ledger: if genre_profile.numerical_system {
                &ledger
            } else {
                ""
            },
            hooks: &hooks,
            chapter_summaries: &chapter_summaries,
            subplot_board: &subplot_board,
            emotional_arcs: &emotional_arcs,
            character_matrix: &character_matrix,
            volume_outline: &volume_outline,
            selected_evidence_block: join_governed_evidence_blocks(governed_memory_blocks.as_ref())
                .as_deref(),
            chapter_intent: input.chapter_intent,
            context_package: input.context_package,
            rule_stack: input.rule_stack,
            validation_feedback: input.validation_feedback,
            original_hooks: &hooks,
            original_subplots: &subplot_board,
            original_emotional_arcs: &emotional_arcs,
            original_character_matrix: &character_matrix,
        },
    )
    .await?;

    let book_dir_str = book_dir.to_string_lossy().into_owned();
    let runtime_state_artifacts = build_runtime_state_artifacts_if_present(
        ctx.state_store,
        &book_dir_str,
        settlement.runtime_state_delta.as_ref(),
        resolved_language,
        Some(input.chapter_number),
        input.allow_reapply,
        input.allow_new_hooks,
        input.baseline_chapter,
    )
    .await
    .map_err(WriteChapterError::Engine)?;

    let counting_mode = if resolved_language == WritingLanguage::En {
        LengthCountingMode::EnWords
    } else {
        LengthCountingMode::ZhChars
    };

    Ok(WriteChapterOutput {
        chapter_number: input.chapter_number,
        title: input.title.to_string(),
        content: input.content.to_string(),
        word_count: count_chapter_length(input.content, counting_mode),
        pre_write_check: String::new(),
        post_settlement: settlement.post_settlement,
        chapter_summary: match &settlement.runtime_state_delta {
            Some(delta) => render_delta_summary_row(delta),
            None => settlement.chapter_summary,
        },
        runtime_state_delta: runtime_state_artifacts
            .as_ref()
            .map(|artifacts| artifacts.resolved_delta.clone())
            .or(settlement.runtime_state_delta.clone()),
        runtime_state_snapshot: runtime_state_artifacts
            .as_ref()
            .map(|artifacts| artifacts.snapshot.clone()),
        updated_state: runtime_state_artifacts
            .as_ref()
            .map(|artifacts| artifacts.current_state_markdown.clone())
            .unwrap_or(settlement.updated_state),
        updated_ledger: settlement.updated_ledger,
        updated_hooks: runtime_state_artifacts
            .as_ref()
            .map(|artifacts| artifacts.hooks_markdown.clone())
            .unwrap_or(settlement.updated_hooks),
        updated_chapter_summaries: runtime_state_artifacts
            .as_ref()
            .map(|artifacts| artifacts.chapter_summaries_markdown.clone()),
        updated_subplots: settlement.updated_subplots,
        updated_emotional_arcs: settlement.updated_emotional_arcs,
        updated_character_matrix: settlement.updated_character_matrix,
        post_write_errors: Vec::new(),
        post_write_warnings: Vec::new(),
        hook_health_issues: Vec::new(),
        tension_metrics: settlement.tension_metrics,
        token_usage: TokenUsage {
            prompt_tokens: settle_usage.prompt_tokens,
            completion_tokens: settle_usage.completion_tokens,
            total_tokens: settle_usage.total_tokens,
        },
    })
}

/// 章节落盘（原子集：章节 + 真相文件 + 状态 JSON）。逐字移植 TS `saveChapter`。
pub async fn save_chapter(
    ctx: &WriterCtx<'_>,
    book_dir: &Path,
    output: &WriteChapterOutput,
    numerical_system: bool,
    language: WritingLanguage,
) -> Result<(), WriteChapterError> {
    let chapters_dir = book_dir.join("chapters");
    tokio::fs::create_dir_all(&chapters_dir).await?;

    let padded_num = format!("{:04}", output.chapter_number);
    let filename = format!("{padded_num}_{}.md", sanitize_filename(&output.title));
    let existing_chapter_files: Vec<String> = match tokio::fs::read_dir(&chapters_dir).await {
        Ok(mut entries) => {
            let mut names = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
            names
        }
        Err(_) => Vec::new(),
    };
    let superseded_chapter_files: Vec<String> = existing_chapter_files
        .iter()
        .filter(|file| file.starts_with(&format!("{padded_num}_")) && file.ends_with(".md") && **file != filename)
        .cloned()
        .collect();

    let heading = if language == WritingLanguage::En {
        format!("# Chapter {}: {}", output.chapter_number, output.title)
    } else {
        format!("# 第{}章 {}", output.chapter_number, output.title)
    };
    let chapter_content = format!("{heading}\n\n{}", output.content);

    let book_dir_str = book_dir.to_string_lossy().into_owned();
    let runtime_state_artifacts = resolve_runtime_state_artifacts_for_output(
        ctx.state_store,
        &book_dir_str,
        output,
        language,
    )
    .await
    .map_err(WriteChapterError::Engine)?;

    let mut writes: Vec<AtomicFileWrite> = vec![
        AtomicFileWrite {
            relative_path: format!("chapters/{filename}"),
            content: FileContent::Text(chapter_content),
        },
        AtomicFileWrite {
            relative_path: "story/current_state.md".to_string(),
            content: FileContent::Text(
                runtime_state_artifacts
                    .as_ref()
                    .map(|artifacts| artifacts.current_state_markdown.clone())
                    .unwrap_or_else(|| output.updated_state.clone()),
            ),
        },
        AtomicFileWrite {
            relative_path: "story/pending_hooks.md".to_string(),
            content: FileContent::Text(
                runtime_state_artifacts
                    .as_ref()
                    .map(|artifacts| artifacts.hooks_markdown.clone())
                    .unwrap_or_else(|| output.updated_hooks.clone()),
            ),
        },
    ];

    if let Some(artifacts) = &runtime_state_artifacts {
        if !artifacts.chapter_summaries_markdown.is_empty() {
            writes.push(AtomicFileWrite {
                relative_path: "story/chapter_summaries.md".to_string(),
                content: FileContent::Text(artifacts.chapter_summaries_markdown.clone()),
            });
        }
    }

    let runtime_state_snapshot = runtime_state_artifacts
        .as_ref()
        .map(|artifacts| artifacts.snapshot.clone())
        .or_else(|| output.runtime_state_snapshot.clone());
    if let Some(snapshot) = runtime_state_snapshot {
        writes.extend([
            AtomicFileWrite {
                relative_path: "story/state/manifest.json".to_string(),
                content: FileContent::Text(
                    serde_json::to_string_pretty(&snapshot.manifest)
                        .unwrap_or_default(),
                ),
            },
            AtomicFileWrite {
                relative_path: "story/state/current_state.json".to_string(),
                content: FileContent::Text(
                    serde_json::to_string_pretty(&snapshot.current_state)
                        .unwrap_or_default(),
                ),
            },
            AtomicFileWrite {
                relative_path: "story/state/hooks.json".to_string(),
                content: FileContent::Text(
                    serde_json::to_string_pretty(&snapshot.hooks).unwrap_or_default(),
                ),
            },
            AtomicFileWrite {
                relative_path: "story/state/chapter_summaries.json".to_string(),
                content: FileContent::Text(
                    serde_json::to_string_pretty(&snapshot.chapter_summaries)
                        .unwrap_or_default(),
                ),
            },
        ]);
    }

    if numerical_system {
        writes.push(AtomicFileWrite {
            relative_path: "story/particle_ledger.md".to_string(),
            content: FileContent::Text(output.updated_ledger.clone()),
        });
    }

    commit_atomic_file_set(&AtomicFileSet {
        root_dir: book_dir,
        writes,
        deletes: superseded_chapter_files
            .iter()
            .map(|file| format!("chapters/{file}"))
            .collect(),
    })
    .await?;

    Ok(())
}

/// 保存新真相文件（摘要/支线/情感弧线/角色矩阵）。逐字移植 TS `saveNewTruthFiles`。
pub async fn save_new_truth_files(
    book_dir: &Path,
    output: &WriteChapterOutput,
    language: WritingLanguage,
) -> std::io::Result<()> {
    let story_dir = book_dir.join("story");

    let no_delta = output.runtime_state_delta.is_none();
    if no_delta {
        if let Some(updated) = &output.updated_chapter_summaries {
            tokio::fs::write(story_dir.join("chapter_summaries.md"), updated).await?;
        } else if !output.chapter_summary.is_empty() {
            append_chapter_summary(
                &story_dir,
                &output.chapter_summary,
                language,
                output.tension_metrics.as_ref(),
            )
            .await?;
        }
    }

    if !output.updated_subplots.is_empty() {
        tokio::fs::write(story_dir.join("subplot_board.md"), &output.updated_subplots).await?;
    }
    if !output.updated_emotional_arcs.is_empty() {
        tokio::fs::write(story_dir.join("emotional_arcs.md"), &output.updated_emotional_arcs).await?;
    }
    if !output.updated_character_matrix.is_empty() {
        tokio::fs::write(story_dir.join("character_matrix.md"), &output.updated_character_matrix).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_strips_and_truncates() {
        assert_eq!(sanitize_filename("夜/港:账本?"), "夜港账本");
        assert_eq!(sanitize_filename("a b  c"), "a_b_c");
        let long = "字".repeat(60);
        assert_eq!(sanitize_filename(&long).encode_utf16().count(), 50);
    }

    #[test]
    fn length_requirement_block_languages() {
        let spec = build_length_spec(3000, WritingLanguage::Zh);
        assert_eq!(
            build_length_requirement_block(&spec, None),
            "要求：\n- 目标字数：3000字\n- 允许区间：2591-3409字"
        );
        assert!(build_length_requirement_block(&spec, Some(WritingLanguage::En)).contains("3000 words"));
    }

    #[test]
    fn chapter_context_block_trims_and_localizes() {
        assert_eq!(build_chapter_context_block(None, WritingLanguage::Zh), "");
        assert_eq!(build_chapter_context_block(Some("   "), WritingLanguage::Zh), "");
        let zh = build_chapter_context_block(Some(" 写打斗 "), WritingLanguage::Zh);
        assert!(zh.starts_with("## 本章用户指令（最高优先级）\n写打斗"));
        let en = build_chapter_context_block(Some("fight"), WritingLanguage::En);
        assert!(en.contains("use that title exactly in CHAPTER_TITLE"));
    }

    #[test]
    fn dialogue_fingerprints_extract_speaker_styles() {
        // 贪婪怪癖 parity：speaker 捕获吃满后回退到动词尾字（如 "林动冷声"+道），
        // 同前缀两句才聚合成一个说话人（与 JS regex 引擎语义一致）。
        let text = "林动冷声道：\"你敢再来？\"\n林动冷声道：\"滚出去？\"\n苏檀儿笑道：\"人家才不怕呢，人家才不怕呢，人家才不怕呢。\"\n路人说道：\"不知道。\"";
        let fingerprints = extract_dialogue_fingerprints(text);
        assert!(fingerprints.contains("林动冷声："), "got: {fingerprints}");
        assert!(
            fingerprints.contains("短句为主") || fingerprints.contains("长句为主"),
            "got: {fingerprints}"
        );
        assert!(!fingerprints.contains("路人"), "单条台词不入指纹");
    }

    #[test]
    fn dialogue_fingerprints_empty_input() {
        assert_eq!(extract_dialogue_fingerprints(""), "");
    }

    #[test]
    fn relevant_summaries_matches_outline_names_and_hooks() {
        // TS 语义：名字后必须紧跟 [，、。：] 或串尾才被提取（lookahead）。
        let outline = "本卷主线：林动，回收 H01 伏笔，绫清竹出场。";
        let summaries = "# 章节摘要\n\n| 章节 | 标题 |\n|---|---|\n| 1 | 林动初醒 |\n| 2 | 无关章节 |\n| 3 | H01 推进 |\n| 5 | 林动再战 |\n";
        let out = find_relevant_summaries(summaries, outline, 6);
        assert!(out.contains("| 1 | 林动初醒 |"), "got: {out}");
        assert!(out.contains("| 3 | H01 推进 |"), "got: {out}");
        // 章节 5 >= 6-1 被跳过（最近一章全文已在上下文）。
        assert!(!out.contains("| 5 |"), "got: {out}");
        assert!(!out.contains("无关章节"));
    }

    #[test]
    fn relevant_summaries_placeholder_short_circuits() {
        assert_eq!(find_relevant_summaries(MISSING_FILE, "卷纲", 3), "");
        assert_eq!(find_relevant_summaries("| 1 | a |", MISSING_FILE, 3), "");
    }

    #[test]
    fn style_fingerprint_truthiness_gates() {
        let json = r#"{"avgSentenceLength": 18.5, "sentenceLengthStdDev": 6, "topPatterns": ["排比", "对仗"]}"#;
        let fingerprint = build_style_fingerprint(json).expect("应产出");
        assert!(fingerprint.contains("- 平均句长：18.5字"));
        assert!(fingerprint.contains("- 句长标准差：6"));
        assert!(fingerprint.contains("排比、对仗"));
        assert!(!fingerprint.contains("段落长度范围"));

        assert_eq!(build_style_fingerprint(MISSING_FILE), None);
        assert_eq!(build_style_fingerprint("not json"), None);
        // 全 falsy 字段 → None。
        assert_eq!(
            build_style_fingerprint(r#"{"avgSentenceLength": 0, "topPatterns": []}"#),
            None
        );
    }

    #[test]
    fn delta_summary_row_renders_and_escapes() {
        let delta = RuntimeStateDelta {
            chapter_summary: Some(crate::models::runtime_state::ChapterSummaryRow {
                chapter: 3,
                title: "风|起".to_string(),
                characters: "林动".to_string(),
                events: "夺舍".to_string(),
                state_changes: "境界+1".to_string(),
                hook_activity: "H01 推进".to_string(),
                mood: "紧张".to_string(),
                chapter_type: "推进章".to_string(),
                conflict_level: None,
                reveal_level: None,
            }),
            ..RuntimeStateDelta::default()
        };
        let row = render_delta_summary_row(&delta);
        assert!(row.starts_with("| 3 | 风\\|起 |"));
        assert!(row.ends_with("| 推进章 |"));

        let empty = RuntimeStateDelta::default();
        assert_eq!(render_delta_summary_row(&empty), "");
    }

    #[test]
    fn normalize_delta_chapter_clamps_and_flips_summary() {
        let delta = RuntimeStateDelta {
            chapter: 9,
            hook_ops: HookOps {
                upsert: vec![crate::models::runtime_state::HookRecord {
                    hook_id: "H01".to_string(),
                    start_chapter: 12,
                    last_advanced_chapter: 15,
                    ..test_hook_record()
                }],
                ..HookOps::default()
            },
            chapter_summary: Some(crate::models::runtime_state::ChapterSummaryRow {
                chapter: 9,
                ..test_summary_row()
            }),
            ..RuntimeStateDelta::default()
        };
        let normalized = normalize_runtime_state_delta_chapter(&delta, 7);
        assert_eq!(normalized.chapter, 7);
        assert_eq!(normalized.hook_ops.upsert[0].start_chapter, 7);
        assert_eq!(normalized.hook_ops.upsert[0].last_advanced_chapter, 7);
        assert_eq!(normalized.chapter_summary.as_ref().unwrap().chapter, 7);

        // 无变化 → 相等副本。
        let clean = RuntimeStateDelta {
            chapter: 7,
            ..RuntimeStateDelta::default()
        };
        assert_eq!(normalize_runtime_state_delta_chapter(&clean, 7), clean);
    }

    fn test_hook_record() -> crate::models::runtime_state::HookRecord {
        crate::models::runtime_state::HookRecord {
            hook_id: String::new(),
            start_chapter: 0,
            hook_type: "main".to_string(),
            status: crate::models::runtime_state::HookStatus::Open,
            status_raw: String::new(),
            last_advanced_chapter: 0,
            expected_payoff: String::new(),
            payoff_timing: None,
            notes: String::new(),
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        }
    }

    fn test_summary_row() -> crate::models::runtime_state::ChapterSummaryRow {
        crate::models::runtime_state::ChapterSummaryRow {
            chapter: 1,
            title: String::new(),
            characters: String::new(),
            events: String::new(),
            state_changes: String::new(),
            hook_activity: String::new(),
            mood: String::new(),
            chapter_type: String::new(),
            conflict_level: None,
            reveal_level: None,
        }
    }

    #[test]
    fn verify_pre_write_check_missing_sections() {
        assert!(verify_pre_write_check_aligns_with_memo("", 3, WritingLanguage::Zh)
            .unwrap()
            .contains("PRE_WRITE_CHECK 为空"));
        let full = "当前任务：x\n不要做：y\n章尾：z";
        assert_eq!(
            verify_pre_write_check_aligns_with_memo(full, 3, WritingLanguage::Zh),
            None
        );
        let partial = "当前任务：x";
        assert!(verify_pre_write_check_aligns_with_memo(partial, 3, WritingLanguage::Zh)
            .unwrap()
            .contains("不要做"));
    }

    #[test]
    fn user_prompt_zh_template() {
        let spec = build_length_spec(3000, WritingLanguage::Zh);
        let prompt = build_user_prompt(&UserPromptInput {
            chapter_number: 2,
            story_bible: "世界观",
            current_state: "状态卡",
            ledger: "",
            hooks: "伏笔池",
            recent_chapters: "",
            length_spec: &spec,
            external_context: Some("加入伏笔"),
            chapter_summaries: MISSING_FILE,
            subplot_board: MISSING_FILE,
            emotional_arcs: MISSING_FILE,
            character_matrix: MISSING_FILE,
            dialogue_fingerprints: None,
            relevant_summaries: None,
            parent_canon: None,
            language: Some(WritingLanguage::Zh),
        });
        assert!(prompt.starts_with("请续写第2章。"));
        assert!(!prompt.contains("## 本章用户指令")); // legacy 用外部指令块
        assert!(prompt.contains("## 外部指令"));
        assert!(prompt.contains("(这是第一章，无前文)"));
        assert!(!prompt.contains("章节摘要"));
        assert!(prompt.contains("目标字数：3000字"));
    }

    #[test]
    fn user_prompt_en_template_with_ledger() {
        let spec = build_length_spec(2000, WritingLanguage::En);
        let prompt = build_user_prompt(&UserPromptInput {
            chapter_number: 1,
            story_bible: "world",
            current_state: "state",
            ledger: "ledger",
            hooks: "hooks",
            recent_chapters: "previous",
            length_spec: &spec,
            external_context: None,
            chapter_summaries: "| old |",
            subplot_board: "| sub |",
            emotional_arcs: MISSING_FILE,
            character_matrix: MISSING_FILE,
            dialogue_fingerprints: Some("A：短句为主"),
            relevant_summaries: Some("| 1 |"),
            parent_canon: Some("canon"),
            language: Some(WritingLanguage::En),
        });
        assert!(prompt.starts_with("Write chapter 1."));
        // TS 的 ledgerBlock 恒用中文标题（buildUserPrompt 未分支语言）。
        assert!(prompt.contains("## 资源账本"));
        // TS 的 summaries 块恒用中文标题（buildUserPrompt 未分支语言）。
        assert!(prompt.contains("## 章节摘要"));
        // TS 的 canon 块恒用中文标题。
        assert!(prompt.contains("## 正传正典参照"));
        assert!(prompt.contains("Target length: 2000 words"));
    }

    #[test]
    fn governed_user_prompt_structure() {
        let spec = build_length_spec(3000, WritingLanguage::Zh);
        let memo = ChapterMemo {
            chapter: 3,
            goal: "目标".to_string(),
            is_golden_opening: false,
            body: "正文要求".to_string(),
            thread_refs: vec!["H01".to_string()],
            reader_experience: None,
        };
        let context_package = ContextPackage {
            chapter: 3,
            selected_context: vec![
                crate::models::input_governance::ContextSource {
                    source: "story/author_intent.md".to_string(),
                    reason: "长期方向".to_string(),
                    excerpt: Some("主角成长".to_string()),
                },
                crate::models::input_governance::ContextSource {
                    source: "story/pending_hooks.md#H01".to_string(),
                    reason: "本章回收".to_string(),
                    excerpt: None,
                },
            ],
        };
        let rule_stack = RuleStack::default();
        let prompt = build_governed_user_prompt(&GovernedUserPromptInput {
            chapter_number: 3,
            chapter_memo: &memo,
            chapter_intent_data: None,
            context_package: &context_package,
            rule_stack: &rule_stack,
            external_context: Some("本章指令"),
            length_spec: &spec,
            language: Some(WritingLanguage::Zh),
            variance_brief: None,
            selected_evidence_block: None,
        });
        assert!(prompt.contains("## 用户方向（优先于模型默认，必须遵循）"));
        assert!(prompt.contains("## 本章用户指令（最高优先级）"));
        assert!(prompt.contains("## 已选上下文"));
        assert!(prompt.contains("## 规则栈"));
        assert!(prompt.contains("- 硬护栏：(无)"));
        assert!(prompt.contains("只需输出 PRE_WRITE_CHECK、CHAPTER_TITLE、CHAPTER_CONTENT 三个区块"));
    }

    #[test]
    fn settler_control_block_demotes_headings() {
        let context_package = ContextPackage {
            chapter: 1,
            selected_context: vec![crate::models::input_governance::ContextSource {
                source: "story/pending_hooks.md#H01".to_string(),
                reason: "回收".to_string(),
                excerpt: None,
            }],
        };
        let rule_stack = RuleStack::default();
        let block = build_settler_governed_control_block(
            "## Goal\n推进主线",
            &context_package,
            &rule_stack,
            WritingLanguage::Zh,
        );
        assert!(block.starts_with("\n## 本章控制输入"));
        assert!(block.contains("### 已选上下文"));
        assert!(block.contains("- none") || block.contains("### 已选上下文"));
    }

    #[tokio::test]
    async fn append_chapter_summary_dedupes_and_appends() {
        let dir = tempfile::tempdir().expect("临时目录");
        let story = dir.path();
        tokio::fs::write(
            story.join("chapter_summaries.md"),
            "# 章节摘要\n\n| 章节 | 标题 |\n|---|---|\n| 1 | 旧一 |\n| 2 | 旧二 |\n",
        )
        .await
        .expect("写摘要");
        append_chapter_summary(
            story,
            "| 章节 | 标题 |\n|---|---|\n| 2 | 新二 |\n| 3 | 新三 |",
            WritingLanguage::Zh,
            None,
        )
        .await
        .expect("追加");
        let updated = tokio::fs::read_to_string(story.join("chapter_summaries.md"))
            .await
            .expect("读回");
        assert!(updated.contains("| 1 | 旧一 |"));
        assert!(!updated.contains("| 2 | 旧二 |"));
        assert!(updated.contains("| 2 | 新二 |"));
        assert!(updated.contains("| 3 | 新三 |"));
    }

    #[tokio::test]
    async fn load_recent_chapters_joins_bodies() {
        let dir = tempfile::tempdir().expect("临时目录");
        let chapters = dir.path().join("chapters");
        tokio::fs::create_dir_all(&chapters).await.expect("建目录");
        tokio::fs::write(chapters.join("0001_a.md"), "一").await.expect("写");
        tokio::fs::write(chapters.join("0002_b.md"), "二").await.expect("写");
        tokio::fs::write(chapters.join("index.md"), "目录").await.expect("写");
        tokio::fs::write(chapters.join("0003_c.txt"), "三").await.expect("写");
        let recent = load_recent_chapters(dir.path(), 4, 2).await;
        assert_eq!(recent, "一\n\n---\n\n二");
        let single = load_recent_chapters(dir.path(), 4, 1).await;
        assert_eq!(single, "二");
        let missing = load_recent_chapters(&dir.path().join("none"), 1, 1).await;
        assert_eq!(missing, "");
    }







    // ---- 编排集成测试（MockChat + FsStateStore + 临时书目录） ----

    struct QueuedMockChat {
        responses: std::sync::Mutex<Vec<String>>,
        calls: std::sync::Mutex<Vec<(Vec<LLMMessage>, f64)>>,
    }

    impl QueuedMockChat {
        fn new(responses: Vec<String>) -> Self {
            QueuedMockChat {
                responses: std::sync::Mutex::new(responses),
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl WriterChat for QueuedMockChat {
        async fn chat(
            &self,
            messages: Vec<LLMMessage>,
            temperature: f64,
        ) -> Result<ChatOutcome, String> {
            self.calls.lock().unwrap().push((messages, temperature));
            let response = self
                .responses
                .lock()
                .unwrap()
                .pop()
                .ok_or_else(|| "mock 队列耗尽".to_string())?;
            Ok(ChatOutcome {
                content: response,
                usage: Some(AuditTokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 50,
                    total_tokens: 150,
                }),
            })
        }
    }

    /// 建 writer fixture：临时 project（genres）/ builtin genres / book 目录。
    async fn writer_fixture(book_files: &[(&str, &str)]) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
        BookConfig,
    ) {
        let dir = tempfile::tempdir().expect("临时目录");
        let project = dir.path().join("project");
        let builtin = dir.path().join("builtin");
        let book = dir.path().join("book");
        tokio::fs::create_dir_all(project.join("genres"))
            .await
            .expect("建 project genres");
        tokio::fs::create_dir_all(&builtin).await.expect("建 builtin");
        tokio::fs::write(builtin.join("xianxia.md"), WRITER_GENRE_ZH)
            .await
            .expect("写内置 genre");
        for (rel, content) in book_files {
            let path = book.join(rel);
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.expect("建父目录");
            }
            tokio::fs::write(&path, content).await.expect("写书文件");
        }
        let config = BookConfig {
            id: "test-book".to_string(),
            title: "测试书".to_string(),
            platform: crate::models::book::Platform::Other,
            genre: "xianxia".to_string(),
            status: crate::models::book::BookStatus::Active,
            target_chapters: 100,
            chapter_word_count: 3000,
            language: Some("zh".to_string()),
            created_at: String::new(),
            updated_at: String::new(),
            parent_book_id: None,
            fanfic_mode: None,
            series: None,
            writing: None,
        };
        (dir, project, builtin, book, config)
    }

    const WRITER_GENRE_ZH: &str = "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\",\"高潮章\"]\nfatigueWords: [\"震惊\"]\nauditDimensions: [1, 6]\nnumericalSystem: true\n---\n正文指导\n";

    const CREATIVE_RESPONSE: &str = "=== PRE_WRITE_CHECK ===\n- 当前任务：醒来\n- 不要做：越级\n- 章尾：出现钩子\n\n=== CHAPTER_TITLE ===\n风起\n\n=== CHAPTER_CONTENT ===\n林动睁开双眼，灵气顺着经脉游走。\n\n他握紧拳头，感受着体内翻涌的力量。多年屈辱，今日起一笔一笔讨回来。\n";

    const SETTLER_DELTA_RESPONSE: &str = "=== POST_SETTLEMENT ===\n结算完成，状态已更新。\n\n=== RUNTIME_STATE_DELTA ===\n```json\n{\"chapter\": 2, \"chapterSummary\": {\"chapter\": 2, \"title\": \"风起\", \"characters\": \"林动\", \"events\": \"醒来\", \"stateChanges\": \"无\", \"hookActivity\": \"无\", \"mood\": \"紧张\", \"chapterType\": \"推进章\"}}\n```\n";

    #[tokio::test]
    async fn write_chapter_full_pipeline_with_delta_settlement() {
        let (_tmp, project, builtin, book, config) = writer_fixture(&[
            ("story/pending_hooks.md", "| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 |\n| --- |\n| H01 | 1 | main | open | 1 | 10 | near-term | 无 | 一卷 | 否 | 5 | 否 | 种子 |\n"),
            ("chapters/0001_第一章.md", "# 第1章 初醒\n\n林动在药田中醒来。"),
        ])
        .await;

        // 调用顺序：creative → observer → settler（pop 从尾取，故逆序入队）。
        let mock = QueuedMockChat::new(vec![
            SETTLER_DELTA_RESPONSE.to_string(),
            "观察：林动醒来，无资源变动。".to_string(),
            CREATIVE_RESPONSE.to_string(),
        ]);
        let prompt_store = crate::state::store::InMemoryStateStore::new();
        let state_store = crate::state::store::FsStateStore;
        let ctx = WriterCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &prompt_store,
            state_store: &state_store,
        };
        let input = WriteChapterInput {
            book: &config,
            book_dir: &book,
            chapter_number: 2,
            external_context: None,
            chapter_intent: None,
            chapter_memo: None,
            chapter_intent_data: None,
            context_package: None,
            rule_stack: None,
            length_spec: None,
            word_count_override: None,
            temperature_override: None,
        };

        let output = write_chapter(&ctx, &mock, &input).await.expect("write_chapter");
        assert_eq!(output.title, "风起");
        assert!(output.content.contains("林动睁开双眼"));
        assert!(output.word_count > 0);
        assert_eq!(output.runtime_state_delta.as_ref().unwrap().chapter, 2);
        assert!(output.runtime_state_snapshot.is_some());
        assert!(output.chapter_summary.starts_with("| 2 |"));
        assert!(!output.updated_state.is_empty());
        assert!(!output.updated_hooks.is_empty());
        // creative 150 + settler 150（observer 用量按 TS 语义丢弃）。
        assert_eq!(output.token_usage.total_tokens, 300);

        // 三次 chat 的温度对齐 0.7 / 0.5 / 0.3（先拷出再跨 await）。
        let temperatures: Vec<f64> = mock
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|(_, temperature)| *temperature)
            .collect();
        assert_eq!(temperatures, vec![0.7, 0.5, 0.3]);

        // 落盘：章节 + 真相文件 + 状态 JSON。
        save_chapter(&ctx, &book, &output, true, WritingLanguage::Zh)
            .await
            .expect("save_chapter");
        assert!(book.join("chapters/0002_风起.md").exists());
        assert!(book.join("story/current_state.md").exists());
        assert!(book.join("story/pending_hooks.md").exists());
        assert!(book.join("story/state/manifest.json").exists());
        assert!(book.join("story/state/hooks.json").exists());
        assert!(book.join("story/particle_ledger.md").exists());

        // saveNewTruthFiles：delta 路径不追加摘要。
        save_new_truth_files(&book, &output, WritingLanguage::Zh)
            .await
            .expect("save_new_truth_files");
    }

    #[tokio::test]
    async fn write_chapter_legacy_settlement_merges_tables() {
        let (_tmp, project, builtin, book, config) = writer_fixture(&[
            ("story/pending_hooks.md", "| hook_id | 状态 |\n| --- | --- |\n| H01 | open |\n"),
        ])
        .await;

        let settler_legacy = "=== POST_SETTLEMENT ===\n结算完成\n\n=== UPDATED_STATE ===\n状态卡：林动凝魂境三层\n\n=== UPDATED_LEDGER ===\n灵石：10\n\n=== UPDATED_HOOKS ===\n| hook_id | 状态 |\n| --- | --- |\n| H01 | resolved |\n\n=== CHAPTER_SUMMARY ===\n| 2 | 风起 |\n\n=== UPDATED_SUBPLOTS ===\n支线A 已推进\n";
        let mock = QueuedMockChat::new(vec![
            settler_legacy.to_string(),
            "观察内容".to_string(),
            CREATIVE_RESPONSE.to_string(),
        ]);
        let prompt_store = crate::state::store::InMemoryStateStore::new();
        let state_store = crate::state::store::FsStateStore;
        let ctx = WriterCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &prompt_store,
            state_store: &state_store,
        };
        let input = WriteChapterInput {
            book: &config,
            book_dir: &book,
            chapter_number: 2,
            external_context: None,
            chapter_intent: None,
            chapter_memo: None,
            chapter_intent_data: None,
            context_package: None,
            rule_stack: None,
            length_spec: None,
            word_count_override: None,
            temperature_override: None,
        };

        let output = write_chapter(&ctx, &mock, &input).await.expect("write_chapter");
        assert_eq!(output.runtime_state_delta, None);
        assert_eq!(output.post_settlement, "结算完成");
        assert!(output.updated_state.contains("凝魂境"));
        assert!(output.updated_ledger.contains("灵石"));
        assert!(output.updated_hooks.contains("resolved"));
        assert!(output.chapter_summary.contains("风起"));
    }

    #[tokio::test]
    async fn settle_chapter_state_reuses_pipeline() {
        let (_tmp, project, builtin, book, config) = writer_fixture(&[]).await;
        let mock = QueuedMockChat::new(vec![
            SETTLER_DELTA_RESPONSE.to_string(),
            "观察：主角突破。".to_string(),
        ]);
        let prompt_store = crate::state::store::InMemoryStateStore::new();
        let state_store = crate::state::store::FsStateStore;
        let ctx = WriterCtx {
            project_root: &project,
            builtin_genres_dir: &builtin,
            prompt_store: &prompt_store,
            state_store: &state_store,
        };
        let input = SettleChapterStateInput {
            book: &config,
            book_dir: &book,
            chapter_number: 3,
            baseline_chapter: None,
            title: "旧章",
            content: "正文内容，主角突破。",
            allow_reapply: None,
            allow_new_hooks: None,
            chapter_intent: None,
            context_package: None,
            rule_stack: None,
            validation_feedback: None,
        };
        let output = settle_chapter_state(&ctx, &mock, &input)
            .await
            .expect("settle_chapter_state");
        assert_eq!(output.title, "旧章");
        assert_eq!(output.pre_write_check, "");
        assert!(output.word_count > 0);
        assert!(output.runtime_state_delta.is_some());
        assert_eq!(output.token_usage.total_tokens, 150);
    }
}
