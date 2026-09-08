//! composer 编排 —— 治理上下文包/规则栈/追踪链装配 + 语义压缩预算控制。
//!
//! 移植自 `packages/core/src/agents/composer.ts`（1083 行）。write-next 链的
//! 中游：planner 产物 → [`compose_governed_chapter`] → writer 入参。
//!
//! - **上下文收集**：memo 携带 + 四路权威文件（legacy 回退）+ 大纲选段（确定性
//!   相关性 + 可选 LLM 语义选择器）+ 正典 + 近章轨迹 + 记忆选集（事实/摘要/
//!   hook/卷摘要）+ hook 债简报
//! - **预算控制**：保护源不可压缩（超预算直接报错）；可压源经编译器语义压缩，
//!   全程 ContextCompression 回调上报
//! - **工件落盘**：context.json / rule-stack.yaml / trace.json 三件套
//!
//! ## 移植纪律
//! - 选定条目的**顺序**是 load-bearing（memo → 焦点/意图/漂移/状态 → 大纲 →
//!   正典 → 轨迹 → hook 债 → 事实 → 摘要 → 卷摘要 → hook）
//! - `parseInt(file.slice(0,4))` 前缀解析、`slice(0,57)`/`length > 5` 的
//!   UTF-16 语义、`Set` 保序去重逐字对齐
//! - 语义选择器失败静默回退确定性路径（质量引导非硬依赖）

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::agents::continuity::ChatOutcome;
use crate::agents::planner::PlanChapterOutput;
use crate::llm::provider::{estimate_text_tokens, LLMMessage, LLMRole};
use crate::models::context_compression::{
    ContextCompressionCategory, ContextCompressionEvent, ContextCompressionPhase,
};
use crate::models::input_governance::{
    ChapterTrace, ContextPackage, ContextSource, RuleStack, TraceCompression,
};
use crate::models::runtime_state::HookRecord;
use crate::state::memory_db::{NewFact, StoredSummary};
use crate::utils::context_assembly::{
    build_governed_rule_stack, build_governed_trace, is_protected_context_source,
    GovernedTraceParams,
};
use crate::utils::hook_lifecycle::{hook_payoff_timing_canonical, hook_status_text};
use crate::utils::language::WritingLanguage;
use crate::utils::memory_retrieval::{
    retrieve_memory_selection, RetrieveMemoryParams, VolumeSummarySelection,
};
use crate::utils::runtime_writer::write_governed_runtime_artifacts;
use crate::utils::story_markdown::parse_chapter_summaries_markdown;

/// readFileOrDefault fallback（对齐 TS 硬编码）。
const MISSING_FILE: &str = "(文件尚未创建)";

/// 上下文预算。对齐 TS `ContextBudget`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ContextBudget {
    pub context_window_tokens: u64,
    pub reserved_output_tokens: u64,
}

/// 可压缩上下文编译请求。对齐 TS `CompressibleContextCompileRequest`。
pub struct CompressibleContextCompileRequest<'a> {
    pub chapter_number: u32,
    pub goal: &'a str,
    pub language: WritingLanguage,
    pub max_input_tokens: u64,
    pub protected_entries: &'a [ContextSource],
    pub compressible_entries: &'a [ContextSource],
}

/// 可压缩上下文编译器端口（生产实现为 LLM 语义编译；测试 mock）。
#[async_trait]
pub trait CompressibleContextCompiler: Send + Sync {
    async fn compile(&self, request: CompressibleContextCompileRequest<'_>) -> Result<String, String>;
}

/// 大纲选段候选。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutlineSectionCandidate {
    pub source: String,
    pub heading: String,
    pub excerpt: String,
}

/// 大纲选段请求。对齐 TS `OutlineSectionSelectionRequest`。
pub struct OutlineSectionSelectionRequest<'a> {
    pub file_name: &'a str,
    pub kind: OutlineSectionKind,
    pub chapter_number: u32,
    pub goal: &'a str,
    pub outline_node: &'a str,
    pub language: WritingLanguage,
    pub candidates: &'a [OutlineSectionCandidate],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineSectionKind {
    StoryFrame,
    VolumeMap,
}

/// 大纲语义选择器端口（生产实现为 LLM 选择；测试 mock）。
#[async_trait]
pub trait OutlineSectionSelector: Send + Sync {
    async fn select(&self, request: OutlineSectionSelectionRequest<'_>) -> Result<Vec<String>, String>;
}

/// 压缩进度回调（同步、fire-and-forget）。
pub type CompressionCallback = Arc<dyn Fn(&ContextCompressionEvent) + Send + Sync>;

/// compose 入参。对齐 TS `ComposeChapterInput`（回调以 trait 注入）。
pub struct ComposeChapterInput<'a> {
    pub book_language: Option<&'a str>,
    pub book_dir: &'a Path,
    pub chapter_number: u32,
    pub plan: &'a PlanChapterOutput,
    pub context_budget: Option<ContextBudget>,
    pub compiler: Option<&'a dyn CompressibleContextCompiler>,
    pub outline_section_selector: Option<&'a dyn OutlineSectionSelector>,
    /// 216 号：引用选段注入（TS `referenceContextProvider`——None = 不注入）。
    pub reference_context_provider: Option<&'a dyn crate::references::BookReferenceContextProvider>,
    /// 244 号：记忆语义精简（TS `memorySemanticSelector`——None = BM25 直通）。
    pub memory_semantic_selector: Option<&'a dyn crate::utils::memory_retrieval::MemorySemanticSelector>,
    pub on_context_compression: Option<CompressionCallback>,
}

/// compose 出参。对齐 TS `ComposeChapterOutput`。
#[derive(Debug, Clone)]
pub struct ComposeChapterOutput {
    pub context_package: ContextPackage,
    pub rule_stack: RuleStack,
    pub trace: ChapterTrace,
    pub context_path: PathBuf,
    pub rule_stack_path: PathBuf,
    pub trace_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ComposeChapterError {
    #[error("Protected context exceeds available input budget ({protected_tokens}/{available_tokens} tokens). InkOS will not compress protected author intent, current focus, hard state, or active hook evidence.")]
    ProtectedOverBudget {
        protected_tokens: u64,
        available_tokens: u64,
    },
    #[error("Context exceeds available input budget ({total_tokens}/{available_tokens} tokens), but no compressible context compiler was provided.")]
    NoCompiler { total_tokens: u64, available_tokens: u64 },
    #[error("Compressible context compiler returned empty output.")]
    EmptyCompiledContext,
    #[error("compressible context compiler failed: {0}")]
    Compiler(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn lang_from_str(value: Option<&str>) -> WritingLanguage {
    if value == Some("en") {
        WritingLanguage::En
    } else {
        WritingLanguage::Zh
    }
}

/// 治理 compose 主入口：收集上下文 → 预算控制 → 规则栈/追踪链 → 落盘三件套。
pub async fn compose_governed_chapter(
    input: &ComposeChapterInput<'_>,
) -> Result<ComposeChapterOutput, ComposeChapterError> {
    let story_dir = input.book_dir.join("story");
    let runtime_dir = story_dir.join("runtime");
    tokio::fs::create_dir_all(&runtime_dir).await?;

    let language = lang_from_str(input.book_language);
    let mut selected_context = collect_selected_context(
        &story_dir,
        input.plan,
        language,
        input.outline_section_selector,
        input.memory_semantic_selector,
    )
    .await;
    // 216 号：引用选段注入（TS loadReferenceContext：条目接在基础上下文之后，
    // notes 前置于预算 notes）。
    let mut reference_notes: Vec<String> = Vec::new();
    if let Some(provider) = input.reference_context_provider {
        let lang_str = if language == WritingLanguage::En { "en" } else { "zh" };
        let task = crate::references::ReferenceSelectionTask {
            chapter_number: input.chapter_number,
            goal: &input.plan.intent.goal,
            outline_node: input.plan.intent.outline_node.as_deref().unwrap_or(""),
            must_keep: &input.plan.intent.must_keep,
            language: lang_str,
        };
        let reference = provider.select_context(&task).await;
        reference_notes = reference.notes;
        selected_context.extend(reference.entries);
    }
    let initial_context_package = ContextPackage {
        chapter: input.chapter_number,
        selected_context,
    };

    let budgeted = apply_context_budget_if_needed(
        &initial_context_package,
        input.chapter_number,
        &input.plan.intent.goal,
        language,
        input.context_budget,
        input.compiler,
        input.on_context_compression.as_ref(),
    )
    .await?;
    let context_package = budgeted.context_package;

    let rule_stack = build_governed_rule_stack(
        &input.plan.intent.must_avoid,
        &input.plan.intent.style_emphasis,
        input.chapter_number,
    );
    // TS：notes = [...referenceContext.notes, ...budgeted.notes]。
    let mut notes = reference_notes;
    notes.extend(budgeted.notes.iter().cloned());
    let trace = build_governed_trace(&GovernedTraceParams {
        chapter_number: input.chapter_number,
        planner_inputs: &input.plan.planner_inputs,
        composer_inputs: &[input.plan.runtime_path.to_string_lossy().into_owned()],
        context_package: &context_package,
        notes: &notes,
        prompt_packs: None,
        compression: budgeted.compression,
    });

    let artifacts = write_governed_runtime_artifacts(
        &runtime_dir,
        input.chapter_number,
        &context_package,
        &rule_stack,
        &trace,
    )
    .await?;

    Ok(ComposeChapterOutput {
        context_package,
        rule_stack,
        trace,
        context_path: artifacts.context_path,
        rule_stack_path: artifacts.rule_stack_path,
        trace_path: artifacts.trace_path,
    })
}

#[derive(Debug)]
struct BudgetedPackage {
    context_package: ContextPackage,
    notes: Vec<String>,
    compression: Option<TraceCompression>,
}

/// 预算控制：保护源不可压（超限报错）；可压源经编译器产出
/// `runtime/compiled-compressible-context` 条目并记录 TraceCompression。
async fn apply_context_budget_if_needed(
    context_package: &ContextPackage,
    chapter_number: u32,
    goal: &str,
    language: WritingLanguage,
    context_budget: Option<ContextBudget>,
    compiler: Option<&dyn CompressibleContextCompiler>,
    on_context_compression: Option<&CompressionCallback>,
) -> Result<BudgetedPackage, ComposeChapterError> {
    let Some(budget) = context_budget else {
        return Ok(BudgetedPackage {
            context_package: context_package.clone(),
            notes: Vec::new(),
            compression: None,
        });
    };
    if budget.context_window_tokens == 0 {
        return Ok(BudgetedPackage {
            context_package: context_package.clone(),
            notes: Vec::new(),
            compression: None,
        });
    }

    let available_input_tokens = budget
        .context_window_tokens
        .saturating_sub(budget.reserved_output_tokens);
    let selected_context = &context_package.selected_context;
    let total_tokens = estimate_selected_context_tokens(selected_context);
    if total_tokens <= available_input_tokens {
        return Ok(BudgetedPackage {
            context_package: context_package.clone(),
            notes: Vec::new(),
            compression: None,
        });
    }

    let protected_entries: Vec<ContextSource> = selected_context
        .iter()
        .filter(|entry| is_protected_context_source(&entry.source))
        .cloned()
        .collect();
    let compressible_entries: Vec<ContextSource> = selected_context
        .iter()
        .filter(|entry| !is_protected_context_source(&entry.source))
        .cloned()
        .collect();
    let protected_tokens = estimate_selected_context_tokens(&protected_entries);

    let emit = |event: ContextCompressionEvent| {
        if let Some(callback) = on_context_compression {
            (callback)(&event);
        }
    };

    if protected_tokens > available_input_tokens {
        emit(ContextCompressionEvent {
            category: ContextCompressionCategory::StoryContext,
            phase: ContextCompressionPhase::Error,
            message: Some(
                "Protected context exceeds available input budget.".to_string(),
            ),
            protected_tokens: Some(protected_tokens as u32),
            compressible_tokens: Some((total_tokens - protected_tokens) as u32),
            budget_tokens: Some(available_input_tokens as u32),
            sources: Some(protected_entries.iter().map(|e| e.source.clone()).collect()),
        });
        return Err(ComposeChapterError::ProtectedOverBudget {
            protected_tokens,
            available_tokens: available_input_tokens,
        });
    }
    if compressible_entries.is_empty() {
        return Ok(BudgetedPackage {
            context_package: context_package.clone(),
            notes: vec!["context-over-budget-no-compressible-entries".to_string()],
            compression: None,
        });
    }
    let Some(compiler) = compiler else {
        let compressible_tokens = estimate_selected_context_tokens(&compressible_entries);
        emit(ContextCompressionEvent {
            category: ContextCompressionCategory::StoryContext,
            phase: ContextCompressionPhase::Error,
            message: Some(
                "Context exceeds available input budget but no compiler was provided."
                    .to_string(),
            ),
            protected_tokens: Some(protected_tokens as u32),
            compressible_tokens: Some(compressible_tokens as u32),
            budget_tokens: Some(available_input_tokens as u32),
            sources: Some(compressible_entries.iter().map(|e| e.source.clone()).collect()),
        });
        return Err(ComposeChapterError::NoCompiler {
            total_tokens,
            available_tokens: available_input_tokens,
        });
    };

    let compile_budget = (available_input_tokens - protected_tokens).max(1);
    let compressible_tokens = estimate_selected_context_tokens(&compressible_entries);
    let compressible_sources: Vec<String> =
        compressible_entries.iter().map(|e| e.source.clone()).collect();
    let protected_sources: Vec<String> =
        protected_entries.iter().map(|e| e.source.clone()).collect();

    emit(ContextCompressionEvent {
        category: ContextCompressionCategory::StoryContext,
        phase: ContextCompressionPhase::Start,
        message: None,
        protected_tokens: Some(protected_tokens as u32),
        compressible_tokens: Some(compressible_tokens as u32),
        budget_tokens: Some(compile_budget as u32),
        sources: Some(compressible_sources.clone()),
    });

    let compiled = match compiler
        .compile(CompressibleContextCompileRequest {
            chapter_number,
            goal,
            language,
            max_input_tokens: compile_budget,
            protected_entries: &protected_entries,
            compressible_entries: &compressible_entries,
        })
        .await
    {
        Ok(compiled) => compiled.trim().to_string(),
        Err(error) => {
            emit(ContextCompressionEvent {
                category: ContextCompressionCategory::StoryContext,
                phase: ContextCompressionPhase::Error,
                message: Some(error.clone()),
                protected_tokens: Some(protected_tokens as u32),
                compressible_tokens: Some(compressible_tokens as u32),
                budget_tokens: Some(compile_budget as u32),
                sources: Some(compressible_sources.clone()),
            });
            return Err(ComposeChapterError::Compiler(error));
        }
    };
    if compiled.is_empty() {
        emit(ContextCompressionEvent {
            category: ContextCompressionCategory::StoryContext,
            phase: ContextCompressionPhase::Error,
            message: Some("Compressible context compiler returned empty output.".to_string()),
            protected_tokens: Some(protected_tokens as u32),
            compressible_tokens: Some(compressible_tokens as u32),
            budget_tokens: Some(compile_budget as u32),
            sources: Some(compressible_sources.clone()),
        });
        return Err(ComposeChapterError::EmptyCompiledContext);
    }
    emit(ContextCompressionEvent {
        category: ContextCompressionCategory::StoryContext,
        phase: ContextCompressionPhase::End,
        message: None,
        protected_tokens: Some(protected_tokens as u32),
        compressible_tokens: Some(compressible_tokens as u32),
        budget_tokens: Some(compile_budget as u32),
        sources: Some(compressible_sources.clone()),
    });

    let mut final_entries = protected_entries;
    final_entries.push(ContextSource {
        source: "runtime/compiled-compressible-context".to_string(),
        reason: "Semantic compilation of lower-priority context after protected context exceeded the input budget.".to_string(),
        excerpt: Some(compiled),
    });

    Ok(BudgetedPackage {
        context_package: ContextPackage {
            chapter: context_package.chapter,
            selected_context: final_entries,
        },
        notes: vec!["compiled-compressible-context".to_string()],
        compression: Some(TraceCompression {
            compiled_source: "runtime/compiled-compressible-context".to_string(),
            protected_sources,
            compressed_sources: compressible_sources,
            protected_tokens,
            compressible_tokens,
            budget_tokens: compile_budget,
        }),
    })
}

pub(crate) fn estimate_selected_context_tokens(entries: &[ContextSource]) -> u64 {
    entries
        .iter()
        .map(|entry| {
            let parts: Vec<&str> = [
                entry.source.as_str(),
                entry.reason.as_str(),
                entry.excerpt.as_deref().unwrap_or(""),
            ]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect();
            u64::from(estimate_text_tokens(&parts.join("\n")))
        })
        .sum()
}

/// 渲染上下文条目块（编译器 prompt 的输入形态）。golden 守门。
pub fn render_context_entries(entries: &[ContextSource]) -> String {
    entries
        .iter()
        .map(|entry| {
            [
                format!("### {}", entry.source),
                format!("Reason: {}", entry.reason),
                entry
                    .excerpt
                    .as_deref()
                    .filter(|excerpt| !excerpt.is_empty())
                    .map(|_| String::new())
                    .unwrap_or_default(),
            ]
            [0]
            .clone()
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 解析 LLM 的 selectedSources JSON（去 code fence、对象截取、白名单过滤）。
pub fn parse_selected_sources(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let trimmed = fence_open_re().replace(trimmed, "");
    let trimmed = fence_close_re().replace(&trimmed, "");
    let trimmed = trimmed.trim();

    let parsed: Option<serde_json::Value> = serde_json::from_str(trimmed).ok().or_else(|| {
        let start = trimmed.find('{')?;
        let end = trimmed.rfind('}')?;
        if end <= start {
            return None;
        }
        serde_json::from_str(&trimmed[start..=end]).ok()
    });

    let Some(values) = parsed
        .and_then(|value| value.get("selectedSources").cloned())
        .and_then(|value| value.as_array().cloned())
    else {
        return Vec::new();
    };
    values
        .into_iter()
        .filter_map(|value| value.as_str().map(String::from))
        .filter(|value| !value.trim().is_empty())
        .collect()
}

// ---- LLM 端口与 prompt 装配 ----

/// composer LLM 聊天端口（温度 + maxTokens）。
#[derive(Debug, Clone, Copy)]
pub struct ComposerChatOptions {
    pub temperature: f64,
    pub max_tokens: Option<u32>,
}

#[async_trait]
pub trait ComposerChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        options: ComposerChatOptions,
    ) -> Result<ChatOutcome, String>;
}

/// LLM 大纲语义选择器（ComposerAgent.selectOutlineSections 的端口适配）。
pub struct LlmOutlineSelector<'a> {
    pub chat: &'a dyn ComposerChat,
}

#[async_trait]
impl OutlineSectionSelector for LlmOutlineSelector<'_> {
    async fn select(
        &self,
        request: OutlineSectionSelectionRequest<'_>,
    ) -> Result<Vec<String>, String> {
        if request.candidates.len() <= 1 {
            return Ok(request
                .candidates
                .iter()
                .map(|candidate| candidate.source.clone())
                .collect());
        }
        let (system, user) = build_outline_selector_messages(&request);
        let response = self
            .chat
            .chat(
                vec![
                    LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
                    LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
                ],
                ComposerChatOptions { temperature: 0.1, max_tokens: Some(1024) },
            )
            .await?;
        let allowed: HashSet<String> = request
            .candidates
            .iter()
            .map(|candidate| candidate.source.clone())
            .collect();
        Ok(parse_selected_sources(&response.content)
            .into_iter()
            .filter(|source| allowed.contains(source))
            .collect())
    }
}

/// LLM 引用选段器（ComposerAgent.selectReferenceSections 的端口适配，216 号）。
pub struct LlmReferenceSelector<'a> {
    pub chat: &'a dyn ComposerChat,
}

#[async_trait]
impl crate::references::ReferenceSectionSelector for LlmReferenceSelector<'_> {
    async fn select(
        &self,
        request: crate::references::ReferenceSectionSelectionRequest<'_>,
    ) -> Result<Vec<String>, String> {
        let (system, user) = build_reference_selector_messages(&request);
        let response = self
            .chat
            .chat(
                vec![
                    LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
                    LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
                ],
                ComposerChatOptions { temperature: 0.1, max_tokens: Some(2048) },
            )
            .await?;
        let allowed: HashSet<String> = request
            .candidates
            .iter()
            .map(|candidate| candidate.source.clone())
            .collect();
        Ok(parse_selected_sources(&response.content)
            .into_iter()
            .filter(|source| allowed.contains(source))
            .collect())
    }
}

/// LLM 记忆语义选择器（ComposerAgent.selectMemoryCandidates 的端口适配，244 号）。
pub struct LlmMemorySelector<'a> {
    pub chat: &'a dyn ComposerChat,
}

#[async_trait]
impl crate::utils::memory_retrieval::MemorySemanticSelector for LlmMemorySelector<'_> {
    async fn select(
        &self,
        request: &crate::utils::memory_retrieval::MemorySemanticSelectionRequest<'_>,
    ) -> Result<Vec<String>, String> {
        let candidates = request
            .candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                [
                    format!("#{} {}", index + 1, candidate.id),
                    format!("kind: {}", candidate.kind),
                    format!("source: {}", candidate.source),
                    format!("title: {}", candidate.title),
                    candidate.excerpt.clone(),
                ]
                .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let system = [
            "You are InkOS's semantic story-memory selector.",
            "Select only candidate memories that materially help the current chapter task. Understand negation, corrections, causal relationships, aliases, and paraphrases; do not rank by keyword overlap.",
            "Established current-state facts and active hook lifecycle are protected separately by the host, so do not invent ids or retain unrelated candidates just to be safe.",
            "Return strict JSON only: {\"selectedSources\":[\"candidate-id\"]}.",
        ]
        .join("\n");
        let user = [
            format!("Chapter: {}", request.chapter_number),
            "Current task:".to_string(),
            request.query.to_string(),
            String::new(),
            "BM25 candidates:".to_string(),
            candidates,
        ]
        .join("\n");
        let response = self.chat.chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
            ],
            ComposerChatOptions { temperature: 0.1, max_tokens: Some(2048) },
        ).await?;
        let allowed: HashSet<String> = request
            .candidates
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect();
        Ok(parse_selected_sources(&response.content)
            .into_iter()
            .filter(|id| allowed.contains(id))
            .collect())
    }
}

/// LLM 可压缩上下文编译器（ComposerAgent.compileCompressibleContext 的端口适配）。
pub struct LlmContextCompiler<'a> {
    pub chat: &'a dyn ComposerChat,
}

#[async_trait]
impl CompressibleContextCompiler for LlmContextCompiler<'_> {
    async fn compile(
        &self,
        request: CompressibleContextCompileRequest<'_>,
    ) -> Result<String, String> {
        let (system, user) = build_context_compiler_messages(&request);
        let max_tokens = request.max_input_tokens.clamp(512, 8192) as u32;
        let response = self
            .chat
            .chat(
                vec![
                    LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
                    LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
                ],
                ComposerChatOptions { temperature: 0.2, max_tokens: Some(max_tokens) },
            )
            .await?;
        Ok(response.content.trim().to_string())
    }
}

/// 引用选段器 prompt 装配（zh/en，TS selectReferenceSections 逐字，216 号）。
pub fn build_reference_selector_messages(
    request: &crate::references::ReferenceSectionSelectionRequest<'_>,
) -> (String, String) {
    let is_en = request.language == "en";
    let candidates = request
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let mut lines = vec![
                format!("#{} {}", index + 1, candidate.source),
                format!("title: {}", candidate.title),
                format!("heading: {}", candidate.heading),
                format!("user-defined uses: {}", candidate.uses.join("; ")),
            ];
            if let Some(note) = candidate.note.as_deref() {
                lines.push(format!("user note: {note}"));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let system = if is_en {
        [
            "You are InkOS's semantic reference-section selector.",
            "The user explicitly bound these reference assets to this book and described how each may be used.",
            "Select only sections useful for the current chapter task. References are creative guidance, never canon and never stronger than author intent or established facts.",
            "Return strict JSON only: {\"selectedSources\":[\"...\"]}. Use exact candidate source ids. An empty list is valid when no section is relevant.",
        ]
        .join("\n")
    } else {
        [
            "你是 InkOS 的参考资产语义选段器。",
            "用户已把这些参考资产绑定到本书，并明确说明每份资料可以借鉴什么。",
            "只选择当前章节任务真正需要的段落。参考资料只是创作借鉴，不能成为正典，也不能压过作者意图和既成事实。",
            "只返回严格 JSON：{\"selectedSources\":[\"...\"]}。必须使用候选中的精确 source id；没有相关段落时可以返回空数组。",
        ]
        .join("\n")
    };
    let user = if is_en {
        [
            format!("Chapter: {}", request.chapter_number),
            format!("Goal: {}", request.goal),
            format!("Outline node: {}", request.outline_node),
            format!(
                "Must keep: {}",
                if request.must_keep.is_empty() { "(none)".to_string() } else { request.must_keep.join("; ") }
            ),
            String::new(),
            "Candidates (headings only; selected sections will be loaded verbatim by the host):".to_string(),
            candidates,
        ]
        .join("\n")
    } else {
        [
            format!("章节：第{}章", request.chapter_number),
            format!("目标：{}", request.goal),
            format!("大纲节点：{}", request.outline_node),
            format!(
                "必须保留：{}",
                if request.must_keep.is_empty() { "（无）".to_string() } else { request.must_keep.join("；") }
            ),
            String::new(),
            "候选段落（这里只给标题；宿主会把选中的段落原文完整载入）：".to_string(),
            candidates,
        ]
        .join("\n")
    };
    (system, user)
}

/// 大纲选择器 prompt 装配（zh/en）。golden 守门。
pub fn build_outline_selector_messages(
    request: &OutlineSectionSelectionRequest<'_>,
) -> (String, String) {
    let is_en = request.language == WritingLanguage::En;
    let candidates = request
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            [
                format!("#{} {}", index + 1, candidate.source),
                format!("heading: {}", candidate.heading),
                candidate.excerpt.clone(),
            ]
            .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let system = if is_en {
        [
            "You are InkOS's semantic outline-section selector.",
            "Select only the outline sections needed for the current chapter. Prefer semantic relevance over keyword overlap.",
            "Return strict JSON only: {\"selectedSources\":[\"...\"]}. Use exact source ids from the candidates. If uncertain, include the safest relevant anchors rather than inventing ids.",
        ]
        .join("\n")
    } else {
        [
            "你是 InkOS 的语义大纲选段器。",
            "只选择当前章节真正需要的大纲段落。按语义相关性判断，不要按关键词重合机械选择。",
            "只返回严格 JSON：{\"selectedSources\":[\"...\"]}。必须使用候选里的精确 source id；不确定时选最安全的相关锚点，不要编造 id。",
        ]
        .join("\n")
    };
    let user = if is_en {
        [
            format!("File: {}", request.file_name),
            format!("Chapter: {}", request.chapter_number),
            format!("Goal: {}", request.goal),
            format!("Outline node: {}", request.outline_node),
            String::new(),
            "Candidates:".to_string(),
            candidates,
        ]
        .join("\n")
    } else {
        [
            format!("文件：{}", request.file_name),
            format!("章节：第{}章", request.chapter_number),
            format!("目标：{}", request.goal),
            format!("大纲节点：{}", request.outline_node),
            String::new(),
            "候选段落：".to_string(),
            candidates,
        ]
        .join("\n")
    };
    (system, user)
}

/// 上下文编译器 prompt 装配（zh/en）。golden 守门。
pub fn build_context_compiler_messages(
    request: &CompressibleContextCompileRequest<'_>,
) -> (String, String) {
    let is_en = request.language == WritingLanguage::En;
    let protected_block = render_context_entries(request.protected_entries);
    let compressible_block = render_context_entries(request.compressible_entries);

    let system = if is_en {
        [
            "You are InkOS's semantic context compiler.",
            "Only compile the COMPRESSIBLE CONTEXT. The PROTECTED CONTEXT is binding reference material and must not be rewritten, summarized as a substitute, or weakened.",
            "Output concise Markdown with source pointers. Preserve names, unresolved promises, evidence, timing, and constraints that may affect the next chapter. Drop low-relevance noise.",
        ]
        .join("\n")
    } else {
        [
            "你是 InkOS 的语义上下文编译器。",
            "只能编译【可压缩上下文】。【受保护上下文】是绑定参照，不得改写、不得替代总结、不得削弱。",
            "输出简洁 Markdown，保留来源指针。保留会影响下一章的人名、未兑现承诺、证据、时间点和约束，丢弃低相关噪声。",
        ]
        .join("\n")
    };
    let user = if is_en {
        [
            format!("Chapter: {}", request.chapter_number),
            format!("Goal: {}", request.goal),
            format!("Target budget for compiled context: <= {} estimated input tokens", request.max_input_tokens),
            String::new(),
            "## Protected Context (reference only, do not compile)".to_string(),
            if protected_block.is_empty() { "(none)".to_string() } else { protected_block },
            String::new(),
            "## Compressible Context (compile this)".to_string(),
            if compressible_block.is_empty() { "(none)".to_string() } else { compressible_block },
        ]
        .join("\n")
    } else {
        [
            format!("章节：第{}章", request.chapter_number),
            format!("目标：{}", request.goal),
            format!("压缩后目标预算：不超过 {} 估算输入 tokens", request.max_input_tokens),
            String::new(),
            "## 受保护上下文（只作为参照，不要编译它）".to_string(),
            if protected_block.is_empty() { "（无）".to_string() } else { protected_block },
            String::new(),
            "## 可压缩上下文（只编译这一部分）".to_string(),
            if compressible_block.is_empty() { "（无）".to_string() } else { compressible_block },
        ]
        .join("\n")
    };
    (system, user)
}

// ---- 上下文收集 ----

async fn collect_selected_context(
    story_dir: &Path,
    plan: &PlanChapterOutput,
    language: WritingLanguage,
    outline_section_selector: Option<&dyn OutlineSectionSelector>,
    memory_semantic_selector: Option<&dyn crate::utils::memory_retrieval::MemorySemanticSelector>,
) -> Vec<ContextSource> {
    let retrieval_hints = derive_retrieval_hints(plan);
    let memo_body_excerpt = plan.memo.body.trim();
    let chapter_memo_entry = if !memo_body_excerpt.is_empty() {
        vec![ContextSource {
            source: "runtime/chapter_memo".to_string(),
            reason: "Carry the planner's chapter memo into governed writing.".to_string(),
            excerpt: Some(
                [
                    Some(format!("goal={}", plan.memo.goal)),
                    plan.memo
                        .is_golden_opening
                        .then(|| "golden-opening=true".to_string()),
                    Some(memo_body_excerpt.to_string()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" | "),
            ),
        }]
    } else {
        vec![ContextSource {
            source: "runtime/chapter_memo".to_string(),
            reason: "Carry the planner's chapter memo into governed writing.".to_string(),
            excerpt: Some(format!("goal={}", plan.memo.goal)),
        }]
    };

    let (focus, intent, drift, state, frame_entries, map_entries, canon, trail) = tokio::join!(
        maybe_context_source(story_dir, "current_focus.md", "Current task focus for this chapter."),
        maybe_context_source(story_dir, "author_intent.md", "User's long-term authorial intent and direction — binding, overrides model defaults."),
        maybe_context_source(story_dir, "audit_drift.md", "Carry forward audit drift guidance from the previous chapter without polluting hard state facts."),
        maybe_context_source(story_dir, "current_state.md", "Preserve hard state facts referenced by the active chapter brief or hard constraints."),
        maybe_outline_section_sources(story_dir, "outline/story_frame.md", "Preserve canon constraints referenced by the active chapter brief or hard constraints.", plan, OutlineSectionKind::StoryFrame, language, outline_section_selector),
        maybe_outline_section_sources(story_dir, "outline/volume_map.md", "Anchor the default planning node for this chapter.", plan, OutlineSectionKind::VolumeMap, language, outline_section_selector),
        async {
            let (parent, fanfic) = tokio::join!(
                maybe_context_source(story_dir, "parent_canon.md", "Preserve parent canon constraints for governed continuation or fanfic writing."),
                maybe_context_source(story_dir, "fanfic_canon.md", "Preserve extracted fanfic canon constraints for governed writing."),
            );
            [parent, fanfic].into_iter().flatten().collect::<Vec<_>>()
        },
        build_recent_chapter_trail_entries(story_dir, plan.intent.chapter),
    );

    let book_dir = story_dir.parent().unwrap_or(story_dir);
    let memory_selection = retrieve_memory_selection(&RetrieveMemoryParams {
        book_dir,
        chapter_number: plan.intent.chapter,
        goal: &plan.intent.goal,
        outline_node: plan.intent.outline_node.as_deref(),
        must_keep: &retrieval_hints,
        semantic_selector: memory_semantic_selector,
    })
    .await;

    let hook_debt_entries =
        build_hook_debt_entries(story_dir, plan, &memory_selection.active_hooks, language).await;

    let fact_entries = memory_selection
        .facts
        .iter()
        .map(fact_entry)
        .collect::<Vec<_>>();
    let summary_entries = memory_selection
        .summaries
        .iter()
        .map(summary_entry)
        .collect::<Vec<_>>();
    let volume_summary_entries = memory_selection
        .volume_summaries
        .iter()
        .map(volume_summary_entry)
        .collect::<Vec<_>>();
    let hook_entries = memory_selection
        .hooks
        .iter()
        .map(hook_entry)
        .collect::<Vec<_>>();

    let mut entries = chapter_memo_entry;
    entries.extend([focus, intent, drift, state].into_iter().flatten());
    entries.extend(frame_entries);
    entries.extend(map_entries);
    entries.extend(canon);
    entries.extend(trail);
    entries.extend(hook_debt_entries);
    entries.extend(fact_entries);
    entries.extend(summary_entries);
    entries.extend(volume_summary_entries);
    entries.extend(hook_entries);
    entries
}

fn derive_retrieval_hints(plan: &PlanChapterOutput) -> Vec<String> {
    let mut hints = vec![plan.intent.goal.clone()];
    hints.extend(plan.intent.outline_node.iter().cloned());
    hints.extend(plan.memo.thread_refs.iter().cloned());
    hints.into_iter().filter(|value| !value.is_empty()).collect()
}

async fn build_recent_chapter_trail_entries(
    story_dir: &Path,
    chapter_number: u32,
) -> Vec<ContextSource> {
    let content = read_file_or_default(&story_dir.join("chapter_summaries.md")).await;
    if content.is_empty() || content == MISSING_FILE {
        return Vec::new();
    }

    let mut recent: Vec<StoredSummary> = parse_chapter_summaries_markdown(&content)
        .into_iter()
        .filter(|summary| summary.chapter < i64::from(chapter_number))
        .collect();
    recent.sort_by(|left, right| right.chapter.cmp(&left.chapter));
    recent.truncate(5);
    if recent.is_empty() {
        return Vec::new();
    }

    let mut entries: Vec<ContextSource> = Vec::new();

    let recent_titles = recent
        .iter()
        .map(|summary| format!("{}: {}", summary.chapter, summary.title))
        .collect::<Vec<_>>()
        .join(" | ");
    if !recent_titles.is_empty() {
        entries.push(ContextSource {
            source: "story/chapter_summaries.md#recent_titles".to_string(),
            reason: "Keep recent title history visible to avoid repetitive chapter naming."
                .to_string(),
            excerpt: Some(recent_titles),
        });
    }

    let mood_trail = recent
        .iter()
        .filter(|summary| !summary.mood.is_empty() || !summary.chapter_type.is_empty())
        .map(|summary| {
            format!(
                "{}: {} / {}",
                summary.chapter,
                if summary.mood.is_empty() { "(none)" } else { &summary.mood },
                if summary.chapter_type.is_empty() { "(none)" } else { &summary.chapter_type },
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    if !mood_trail.is_empty() {
        entries.push(ContextSource {
            source: "story/chapter_summaries.md#recent_mood_type_trail".to_string(),
            reason: "Keep recent mood and chapter-type cadence visible before writing the next chapter.".to_string(),
            excerpt: Some(mood_trail),
        });
    }

    if let Some(ending_trail) = build_recent_ending_trail(story_dir, chapter_number).await {
        entries.push(ContextSource {
            source: "story/chapters#recent_endings".to_string(),
            reason: "Show how recent chapters ended so the writer avoids structural repetition (e.g. 3 consecutive collapse endings).".to_string(),
            excerpt: Some(ending_trail),
        });
    }

    entries
}

async fn build_recent_ending_trail(story_dir: &Path, chapter_number: u32) -> Option<String> {
    let chapters_dir = story_dir.parent()?.join("chapters");
    let mut entries = tokio::fs::read_dir(&chapters_dir).await.ok()?;
    let mut chapter_files: Vec<(String, i64)> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".md") {
            continue;
        }
        // TS parseInt(file.slice(0, 4), 10)：前缀解析（"0012-x" → 12）。
        let head: String = name.chars().take(4).collect();
        if let Some(num) = parse_int_prefix(&head) {
            if num < i64::from(chapter_number) {
                chapter_files.push((name, num));
            }
        }
    }
    chapter_files.sort_by(|a, b| b.1.cmp(&a.1));
    chapter_files.truncate(3);
    chapter_files.reverse();

    let mut endings: Vec<String> = Vec::new();
    for (name, num) in &chapter_files {
        let Ok(content) = tokio::fs::read_to_string(chapters_dir.join(name)).await else {
            continue;
        };
        if let Some(last_line) = extract_last_meaningful_sentence(&content) {
            endings.push(format!("ch{num}: {last_line}"));
        }
    }
    if endings.len() >= 2 {
        Some(endings.join(" | "))
    } else {
        None
    }
}

/// 末行提取：>5 UTF-16 码元、非标题/表格/分隔线；>60 码元截 57 + "..."。
pub fn extract_last_meaningful_sentence(content: &str) -> Option<String> {
    let lines: Vec<&str> = content
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| {
            line.chars().count() > 5
                && !line.starts_with('#')
                && !line.starts_with('|')
                && !line.starts_with("===")
        })
        .collect();
    let last = lines.last()?;
    if last.chars().count() > 60 {
        let head: String = last.chars().take(57).collect();
        Some(format!("{head}..."))
    } else {
        Some((*last).to_string())
    }
}

/// parseInt 前缀语义：开头连续数字解析（"12ab"→12，"ab12"→None）。
fn parse_int_prefix(value: &str) -> Option<i64> {
    let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

async fn build_hook_debt_entries(
    story_dir: &Path,
    plan: &PlanChapterOutput,
    active_hooks: &[HookRecord],
    language: WritingLanguage,
) -> Vec<ContextSource> {
    // Set(threadRefs) 保序去重。
    let mut seen = HashSet::new();
    let target_hook_ids: Vec<&String> = plan
        .memo
        .thread_refs
        .iter()
        .filter(|id| seen.insert((*id).clone()))
        .collect();
    if target_hook_ids.is_empty() {
        return Vec::new();
    }

    let summaries = parse_chapter_summaries_markdown(
        &read_file_or_default(&story_dir.join("chapter_summaries.md")).await,
    );

    let en = language == WritingLanguage::En;
    target_hook_ids
        .iter()
        .filter_map(|hook_id| {
            let hook = active_hooks
                .iter()
                .find(|entry| entry.hook_id == **hook_id)?;

            let seed_summary =
                find_hook_summary(&summaries, hook_id, i64::from(hook.start_chapter), true);
            let latest_summary = find_hook_summary(
                &summaries,
                hook_id,
                i64::from(hook.last_advanced_chapter),
                false,
            );
            let role = if en { "memo-referenced debt" } else { "备忘引用旧债" };
            let promise = if hook.expected_payoff.is_empty() {
                if en { "(unspecified)" } else { "（未写明）" }
            } else {
                &hook.expected_payoff
            };
            let seed_beat = match &seed_summary {
                Some(summary) => render_hook_debt_beat(summary),
                None => {
                    if hook.notes.is_empty() {
                        promise.to_string()
                    } else {
                        hook.notes.clone()
                    }
                }
            };
            let latest_beat = match &latest_summary {
                Some(latest) if Some(latest) != seed_summary.as_ref() => {
                    Some(render_hook_debt_beat(latest))
                }
                _ => None,
            };
            let age = i64::from(plan.intent.chapter)
                .saturating_sub(i64::from(hook.start_chapter.max(1)))
                .max(0);

            let excerpt = if en {
                [
                    Some(format!("{} ({}, {}, open {} chapters)", hook.hook_id, hook.hook_type, role, age)),
                    Some(format!("reader promise: {promise}")),
                    Some(format!("original seed (ch{}): {}", hook.start_chapter, seed_beat)),
                    latest_beat.map(|beat| format!("latest turn (ch{}): {}", hook.last_advanced_chapter, beat)),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" | ")
            } else {
                [
                    Some(format!("{}（{}，{}，已开{}章）", hook.hook_id, hook.hook_type, role, age)),
                    Some(format!("读者承诺：{promise}")),
                    Some(format!("种于第{}章：{}", hook.start_chapter, seed_beat)),
                    latest_beat.map(|beat| format!("推进于第{}章：{}", hook.last_advanced_chapter, beat)),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" | ")
            };

            Some(ContextSource {
                source: format!("runtime/hook_debt#{}", hook.hook_id),
                reason: if en {
                    "Narrative debt brief with original seed text for this hook agenda target."
                        .to_string()
                } else {
                    "含原始种子文本的叙事债务简报。".to_string()
                },
                excerpt: Some(excerpt),
            })
        })
        .collect()
}

async fn maybe_context_source(
    story_dir: &Path,
    file_name: &str,
    reason: &str,
) -> Option<ContextSource> {
    let mut content = read_file_or_default(&story_dir.join(file_name)).await;
    let mut resolved_file_name = file_name.to_string();

    if content.is_empty() || content == MISSING_FILE {
        // Phase 5 back-compat：legacy 书缺 outline/ 新文件时透明回退旧路径。
        if let Some(legacy_fallback) = outline_fallback(file_name) {
            let legacy_content =
                read_file_or_default(&story_dir.join(legacy_fallback)).await;
            if !legacy_content.is_empty() && legacy_content != MISSING_FILE {
                content = legacy_content;
                resolved_file_name = legacy_fallback.to_string();
            }
        }
    }

    if content.is_empty() || content == MISSING_FILE {
        return None;
    }

    Some(ContextSource {
        source: format!("story/{resolved_file_name}"),
        reason: reason.to_string(),
        excerpt: Some(content.trim().to_string()),
    })
}

async fn maybe_outline_section_sources(
    story_dir: &Path,
    file_name: &str,
    reason: &str,
    plan: &PlanChapterOutput,
    kind: OutlineSectionKind,
    language: WritingLanguage,
    outline_section_selector: Option<&dyn OutlineSectionSelector>,
) -> Vec<ContextSource> {
    let content = read_file_or_default(&story_dir.join(file_name)).await;
    if content.is_empty() || content == MISSING_FILE {
        let Some(legacy_fallback) = outline_fallback(file_name) else {
            return Vec::new();
        };
        let legacy_content = read_file_or_default(&story_dir.join(legacy_fallback)).await;
        if legacy_content.is_empty() || legacy_content == MISSING_FILE {
            return Vec::new();
        }
        return select_outline_section_entries(
            legacy_fallback,
            &legacy_content,
            reason,
            plan,
            kind,
            language,
            outline_section_selector,
        )
        .await;
    }

    select_outline_section_entries(
        file_name,
        &content,
        reason,
        plan,
        kind,
        language,
        outline_section_selector,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn select_outline_section_entries(
    file_name: &str,
    content: &str,
    reason: &str,
    plan: &PlanChapterOutput,
    kind: OutlineSectionKind,
    language: WritingLanguage,
    outline_section_selector: Option<&dyn OutlineSectionSelector>,
) -> Vec<ContextSource> {
    let sections = split_markdown_sections(content);
    if sections.is_empty() {
        return vec![ContextSource {
            source: format!("story/{file_name}#document"),
            reason: reason.to_string(),
            excerpt: Some(content.trim().to_string()),
        }];
    }

    let hints = derive_outline_selection_hints(plan);
    let selected: Vec<&MarkdownSection> = sections
        .iter()
        .filter(|section| match kind {
            OutlineSectionKind::StoryFrame => {
                is_relevant_story_frame_section(section, &hints)
            }
            OutlineSectionKind::VolumeMap => is_relevant_volume_map_section(
                section,
                &hints,
                plan.intent.chapter,
            ),
        })
        .collect();
    let final_sections: Vec<&MarkdownSection> = if !selected.is_empty() {
        selected
    } else {
        fallback_outline_sections(&sections, kind, plan.intent.chapter)
            .into_iter()
            .collect()
    };

    let candidates: Vec<OutlineSectionCandidate> = sections
        .iter()
        .map(|section| OutlineSectionCandidate {
            source: format!("story/{file_name}#{}", slugify_anchor(&section.heading, "section")),
            heading: section.heading.clone(),
            excerpt: section.raw.trim().to_string(),
        })
        .collect();

    if let Some(selector) = outline_section_selector {
        // 语义选择是质量引导非硬依赖：provider 抖动/坏 JSON 时回退确定性路径。
        let request = OutlineSectionSelectionRequest {
            file_name,
            kind,
            chapter_number: plan.intent.chapter,
            goal: &plan.intent.goal,
            outline_node: plan.intent.outline_node.as_deref().unwrap_or(""),
            language,
            candidates: &candidates,
        };
        if let Ok(selected_sources) = selector.select(request).await {
            let selected_set: HashSet<String> = selected_sources.into_iter().collect();
            let llm_sections: Vec<ContextSource> = sections
                .iter()
                .filter(|section| {
                    selected_set
                        .contains(&format!("story/{file_name}#{}", slugify_anchor(&section.heading, "section")))
                })
                .map(|section| ContextSource {
                    source: format!(
                        "story/{file_name}#{}",
                        slugify_anchor(&section.heading, "section")
                    ),
                    reason: reason.to_string(),
                    excerpt: Some(section.raw.trim().to_string()),
                })
                .collect();
            if !llm_sections.is_empty() {
                return dedupe_by_source(llm_sections);
            }
        }
    }

    dedupe_by_source(
        final_sections
            .into_iter()
            .map(|section| ContextSource {
                source: format!(
                    "story/{file_name}#{}",
                    slugify_anchor(&section.heading, "section")
                ),
                reason: reason.to_string(),
                excerpt: Some(section.raw.trim().to_string()),
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownSection {
    pub heading: String,
    pub raw: String,
}

/// 按 1-6 级标题切段（段内有非空行才算数；标题行计入 raw）。
pub fn split_markdown_sections(content: &str) -> Vec<MarkdownSection> {
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in content.split("\r\n").flat_map(|chunk| chunk.split('\n')) {
        if let Some(heading) = parse_heading(line) {
            if let Some((_, lines)) = &current {
                if lines.iter().any(|entry| !entry.trim().is_empty()) {
                    sections.push(current.take().expect("已判存在"));
                }
            }
            current = Some((heading, vec![line]));
            continue;
        }
        if let Some((_, lines)) = &mut current {
            lines.push(line);
        }
    }
    if let Some((_, lines)) = &current {
        if lines.iter().any(|entry| !entry.trim().is_empty()) {
            sections.push(current.take().expect("已判存在"));
        }
    }
    sections
        .into_iter()
        .map(|(heading, lines)| MarkdownSection {
            heading,
            raw: lines.join("\n").trim().to_string(),
        })
        .filter(|section| !section.raw.is_empty())
        .collect()
}

fn parse_heading(line: &str) -> Option<String> {
    let captures = heading_re().captures(line)?;
    Some(captures.get(2)?.as_str().trim().to_string())
}

fn derive_outline_selection_hints(plan: &PlanChapterOutput) -> Vec<String> {
    let mut hints = vec![
        plan.intent.goal.clone(),
        plan.intent.outline_node.clone().unwrap_or_default(),
        plan.intent.arc_context.clone().unwrap_or_default(),
    ];
    hints.extend(plan.intent.must_keep.iter().cloned());
    hints.extend(plan.intent.must_avoid.iter().cloned());
    hints.extend(plan.intent.style_emphasis.iter().cloned());
    hints.push(plan.memo.goal.clone());
    hints.push(plan.memo.body.clone());
    hints.extend(plan.memo.thread_refs.iter().cloned());
    hints
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect()
}

fn is_relevant_story_frame_section(section: &MarkdownSection, hints: &[String]) -> bool {
    let heading = normalize_for_match(&section.heading);
    const HARD_HEADING_SIGNALS: [&str; 11] = [
        "世界观", "底色", "铁律", "规则", "核心冲突", "终局", "world", "tonal", "rule",
        "core conflict", "endgame",
    ];
    if HARD_HEADING_SIGNALS
        .iter()
        .any(|signal| heading.contains(signal))
    {
        return true;
    }
    matches_outline_hints(&normalize_for_match(&section.raw), hints)
}

fn is_relevant_volume_map_section(
    section: &MarkdownSection,
    hints: &[String],
    chapter_number: u32,
) -> bool {
    let heading = normalize_for_match(&section.heading);
    if heading_mentions_chapter(&heading, chapter_number) {
        return true;
    }
    matches_outline_hints(&normalize_for_match(&section.raw), hints)
}

fn matches_outline_hints(section_text: &str, hints: &[String]) -> bool {
    for hint in hints {
        let terms = extract_match_terms(hint);
        if terms.is_empty() {
            continue;
        }
        let required = std::cmp::min(2, terms.len());
        let hits = terms
            .iter()
            .filter(|term| section_text.contains(term.as_str()))
            .count();
        if hits >= required {
            return true;
        }
    }
    false
}

fn fallback_outline_sections(
    sections: &[MarkdownSection],
    kind: OutlineSectionKind,
    chapter_number: u32,
) -> Vec<&MarkdownSection> {
    if kind == OutlineSectionKind::VolumeMap {
        if let Some(chapter_hit) = sections.iter().find(|section| {
            heading_mentions_chapter(&normalize_for_match(&section.heading), chapter_number)
        }) {
            return vec![chapter_hit];
        }
    }
    sections.iter().take(1).collect()
}

fn extract_match_terms(value: &str) -> Vec<String> {
    let normalized = normalize_for_match(value);
    let mut terms: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for m in match_word_re().find_iter(&normalized) {
        if seen.insert(m.as_str().to_string()) {
            terms.push(m.as_str().to_string());
        }
    }
    for m in match_cjk_re().find_iter(&normalized) {
        if seen.insert(m.as_str().to_string()) {
            terms.push(m.as_str().to_string());
        }
    }
    terms.into_iter().filter(|term| term.chars().count() >= 2).collect()
}

fn normalize_for_match(value: &str) -> String {
    let collapsed = match_ws_re().replace_all(value, " ");
    collapsed.trim().to_lowercase()
}

fn heading_mentions_chapter(normalized_heading: &str, chapter_number: u32) -> bool {
    normalized_heading.contains(&format!("chapter {chapter_number}"))
        || normalized_heading.contains(&format!("chapter{chapter_number}"))
        || normalized_heading.contains(&format!("ch.{chapter_number}"))
        || normalized_heading.contains(&format!("ch{chapter_number}"))
        || normalized_heading.contains(&format!("第{chapter_number}章"))
}

/// slug 化锚点（非 [a-z0-9 CJK] 折叠为 "-"；空回退 fallback）。golden 守门。
pub fn slugify_anchor(value: &str, fallback: &str) -> String {
    let lower = value.trim().to_lowercase();
    let dashed = anchor_re().replace_all(&lower, "-");
    let trimmed = anchor_edge_re().replace_all(&dashed, "");
    let result = trimmed.to_string();
    if result.is_empty() {
        fallback.to_string()
    } else {
        result
    }
}

fn dedupe_by_source(entries: Vec<ContextSource>) -> Vec<ContextSource> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter(|entry| seen.insert(entry.source.clone()))
        .collect()
}

fn outline_fallback(file_name: &str) -> Option<&'static str> {
    match file_name {
        "outline/story_frame.md" => Some("story_bible.md"),
        "outline/volume_map.md" => Some("volume_outline.md"),
        _ => None,
    }
}

fn to_fact_anchor(predicate: &str) -> String {
    slugify_anchor(predicate, "fact")
}

fn fact_entry(fact: &NewFact) -> ContextSource {
    ContextSource {
        source: format!("story/current_state.md#{}", to_fact_anchor(&fact.predicate)),
        reason: "Relevant current-state fact retrieved for the current chapter goal."
            .to_string(),
        excerpt: Some(format!("{} | {}", fact.predicate, fact.object)),
    }
}

fn summary_entry(summary: &StoredSummary) -> ContextSource {
    ContextSource {
        source: format!("story/chapter_summaries.md#{}", summary.chapter),
        reason: "Relevant episodic memory retrieved for the current chapter goal.".to_string(),
        excerpt: Some(
            [
                summary.title.as_str(),
                summary.events.as_str(),
                summary.state_changes.as_str(),
                summary.hook_activity.as_str(),
            ]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" | "),
        ),
    }
}

fn volume_summary_entry(summary: &VolumeSummarySelection) -> ContextSource {
    ContextSource {
        source: format!("story/volume_summaries.md#{}", summary.anchor),
        reason: "Carry forward long-span arc memory compressed from earlier volumes."
            .to_string(),
        excerpt: Some(format!("{} | {}", summary.heading, summary.content)),
    }
}

fn hook_entry(hook: &HookRecord) -> ContextSource {
    ContextSource {
        source: format!("story/pending_hooks.md#{}", hook.hook_id),
        reason: "Carry forward unresolved hooks that match the chapter focus.".to_string(),
        excerpt: Some(
            [
                hook.hook_type.as_str(),
                hook_status_text(hook),
                hook.expected_payoff.as_str(),
                hook.payoff_timing
                    .map(hook_payoff_timing_canonical)
                    .unwrap_or_default(),
                hook.notes.as_str(),
            ]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" | "),
        ),
    }
}

fn find_hook_summary(
    summaries: &[StoredSummary],
    hook_id: &str,
    chapter: i64,
    seed_mode: bool,
) -> Option<StoredSummary> {
    let direct_chapter_hit = summaries
        .iter()
        .find(|summary| summary.chapter == chapter)
        .cloned();
    let hook_mentions: Vec<usize> = summaries
        .iter()
        .enumerate()
        .filter(|(_, summary)| {
            [summary.title.as_str(), summary.events.as_str(), summary.state_changes.as_str(), summary.hook_activity.as_str()]
                .iter()
                .any(|text| text.contains(hook_id))
        })
        .map(|(index, _)| index)
        .collect();

    if seed_mode {
        hook_mentions
            .iter()
            .find(|&&index| summaries[index].chapter == chapter)
            .map(|&index| summaries[index].clone())
            .or_else(|| hook_mentions.first().map(|&index| summaries[index].clone()))
            .or(direct_chapter_hit)
    } else {
        hook_mentions
            .iter()
            .rev()
            .find(|&&index| summaries[index].chapter == chapter)
            .map(|&index| summaries[index].clone())
            .or_else(|| hook_mentions.last().map(|&index| summaries[index].clone()))
            .or(direct_chapter_hit)
    }
}

fn render_hook_debt_beat(summary: &StoredSummary) -> String {
    let detail = if !summary.events.is_empty() {
        &summary.events
    } else if !summary.hook_activity.is_empty() {
        &summary.hook_activity
    } else if !summary.state_changes.is_empty() {
        &summary.state_changes
    } else {
        "(none)"
    };
    format!("ch{} {} - {}", summary.chapter, summary.title, detail)
}

async fn read_file_or_default(path: &Path) -> String {
    tokio::fs::read_to_string(path)
        .await
        .unwrap_or_else(|_| MISSING_FILE.to_string())
}

// ---- 静态正则 ----

fn fence_open_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^```(?:json)?\s*").unwrap())
}

fn fence_close_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)```\s*$").unwrap())
}

fn heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(#{1,6})\s+(.+?)\s*$").unwrap())
}

fn match_word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[a-z0-9]{3,}").unwrap())
}

fn match_cjk_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\x{4e00}-\x{9fff}]{2,}").unwrap())
}

fn match_ws_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\s+").unwrap())
}

fn anchor_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[^a-z0-9\x{4e00}-\x{9fff}]+").unwrap())
}

fn anchor_edge_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^-+|-+$").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selected_sources_handles_fences_and_junk() {
        assert_eq!(
            parse_selected_sources("```json\n{\"selectedSources\":[\"a#1\",\" b \"]}\n```"),
            vec!["a#1".to_string(), " b ".to_string()]
        );
        assert_eq!(
            parse_selected_sources("前缀 {\"selectedSources\":[\"x\"]} 后缀"),
            vec!["x".to_string()]
        );
        assert!(parse_selected_sources("{\"selectedSources\":[]}").is_empty());
        assert!(parse_selected_sources("完全不是 JSON").is_empty());
        assert!(parse_selected_sources("{\"selectedSources\":\"not-array\"}").is_empty());
    }

    #[test]
    fn slugify_anchor_section_fallback() {
        assert_eq!(slugify_anchor("卷一 觉醒", "section"), "卷一-觉醒");
        assert_eq!(slugify_anchor("!!!", "section"), "section");
        assert_eq!(slugify_anchor("  A  B  ", "section"), "a-b");
    }

    #[test]
    fn split_markdown_sections_keeps_heading_lines_in_raw() {
        let md = "## 卷一\n内容甲\n\n## 卷二\n内容乙\n\n无标题尾行";
        let sections = split_markdown_sections(md);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "卷一");
        assert!(sections[0].raw.starts_with("## 卷一"));
        assert!(sections[0].raw.contains("内容甲"));
    }

    #[test]
    fn extract_last_meaningful_sentence_filters_and_truncates() {
        let content = "# 标题行会被过滤掉哦\n| 表格行也会被过滤掉哦 |\n这是一句足够长的正文句子结尾。";
        assert_eq!(
            extract_last_meaningful_sentence(content),
            Some("这是一句足够长的正文句子结尾。".to_string())
        );
        let long_tail = format!("#x\n{}", "字".repeat(80));
        let got = extract_last_meaningful_sentence(&long_tail).unwrap();
        assert_eq!(got.chars().count(), 60);
        assert!(got.ends_with("..."));
        assert_eq!(extract_last_meaningful_sentence("# 只有标题"), None);
    }

    #[test]
    fn relevance_and_fallback_selection() {
        let sections = vec![
            MarkdownSection { heading: "世界观铁律".into(), raw: "基础设定".into() },
            MarkdownSection { heading: "闲聊杂谈".into(), raw: "无关内容".into() },
            MarkdownSection { heading: "第3章 夜探".into(), raw: "本章安排".into() },
        ];
        let hints = vec!["祖符".to_string()];
        assert!(is_relevant_story_frame_section(&sections[0], &hints));
        assert!(!is_relevant_story_frame_section(&sections[1], &hints));
        assert!(is_relevant_volume_map_section(&sections[2], &hints, 3));
        // 兜底：volume-map 无章节命中 → 首段；有命中 → 命中段。
        assert_eq!(fallback_outline_sections(&sections, OutlineSectionKind::StoryFrame, 9).len(), 1);
        let hit = fallback_outline_sections(&sections, OutlineSectionKind::VolumeMap, 3);
        assert_eq!(hit[0].heading, "第3章 夜探");
    }

    #[test]
    fn heading_mentions_chapter_variants() {
        assert!(heading_mentions_chapter("chapter 12", 12));
        assert!(heading_mentions_chapter("chapter12", 12));
        assert!(heading_mentions_chapter("ch.12", 12));
        assert!(heading_mentions_chapter("ch12", 12));
        assert!(heading_mentions_chapter("第12章", 12));
        // TS includes 是子串匹配，无数值边界："chapter 121" 命中 "chapter 12"。
        assert!(heading_mentions_chapter("chapter 121", 12));
        // `第${n}章` 无空格形态：带空格的「第 3 章」不命中。
        assert!(!heading_mentions_chapter("第 3 章", 3));
    }

    #[test]
    fn outline_fallback_mapping() {
        assert_eq!(outline_fallback("outline/story_frame.md"), Some("story_bible.md"));
        assert_eq!(outline_fallback("outline/volume_map.md"), Some("volume_outline.md"));
        assert_eq!(outline_fallback("current_focus.md"), None);
    }

    struct MockCompiler {
        output: String,
        fail: bool,
    }

    #[async_trait]
    impl CompressibleContextCompiler for MockCompiler {
        async fn compile(
            &self,
            _request: CompressibleContextCompileRequest<'_>,
        ) -> Result<String, String> {
            if self.fail {
                Err("compiler boom".to_string())
            } else {
                Ok(self.output.clone())
            }
        }
    }

    fn big_package() -> ContextPackage {
        ContextPackage {
            chapter: 2,
            selected_context: vec![
                ContextSource {
                    source: "runtime/chapter_memo".into(),
                    reason: "memo".into(),
                    excerpt: Some("goal=保护条目内容".into()),
                },
                ContextSource {
                    source: "story/chapter_summaries.md#1".into(),
                    reason: "episodic".into(),
                    excerpt: Some("长".repeat(4000)),
                },
            ],
        }
    }

    #[tokio::test]
    async fn budget_passes_when_under() {
        let package = big_package();
        let out = apply_context_budget_if_needed(
            &package,
            2,
            "goal",
            WritingLanguage::Zh,
            Some(ContextBudget { context_window_tokens: 1_000_000, reserved_output_tokens: 1000 }),
            None,
            None,
        )
        .await
        .unwrap();
        assert!(out.compression.is_none());
        assert_eq!(out.context_package.selected_context.len(), 2);
    }

    #[tokio::test]
    async fn budget_compiles_and_records_trace() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let callback: CompressionCallback = Arc::new(move |event: &ContextCompressionEvent| {
            sink.lock().unwrap().push(event.phase);
        });
        let compiler = MockCompiler { output: "  编译后的紧凑上下文  ".into(), fail: false };

        let out = apply_context_budget_if_needed(
            &big_package(),
            2,
            "goal",
            WritingLanguage::Zh,
            Some(ContextBudget { context_window_tokens: 100, reserved_output_tokens: 10 }),
            Some(&compiler),
            Some(&callback),
        )
        .await
        .unwrap();

        assert_eq!(out.notes, vec!["compiled-compressible-context"]);
        let compression = out.compression.unwrap();
        assert_eq!(compression.compiled_source, "runtime/compiled-compressible-context");
        assert_eq!(compression.protected_sources, vec!["runtime/chapter_memo"]);
        assert_eq!(compression.compressed_sources, vec!["story/chapter_summaries.md#1"]);
        // 编译条目替换全部可压源。
        assert_eq!(out.context_package.selected_context.len(), 2);
        assert_eq!(
            out.context_package.selected_context[1].source,
            "runtime/compiled-compressible-context"
        );
        assert_eq!(
            out.context_package.selected_context[1].excerpt.as_deref(),
            Some("编译后的紧凑上下文")
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                ContextCompressionPhase::Start,
                ContextCompressionPhase::End
            ]
        );
    }

    #[tokio::test]
    async fn budget_errors_without_compiler() {
        let error = apply_context_budget_if_needed(
            &big_package(),
            2,
            "goal",
            WritingLanguage::Zh,
            Some(ContextBudget { context_window_tokens: 100, reserved_output_tokens: 10 }),
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ComposeChapterError::NoCompiler { .. }));
    }

    #[tokio::test]
    async fn budget_compiler_failure_propagates_with_error_event() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let callback: CompressionCallback = Arc::new(move |event: &ContextCompressionEvent| {
            sink.lock().unwrap().push(event.phase);
        });
        let compiler = MockCompiler { output: String::new(), fail: true };
        let result = apply_context_budget_if_needed(
            &big_package(),
            2,
            "goal",
            WritingLanguage::Zh,
            Some(ContextBudget { context_window_tokens: 100, reserved_output_tokens: 10 }),
            Some(&compiler),
            Some(&callback),
        )
        .await;
        assert!(matches!(result, Err(ComposeChapterError::Compiler(_))));
        assert_eq!(*events.lock().unwrap(), vec![ContextCompressionPhase::Start, ContextCompressionPhase::Error]);
    }

    #[tokio::test]
    async fn compose_full_flow_with_mocks() {
        let dir = tempfile::tempdir().unwrap();
        let story = dir.path().join("story");
        tokio::fs::create_dir_all(story.join("runtime")).await.unwrap();
        tokio::fs::create_dir_all(story.join("outline")).await.unwrap();
        tokio::fs::write(story.join("outline").join("story_frame.md"), "## 世界观铁律\n灵气体系。\n").await.unwrap();
        tokio::fs::write(story.join("current_focus.md"), "聚焦夺符").await.unwrap();
        tokio::fs::write(
            story.join("chapter_summaries.md"),
            "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n| 1 | 初入 | 林动 | 祖符觉醒 | 无 | H01 open | 平静 | 开局 |\n",
        ).await.unwrap();

        let plan = PlanChapterOutput {
            intent: crate::models::input_governance::ChapterIntent {
                chapter: 2,
                goal: "夺回祖符".into(),
                outline_node: None,
                arc_context: None,
                must_keep: vec![],
                must_avoid: vec!["禁止降智".into()],
                style_emphasis: vec![],
            },
            memo: crate::models::input_governance::ChapterMemo {
                chapter: 2,
                goal: "夺回祖符".into(),
                is_golden_opening: false,
                body: "## 当前任务\n林动夜探藏书阁夺回祖符，避开封锁线。".into(),
                thread_refs: vec![],
            },
            intent_markdown: String::new(),
            planner_inputs: vec![],
            runtime_path: dir.path().join("story/runtime/chapter-0002.intent.md"),
        };

        struct RejectingSelector;
        #[async_trait]
        impl OutlineSectionSelector for RejectingSelector {
            async fn select(
                &self,
                _request: OutlineSectionSelectionRequest<'_>,
            ) -> Result<Vec<String>, String> {
                Err("selector unavailable".to_string())
            }
        }
        let selector = RejectingSelector;

        let out = compose_governed_chapter(&ComposeChapterInput {
            book_language: Some("zh"),
            book_dir: dir.path(),
            chapter_number: 2,
            plan: &plan,
            context_budget: None,
            compiler: None,
            outline_section_selector: Some(&selector),
            reference_context_provider: None,
            memory_semantic_selector: None,
            on_context_compression: None,
        })
        .await
        .unwrap();

        let sources: Vec<&str> = out
            .context_package
            .selected_context
            .iter()
            .map(|entry| entry.source.as_str())
            .collect();
        // 顺序 load-bearing：memo → 焦点 → 大纲（选择器失败回退确定性）→ 事实/摘要…
        assert_eq!(sources.first(), Some(&"runtime/chapter_memo"));
        assert!(sources.contains(&"story/current_focus.md"));
        assert!(sources.contains(&"story/outline/story_frame.md#世界观铁律"));
        // mustAvoid 派生规则栈覆盖。
        assert_eq!(out.rule_stack.active_overrides.len(), 1);
        assert_eq!(out.rule_stack.active_overrides[0].target, "chapter:2/mustAvoid");
        // trace 记录 composerInputs = plan.runtimePath。
        assert_eq!(out.trace.composer_inputs.len(), 1);
        // 三件套落盘。
        assert!(out.context_path.exists());
        assert!(out.rule_stack_path.exists());
        assert!(out.trace_path.exists());
    }

    /// 216 号：引用选段注入——生产 provider + mock 选段器，条目接在基础
    /// 上下文之后；选段失败记 notes（进 trace.notes）。
    #[tokio::test]
    async fn compose_injects_reference_context() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 绑定素材 + 绑定清单。
        let materials = root.join(".inkos").join("materials");
        std::fs::create_dir_all(&materials).unwrap();
        std::fs::write(
            materials.join("mat1.json"),
            serde_json::json!({
                "id": "mat1", "title": "开篇参考", "kind": "text", "purpose": "reference",
                "source": "upload", "mimeType": "text/markdown",
                "markdownPath": ".inkos/materials/mat1.md",
                "manifestPath": ".inkos/materials/mat1.json",
                "charCount": 50, "excerpt": "..."
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            materials.join("mat1.md"),
            "## Extracted content\n\n## 人物关系\n\n师徒线张力。\n",
        )
        .unwrap();
        let book_dir = root.join("books").join("b1");
        std::fs::create_dir_all(&book_dir).unwrap();
        crate::references::bind_book_reference(
            root,
            "b1",
            &crate::references::BindBookReferenceInput {
                material_id: "mat1",
                uses: &["人物关系".to_string()],
                note: None,
            },
        )
        .unwrap();

        // story 目录最小面（基础上下文可空）。
        let story = book_dir.join("story");
        tokio::fs::create_dir_all(&story).await.unwrap();
        tokio::fs::write(story.join("current_focus.md"), "聚焦夺符").await.unwrap();

        let plan = PlanChapterOutput {
            intent: crate::models::input_governance::ChapterIntent {
                chapter: 2,
                goal: "夺回祖符".into(),
                outline_node: None,
                arc_context: None,
                must_keep: vec![],
                must_avoid: vec![],
                style_emphasis: vec![],
            },
            memo: crate::models::input_governance::ChapterMemo {
                chapter: 2,
                goal: "夺回祖符".into(),
                is_golden_opening: false,
                body: "## 当前任务\n夺符。".into(),
                thread_refs: vec![],
            },
            intent_markdown: String::new(),
            planner_inputs: vec![],
            runtime_path: dir.path().join("story/runtime/chapter-0002.intent.md"),
        };

        struct ScriptedSelector {
            result: Result<Vec<String>, String>,
        }
        #[async_trait]
        impl crate::references::ReferenceSectionSelector for ScriptedSelector {
            async fn select(
                &self,
                _request: crate::references::ReferenceSectionSelectionRequest<'_>,
            ) -> Result<Vec<String>, String> {
                self.result.clone()
            }
        }
        let selector = ScriptedSelector { result: Ok(vec!["reference/mat1#人物关系".to_string()]) };
        let provider = crate::references::ProductionReferenceContextProvider {
            project_root: root,
            book_id: "b1",
            selector: &selector,
        };

        let out = compose_governed_chapter(&ComposeChapterInput {
            book_language: Some("zh"),
            book_dir: &book_dir,
            chapter_number: 2,
            plan: &plan,
            context_budget: None,
            compiler: None,
            outline_section_selector: None,
            reference_context_provider: Some(&provider),
            memory_semantic_selector: None,
            on_context_compression: None,
        })
        .await
        .unwrap();

        let sources: Vec<&str> = out
            .context_package
            .selected_context
            .iter()
            .map(|entry| entry.source.as_str())
            .collect();
        // 条目接在基础上下文之后（TS [...base, ...reference]）。
        assert_eq!(sources.last(), Some(&"reference/mat1#人物关系"));
        let entry = out.context_package.selected_context.last().unwrap();
        assert_eq!(entry.excerpt.as_deref(), Some("## 人物关系\n\n师徒线张力。"));
        assert!(entry.reason.contains("User-bound reference \"开篇参考\" for: 人物关系."));
        assert!(out.trace.notes.is_empty(), "无失败时 notes 为空");

        // 选段失败 → 无条目 + book-reference-selection-failed 进 trace.notes。
        let failing = ScriptedSelector { result: Err("llm down".to_string()) };
        let provider = crate::references::ProductionReferenceContextProvider {
            project_root: root,
            book_id: "b1",
            selector: &failing,
        };
        let out = compose_governed_chapter(&ComposeChapterInput {
            book_language: Some("zh"),
            book_dir: &book_dir,
            chapter_number: 2,
            plan: &plan,
            context_budget: None,
            compiler: None,
            outline_section_selector: None,
            reference_context_provider: Some(&provider),
            memory_semantic_selector: None,
            on_context_compression: None,
        })
        .await
        .unwrap();
        assert!(!out
            .context_package
            .selected_context
            .iter()
            .any(|entry| entry.source.starts_with("reference/")));
        assert!(out.trace.notes.contains(&"book-reference-selection-failed".to_string()));
    }
}
