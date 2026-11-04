//! 短篇生产纯函数层（887 号 M3a）：`packages/core/src/agents/short-fiction.ts`
//! 纯函数面 + 契约 + `pipeline/short-production-state.ts` 状态机——**LLM-free**，
//! 全部可金标准锁。
//!
//! ## 与旧形态的处置关系（886 号勘察判定）
//! `crate::agents::short_fiction`（78 号）是旧 tagged 链活体（旧 runner 引用中，
//! 不动）。本模块为 2.0 形态并排落位，**重名语义函数 = 形态替换非复用**：
//! - `format_chapter_heading`：旧版空标题回退+前缀检测；2.0 空标题**抛错**、纯格式化。
//! - `render_draft_markdown`：旧版渲染全部章节；2.0 **跳过未完成章节**
//!   （title+content 双非空才算 completed）。
//! - `validate_short_fiction_draft_for_final`：2.0 新增 U+FFFD 腐化检测 +
//!   title/openingHook 契约 + min/max 语义 options。
//!
//! 计数底座复用 [`crate::utils::length_metrics`]（strip_markdown_metadata 逐条对齐）。
//!
//! serde 契约对齐：zod/TypeBox strict ↔ deny_unknown_fields；optional 字段
//! skip_serializing_if 与 TS JSON.stringify 的 undefined 消失语义互逆。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::models::observation::Observation;
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{count_chapter_length, resolve_length_counting_mode};

// ── 常量（agents/short-fiction.ts L28-30 + runner L61-62）─────────────

pub const SHORT_FICTION_DEFAULT_CHAPTERS: u32 = 12;
pub const SHORT_FICTION_DEFAULT_CHARS_PER_CHAPTER: u32 = 1000;
pub const SHORT_FICTION_EN_DEFAULT_WORDS_PER_CHAPTER: u32 = 650;
/// runner `SHORT_FICTION_DRAFT_COMPLETION_ATTEMPTS`。
pub const SHORT_FICTION_DRAFT_COMPLETION_ATTEMPTS: u32 = 3;

/// 交付合同观察码集合（runner `SHORT_DELIVERY_CONTRACT_CODES`）。
pub const SHORT_DELIVERY_CONTRACT_CODES: &[&str] = &[
    "SHORT_CHAPTER_CONTRACT",
    "SHORT_TITLE_MISMATCH",
    "SHORT_OPENING_HOOK_CONTRACT",
];

// ── 契约 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortFictionChapter {
    pub number: u32,
    pub title: String,
    pub content: String,
    #[serde(rename = "charCount")]
    pub char_count: u32,
}

/// zod `ShortFictionBatchDraftSchema`（strict）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortFictionBatchDraft {
    #[serde(rename = "storyTitle")]
    pub story_title: String,
    #[serde(rename = "openingHook", skip_serializing_if = "Option::is_none")]
    pub opening_hook: Option<String>,
    pub chapters: Vec<ShortFictionChapter>,
    #[serde(rename = "rawContent")]
    pub raw_content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortFictionSalesPackage {
    pub title: String,
    pub intro: String,
    #[serde(rename = "sellingPoints")]
    pub selling_points: Vec<String>,
    #[serde(rename = "coverPrompt")]
    pub cover_prompt: String,
    #[serde(rename = "rawContent")]
    pub raw_content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShortFictionLengthUnit {
    #[serde(rename = "non-whitespace-characters")]
    NonWhitespaceCharacters,
    #[serde(rename = "words")]
    Words,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortFictionChapterLength {
    pub number: u32,
    pub length: u32,
}

/// TS `ShortFictionMeasurements` 接口（结构化落盘面——delivery.measurements）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortFictionMeasurements {
    pub title: String,
    #[serde(rename = "chapterCount")]
    pub chapter_count: u32,
    #[serde(rename = "totalLength")]
    pub total_length: u32,
    pub unit: ShortFictionLengthUnit,
    #[serde(rename = "openingHookLength")]
    pub opening_hook_length: u32,
    #[serde(rename = "chapterLengths")]
    pub chapter_lengths: Vec<ShortFictionChapterLength>,
}

/// TS `ShortAuthorScopeSchema`（TypeBox additionalProperties:false）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortAuthorScope {
    #[serde(rename = "wholeManuscript")]
    pub whole_manuscript: bool,
    #[serde(rename = "storyTitle", skip_serializing_if = "Option::is_none")]
    pub story_title: Option<bool>,
    #[serde(rename = "chapterNumbers")]
    pub chapter_numbers: Vec<u32>,
    #[serde(rename = "chapterTitleNumbers", skip_serializing_if = "Option::is_none")]
    pub chapter_title_numbers: Option<Vec<u32>>,
    pub opening: bool,
    pub outline: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortRevisionPlanChapter {
    pub number: u32,
    #[serde(rename = "sourceNumber", skip_serializing_if = "Option::is_none")]
    pub source_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
}

/// TS `ShortRevisionPlanSchema`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortRevisionPlan {
    #[serde(rename = "storyTitle", skip_serializing_if = "Option::is_none")]
    pub story_title: Option<String>,
    #[serde(rename = "revisionBrief")]
    pub revision_brief: String,
    #[serde(rename = "outlineMarkdown", skip_serializing_if = "Option::is_none")]
    pub outline_markdown: Option<String>,
    #[serde(rename = "openingHook", skip_serializing_if = "Option::is_none")]
    pub opening_hook: Option<String>,
    pub chapters: Vec<ShortRevisionPlanChapter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShortStageStatus {
    Completed,
    Failed,
}

/// 修订对照面（runner comparison：episode_start | parent_revision）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShortComparisonScope {
    #[serde(rename = "episode_start")]
    EpisodeStart,
    #[serde(rename = "parent_revision")]
    ParentRevision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortArtifactAddress {
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    #[serde(rename = "revisionId")]
    pub revision_id: String,
    pub checksum: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortStageComparison {
    pub scope: ShortComparisonScope,
    pub before: ShortArtifactAddress,
    pub after: ShortArtifactAddress,
}

/// zod `ShortStageSchema`（strict）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortStage {
    pub status: ShortStageStatus,
    #[serde(rename = "inputHash")]
    pub input_hash: String,
    #[serde(rename = "requestHash", skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(rename = "reviewHash", skip_serializing_if = "Option::is_none")]
    pub review_hash: Option<String>,
    #[serde(rename = "reviewedArtifact", skip_serializing_if = "Option::is_none")]
    pub reviewed_artifact: Option<ShortStageReviewedArtifact>,
    #[serde(rename = "reviewedReference", skip_serializing_if = "Option::is_none")]
    pub reviewed_reference: Option<ShortStageReviewedReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<ShortStageComparison>,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub observations: Vec<Observation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortStageReviewedArtifact {
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    #[serde(rename = "revisionId")]
    pub revision_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortStageReviewedReference {
    #[serde(rename = "workId")]
    pub work_id: String,
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    #[serde(rename = "revisionId")]
    pub revision_id: String,
    pub checksum: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShortDeliveryStatus {
    #[serde(rename = "checks_passed")]
    ChecksPassed,
    #[serde(rename = "needs_revision")]
    NeedsRevision,
    #[serde(rename = "unverified")]
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortDelivery {
    pub status: ShortDeliveryStatus,
    #[serde(rename = "inputHash")]
    pub input_hash: String,
    pub observations: Vec<Observation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measurements: Option<ShortFictionMeasurements>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortProductionTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "chapterCount")]
    pub chapter_count: u32,
    #[serde(rename = "charsPerChapter")]
    pub chars_per_chapter: u32,
    #[serde(rename = "minChapterLength", skip_serializing_if = "Option::is_none")]
    pub min_chapter_length: Option<u32>,
    #[serde(rename = "maxChapterLength", skip_serializing_if = "Option::is_none")]
    pub max_chapter_length: Option<u32>,
    #[serde(rename = "openingHookChars", skip_serializing_if = "Option::is_none")]
    pub opening_hook_chars: Option<u32>,
    #[serde(rename = "coverRequired", skip_serializing_if = "Option::is_none")]
    pub cover_required: Option<bool>,
    pub language: WritingLanguage,
}

/// zod `ShortProductionStateSchema`（strict；stages 三键恒在对象）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortProductionState {
    pub version: u32,
    pub intent: String,
    #[serde(rename = "outlineSource", skip_serializing_if = "Option::is_none")]
    pub outline_source: Option<ShortStageReviewedReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ShortProductionTarget>,
    #[serde(rename = "revisionRequest", skip_serializing_if = "Option::is_none")]
    pub revision_request: Option<String>,
    #[serde(rename = "reviewScope", skip_serializing_if = "Option::is_none")]
    pub review_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<ShortDelivery>,
    pub stages: ShortProductionStages,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortProductionStages {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<ShortStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<ShortStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<ShortStage>,
}

/// zod version literal(2)——contracts.validate 惯例外补（profile_registry 同款）。
pub fn validate_short_production_state(state: &ShortProductionState) -> Result<(), String> {
    if state.version != 2 {
        return Err("state version must be 2".into());
    }
    for observation in state.stages.review.iter().chain(state.stages.package.iter()).chain(state.stages.cover.iter())
        .flat_map(|stage| stage.observations.iter())
        .chain(state.delivery.iter().flat_map(|delivery| delivery.observations.iter()))
    {
        observation.validate()?;
    }
    Ok(())
}

// ── 错误（TS Object.assign(new Error, {code, …}) 的 Rust 形态）────────

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ShortFictionError {
    pub code: &'static str,
    pub message: String,
    /// SHORT_CHAPTER_INCOMPLETE / SHORT_CHAPTER_TEXT_CORRUPTED 等携带章号。
    pub chapters: Vec<u32>,
    pub details: Option<Value>,
}

impl ShortFictionError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), chapters: Vec::new(), details: None }
    }

    fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    fn with_chapters(mut self, chapters: Vec<u32>) -> Self {
        self.chapters = chapters;
        self
    }

    /// TS `throw Object.assign(new Error(JSON.stringify(details)), details)` 面：
    /// message = details JSON 文本（错误消息面与 TS 同形）。
    fn from_details(code: &'static str, details: Value) -> Self {
        Self {
            code,
            message: details.to_string(),
            chapters: details
                .get("chapters")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_u64).map(|n| n as u32).collect())
                .unwrap_or_default(),
            details: Some(details),
        }
    }
}

// ── 度量与渲染 ────────────────────────────────────────────────────────

/// TS `measureShortFictionDraft`。
pub fn measure_short_fiction_draft(
    draft: &ShortFictionBatchDraft,
    language: WritingLanguage,
) -> ShortFictionMeasurements {
    let mode = resolve_length_counting_mode(language);
    let chapter_lengths: Vec<ShortFictionChapterLength> = draft
        .chapters
        .iter()
        .map(|chapter| ShortFictionChapterLength {
            number: chapter.number,
            length: count_chapter_length(&chapter.content, mode),
        })
        .collect();
    let opening_hook_length =
        count_chapter_length(draft.opening_hook.as_deref().unwrap_or(""), mode);
    let total_length = chapter_lengths.iter().map(|item| item.length).sum::<u32>() + opening_hook_length;
    ShortFictionMeasurements {
        title: draft.story_title.clone(),
        chapter_count: draft.chapters.len() as u32,
        total_length,
        unit: if language == WritingLanguage::En {
            ShortFictionLengthUnit::Words
        } else {
            ShortFictionLengthUnit::NonWhitespaceCharacters
        },
        opening_hook_length,
        chapter_lengths,
    }
}

/// TS `renderShortFictionSalesPackage`。
pub fn render_short_fiction_sales_package(
    sales: &ShortFictionSalesPackage,
    language: WritingLanguage,
) -> String {
    let headings = if language == WritingLanguage::En {
        ["## Synopsis", "## Selling Points", "## Cover Prompt"]
    } else {
        ["## 简介", "## 卖点", "## 封面提示词"]
    };
    let mut lines: Vec<String> = vec![
        format!("# {}", sales.title),
        String::new(),
        headings[0].to_string(),
        String::new(),
        sales.intro.clone(),
        String::new(),
        headings[1].to_string(),
        String::new(),
    ];
    for point in &sales.selling_points {
        lines.push(format!("- {point}"));
    }
    lines.push(String::new());
    lines.push(headings[2].to_string());
    lines.push(String::new());
    lines.push(sales.cover_prompt.clone());
    lines.join("\n")
}

/// TS `formatShortFictionChapterHeading`（2.0 形态：空标题抛错，无前缀检测）。
pub fn format_short_fiction_chapter_heading(
    number: u32,
    title: &str,
    language: WritingLanguage,
) -> Result<String, ShortFictionError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(ShortFictionError::new(
            "SHORT_CHAPTER_INCOMPLETE",
            format!("Short-fiction chapter {number} has no title."),
        )
        .with_chapters(vec![number]));
    }
    Ok(if language == WritingLanguage::En {
        format!("Chapter {number}: {trimmed}")
    } else {
        format!("第{number}章 {trimmed}")
    })
}

/// TS `renderShortFictionDraftMarkdown`（2.0 形态：跳过未完成章节）。
pub fn render_short_fiction_draft_markdown(
    draft: &ShortFictionBatchDraft,
    language: WritingLanguage,
) -> Result<String, ShortFictionError> {
    let hook_heading = if language == WritingLanguage::En { "## Opening Hook" } else { "## 开篇钩子" };
    let mut parts: Vec<String> = Vec::with_capacity(2 + draft.chapters.len());
    parts.push(format!("# {}", draft.story_title));
    if let Some(hook) = &draft.opening_hook {
        parts.push(format!("{hook_heading}\n\n{hook}"));
    }
    for chapter in &draft.chapters {
        if chapter.title.trim().is_empty() || chapter.content.trim().is_empty() {
            continue;
        }
        parts.push(format!(
            "## {}\n\n{}",
            format_short_fiction_chapter_heading(chapter.number, &chapter.title, language)?,
            chapter.content
        ));
    }
    Ok(parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n\n"))
}

// ── 完整性与合同校验 ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct ShortDraftBounds<'a> {
    pub title: Option<&'a str>,
    pub opening_hook_chars: Option<u32>,
    pub expected_chapters: Option<u32>,
    pub min_chapter_length: Option<u32>,
    pub max_chapter_length: Option<u32>,
    pub language: Option<WritingLanguage>,
}

/// TS `openingHookLengthContract`：minimum=floor(target*0.75)、
/// maximum=ceil(target*1.25)（u64 中转防溢出）。
pub fn opening_hook_length_contract(target: u32, language: WritingLanguage) -> (u32, u32) {
    let _ = language;
    let wide = target as u64;
    let minimum = std::cmp::max(1u64, (wide * 3) / 4);
    let maximum = (wide * 5).div_ceil(4); // ceil(target*1.25)
    (minimum.min(u32::MAX as u64) as u32, maximum.min(u32::MAX as u64) as u32)
}

/// TS `validateOpeningHook`。
pub fn validate_opening_hook(
    hook: Option<&str>,
    target: u32,
    language: WritingLanguage,
) -> Result<(), ShortFictionError> {
    let mode = resolve_length_counting_mode(language);
    let length = count_chapter_length(hook.unwrap_or(""), mode);
    let (minimum, maximum) = opening_hook_length_contract(target, language);
    if length < minimum || length > maximum {
        return Err(ShortFictionError::from_details(
            "SHORT_OPENING_HOOK_CONTRACT",
            serde_json::json!({
                "code": "SHORT_OPENING_HOOK_CONTRACT",
                "length": length,
                "minimum": minimum,
                "maximum": maximum,
                "instruction": "Submit an independent opening scene within the requested length, keeping chapter one complete.",
            }),
        ));
    }
    Ok(())
}

/// TS `findIncompleteShortFictionChapters`。
pub fn find_incomplete_short_fiction_chapters(
    draft: &ShortFictionBatchDraft,
    bounds: &ShortDraftBounds<'_>,
) -> Vec<u32> {
    let language = bounds.language.unwrap_or(WritingLanguage::Zh);
    let mode = resolve_length_counting_mode(language);
    draft
        .chapters
        .iter()
        .filter(|chapter| {
            let length = count_chapter_length(&chapter.content, mode);
            chapter.title.trim().is_empty()
                || chapter.content.contains('\u{FFFD}')
                || length < bounds.min_chapter_length.unwrap_or(1)
                || bounds.max_chapter_length.map(|max| length > max).unwrap_or(false)
        })
        .map(|chapter| chapter.number)
        .collect()
}

/// TS `findEmptyShortFictionChapters`（无 bounds 别名）。
pub fn find_empty_short_fiction_chapters(draft: &ShortFictionBatchDraft) -> Vec<u32> {
    find_incomplete_short_fiction_chapters(draft, &ShortDraftBounds::default())
}

/// TS `validateShortFictionDraftForFinal`（2.0 形态）。
pub fn validate_short_fiction_draft_for_final(
    draft: &ShortFictionBatchDraft,
    bounds: &ShortDraftBounds<'_>,
) -> Result<(), ShortFictionError> {
    if let Some(title) = bounds.title {
        if draft.story_title != title {
            return Err(ShortFictionError::from_details(
                "SHORT_TITLE_MISMATCH",
                serde_json::json!({
                    "code": "SHORT_TITLE_MISMATCH",
                    "expectedTitle": title,
                    "actualTitle": draft.story_title,
                }),
            )
            .with_message("The manuscript title differs from the requested title"));
        }
    }
    if let Some(target) = bounds.opening_hook_chars {
        validate_opening_hook(
            draft.opening_hook.as_deref(),
            target,
            bounds.language.unwrap_or(WritingLanguage::Zh),
        )?;
    }
    let corrupted: Vec<u32> = draft
        .chapters
        .iter()
        .filter(|chapter| chapter.content.contains('\u{FFFD}'))
        .map(|chapter| chapter.number)
        .collect();
    if !corrupted.is_empty() {
        return Err(ShortFictionError::new(
            "SHORT_CHAPTER_TEXT_CORRUPTED",
            format!(
                "Chapter text contains Unicode replacement characters: {}",
                corrupted.iter().map(|number| number.to_string()).collect::<Vec<_>>().join(", ")
            ),
        )
        .with_chapters(corrupted));
    }
    if let Some(expected) = bounds.expected_chapters {
        if draft.chapters.len() as u32 != expected {
            return Err(ShortFictionError::new(
                "SHORT_CHAPTER_INCOMPLETE",
                format!(
                    "Short-hit draft is incomplete; expected {expected} chapters, got {}.",
                    draft.chapters.len()
                ),
            ));
        }
    }
    let invalid = find_incomplete_short_fiction_chapters(draft, bounds);
    if !invalid.is_empty() {
        let details = invalid
            .iter()
            .map(|number| {
                let chapter = draft.chapters.iter().find(|item| &item.number == number);
                format!("{number} ({})", chapter.map(|item| item.char_count).unwrap_or(0))
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ShortFictionError::new(
            "SHORT_CHAPTER_INCOMPLETE",
            format!("Short-fiction draft is incomplete; insufficient chapters: {details}."),
        )
        .with_chapters(invalid));
    }
    Ok(())
}

// ── 批次与预算 ────────────────────────────────────────────────────────

/// TS `safeShortFictionOutputBudget`。
pub fn safe_short_fiction_output_budget(model_max_output: f64) -> u32 {
    let usable = if model_max_output.is_finite() && model_max_output > 0.0 {
        model_max_output.floor() as u64
    } else {
        12_288
    };
    std::cmp::max(1, std::cmp::min(usable, 24_576) as u32)
}

/// TS `estimateShortFictionMaxTokens`（2.2 系数标定注释见 TS L802-804）。
pub fn estimate_short_fiction_max_tokens(
    chapter_count: u32,
    chars_per_chapter: u32,
    model_max_output: f64,
) -> u32 {
    let requested = std::cmp::max(
        4096u32,
        (chapter_count as u64 * chars_per_chapter as u64 * 22).div_ceil(10) as u32 + 2048,
    );
    std::cmp::min(requested, safe_short_fiction_output_budget(model_max_output))
}

/// TS `allChapterNumbers`。
pub fn all_chapter_numbers(chapter_count: u32) -> Vec<u32> {
    (1..=chapter_count).collect()
}

/// TS `buildShortFictionChapterBatches`。
pub fn build_short_fiction_chapter_batches(
    chapter_numbers: &[u32],
    chars_per_chapter: u32,
    model_max_output: f64,
    max_chapters_per_call: Option<u32>,
) -> Vec<Vec<u32>> {
    let budget = safe_short_fiction_output_budget(model_max_output);
    let per_chapter = std::cmp::max(1u64, (chars_per_chapter as u64 * 22).div_ceil(10));
    let batch_size = std::cmp::max(
        1usize,
        std::cmp::min(
            max_chapters_per_call.unwrap_or(chapter_numbers.len() as u32) as usize,
            ((budget.saturating_sub(2048)) as u64 / per_chapter.max(1)) as usize,
        ),
    );
    let mut normalized: Vec<u32> = chapter_numbers
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|number| *number > 0)
        .collect();
    normalized.sort_unstable();
    normalized.chunks(batch_size).map(|chunk| chunk.to_vec()).collect()
}

// ── 批归一与校验（M3b structured 提交面的数据归一半）─────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ShortDraftChapterSubmission {
    pub number: u32,
    pub title: String,
    pub content: String,
}

/// TS `mergeShortFictionBatch`。
pub fn merge_short_fiction_batch(
    current: Option<&ShortFictionBatchDraft>,
    story_title: &str,
    opening_hook: Option<&str>,
    submitted: &[ShortDraftChapterSubmission],
    expected_chapters: u32,
    language: WritingLanguage,
) -> Result<ShortFictionBatchDraft, ShortFictionError> {
    let counting_mode = resolve_length_counting_mode(language);
    let mut by_number: BTreeMap<u32, ShortFictionChapter> = current
        .map(|draft| {
            draft
                .chapters
                .iter()
                .map(|chapter| (chapter.number, chapter.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    for chapter in submitted {
        if chapter.number == 0 || chapter.number > expected_chapters {
            return Err(ShortFictionError::new(
                "SHORT_BATCH_INVALID",
                format!(
                    "Short-fiction batch returned invalid chapter number {}.",
                    chapter.number
                ),
            ));
        }
        if !seen.insert(chapter.number) {
            return Err(ShortFictionError::new(
                "SHORT_BATCH_INVALID",
                format!("Short-fiction batch returned duplicate chapter {}.", chapter.number),
            ));
        }
        let content = chapter.content.trim();
        let title = chapter.title.trim();
        if content.is_empty() || title.is_empty() {
            return Err(ShortFictionError::new(
                "SHORT_BATCH_INVALID",
                format!("Short-fiction batch returned empty chapter {}.", chapter.number),
            ));
        }
        by_number.insert(
            chapter.number,
            ShortFictionChapter {
                number: chapter.number,
                title: title.to_string(),
                content: content.to_string(),
                char_count: count_chapter_length(content, counting_mode),
            },
        );
    }
    let story_title = current
        .map(|draft| draft.story_title.clone())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| story_title.trim().to_string());
    if story_title.is_empty() {
        return Err(ShortFictionError::new(
            "SHORT_BATCH_INVALID",
            "Short-fiction batch returned an empty story title.",
        ));
    }
    // 章节批次只拥有章文本；全局开篇变更属显式整稿修订计划（TS 注释同源）。
    // TS：`current ? current.openingHook : batch.openingHook?.trim()`——current
    // 存在即不回退 batch（即使其 openingHook 为 undefined）。
    let opening_hook = match current {
        Some(draft) => draft.opening_hook.clone(),
        None => opening_hook
            .map(str::trim)
            .filter(|hook| !hook.is_empty())
            .map(str::to_string),
    };
    let chapters: Vec<ShortFictionChapter> = (1..=expected_chapters)
        .map(|number| {
            by_number.remove(&number).unwrap_or(ShortFictionChapter {
                number,
                title: String::new(),
                content: String::new(),
                char_count: 0,
            })
        })
        .collect();
    let draft = ShortFictionBatchDraft {
        story_title,
        opening_hook,
        chapters,
        raw_content: String::new(),
    };
    let mut draft = draft;
    draft.raw_content = render_short_fiction_draft_markdown(&draft, language)?;
    Ok(draft)
}

/// TS `shortReviewSources`（M3b sourced review 的源清单构造）。
pub fn short_review_sources(
    outline_markdown: &str,
    draft: &ShortFictionBatchDraft,
    reference_text: Option<&str>,
    comparison: Option<(&ShortFictionBatchDraft,)>,
) -> BTreeMap<String, String> {
    let mut sources = BTreeMap::new();
    sources.insert("outline".to_string(), outline_markdown.to_string());
    sources.insert("manuscript-title".to_string(), draft.story_title.clone());
    if let Some(hook) = &draft.opening_hook {
        sources.insert("manuscript-opening".to_string(), hook.clone());
    }
    for chapter in &draft.chapters {
        sources.insert(
            format!("manuscript-chapter-{}", chapter.number),
            format!("{}\n{}", chapter.title, chapter.content),
        );
    }
    if let Some(text) = reference_text.filter(|text| !text.trim().is_empty()) {
        sources.insert("reference-material".to_string(), text.to_string());
    }
    if let Some((before,)) = comparison {
        sources.insert("baseline-manuscript-title".to_string(), before.story_title.clone());
        if let Some(hook) = &before.opening_hook {
            sources.insert("baseline-manuscript-opening".to_string(), hook.clone());
        }
        for chapter in &before.chapters {
            let current = draft.chapters.iter().find(|item| item.number == chapter.number);
            if current.map(|item| item.title != chapter.title || item.content != chapter.content).unwrap_or(true) {
                sources.insert(
                    format!("baseline-manuscript-chapter-{}", chapter.number),
                    format!("{}\n{}", chapter.title, chapter.content),
                );
            }
        }
    }
    sources
}

/// TS `assertShortAuthorScope`（short-revision-scope.ts）。
pub fn assert_short_author_scope(
    scope: &ShortAuthorScope,
    before: &ShortFictionBatchDraft,
    after: &ShortFictionBatchDraft,
    before_outline: &str,
    after_outline: &str,
) -> Result<(), ShortFictionError> {
    if !scope.story_title.unwrap_or(false) && before.story_title != after.story_title {
        return Err(ShortFictionError::from_details(
            "SHORT_REVISION_OUT_OF_SCOPE",
            serde_json::json!({
                "code": "SHORT_REVISION_OUT_OF_SCOPE",
                "changedChapters": [],
                "changedParts": ["title"],
            }),
        )
        .with_message("The book title is outside the author-authorized scope."));
    }
    if scope.whole_manuscript {
        return Ok(());
    }
    let changed_chapters: Vec<u32> = before
        .chapters
        .iter()
        .filter(|chapter| {
            let next = after.chapters.iter().find(|item| item.number == chapter.number);
            match next {
                None => true,
                Some(next) => {
                    (!scope.chapter_title_numbers.as_ref().map(|list| list.contains(&chapter.number)).unwrap_or(false)
                        && next.title != chapter.title)
                        || (!scope.chapter_numbers.contains(&chapter.number) && next.content != chapter.content)
                }
            }
        })
        .map(|chapter| chapter.number)
        .collect();
    let mut changed_parts: Vec<&str> = Vec::new();
    if !scope.opening && before.opening_hook.clone().unwrap_or_default() != after.opening_hook.clone().unwrap_or_default() {
        changed_parts.push("opening");
    }
    if !scope.outline && before_outline != after_outline {
        changed_parts.push("outline");
    }
    if before.chapters.len() != after.chapters.len() {
        changed_parts.push("chapterCount");
    }
    if !changed_chapters.is_empty() || !changed_parts.is_empty() {
        return Err(ShortFictionError::from_details(
            "SHORT_REVISION_OUT_OF_SCOPE",
            serde_json::json!({
                "code": "SHORT_REVISION_OUT_OF_SCOPE",
                "changedChapters": changed_chapters,
                "changedParts": changed_parts,
            }),
        )
        .with_message("The revision changes protected manuscript parts."));
    }
    Ok(())
}

// ── 状态机（pipeline/short-production-state.ts）──────────────────────

/// TS `Value.Check(ShortPackageToolSchema, document)` 等价：title/intro/
/// coverPrompt 非空字符串 + sellingPoints 数组 ≥1 每项非空字符串。TypeBox
/// 无 additionalProperties 限制——**额外键放行**（sales-package.json 落盘
/// 保留 rawContent 等额外键的前提）。
pub fn check_short_package_tool(value: &Value) -> Result<(), String> {
    let map = value.as_object().ok_or("Invalid short package")?;
    for field in ["title", "intro", "coverPrompt"] {
        let text = map
            .get(field)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| format!("Invalid short package: {field}"))?;
        let _ = text;
    }
    let selling_points = map
        .get("sellingPoints")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or("Invalid short package: sellingPoints")?;
    if selling_points
        .iter()
        .any(|item| item.as_str().map(str::is_empty).unwrap_or(true))
    {
        return Err("Invalid short package: sellingPoints item".into());
    }
    Ok(())
}

/// 从 Value（可能携带 rawContent 等额外键）读四字段渲染 sales markdown
///（TS `renderShortFictionSalesPackage(sales, language)` 的 document 面包装）。
pub fn render_sales_package_from_value(
    value: &Value,
    language: WritingLanguage,
) -> Result<String, String> {
    let map = value.as_object().ok_or("Invalid short package")?;
    let read = |field: &str| -> Result<String, String> {
        map.get(field)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("Invalid short package: {field}"))
    };
    let sales = ShortFictionSalesPackage {
        title: read("title")?,
        intro: read("intro")?,
        selling_points: map
            .get("sellingPoints")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .ok_or("Invalid short package: sellingPoints")?,
        cover_prompt: read("coverPrompt")?,
        raw_content: String::new(),
    };
    Ok(render_short_fiction_sales_package(&sales, language))
}

/// TS `shortInputHash`：canonical（键递归排序）→ JSON 文本 → sha256。
/// serde Value 默认 BTreeMap=键字母序（ASCII 与 TS localeCompare 同序）。
pub fn short_input_hash(value: &Value) -> String {
    let canonical = canonicalize(value);
    let text = serde_json::to_string(&canonical).unwrap_or_else(|_| "null".to_string());
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    format!("sha256:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        Value::Object(map) => {
            // serde_json Map（BTreeMap 底座）重插即键排序（canonical 语义）。
            let mut sorted = serde_json::Map::new();
            for (key, item) in map {
                sorted.insert(key.clone(), canonicalize(item));
            }
            Value::Object(sorted)
        }
        other => other.clone(),
    }
}

/// TS `readShortProductionState`：ENOENT→None，parse 失败上抛。
pub fn read_short_production_state(
    root: &std::path::Path,
    base_dir: &std::path::Path,
) -> Result<Option<ShortProductionState>, String> {
    let path = root.join(base_dir).join("production-state.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let state: ShortProductionState =
        serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    validate_short_production_state(&state)?;
    Ok(Some(state))
}

/// TS `writeShortProductionState`：parse 后 stringify(…,null,2)+"\n" 原子提交。
pub async fn write_short_production_state(
    root: &std::path::Path,
    base_dir: &std::path::Path,
    state: &ShortProductionState,
) -> Result<(), String> {
    validate_short_production_state(state)?;
    let mut text = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
    text.push('\n');
    crate::utils::atomic_file_set::commit_atomic_file_set(&crate::utils::atomic_file_set::AtomicFileSet {
        root_dir: root,
        writes: vec![crate::utils::atomic_file_set::AtomicFileWrite {
            relative_path: format!(
                "{}/production-state.json",
                base_dir.to_string_lossy().replace('\\', "/")
            ),
            content: crate::utils::atomic_file_set::FileContent::Text(text),
        }],
        deletes: Vec::new(),
    })
    .await
    .map_err(|error| error.to_string())
}

// ── 测试（金标准与 TS vitest 逐字共用）───────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_draft() -> ShortFictionBatchDraft {
        serde_json::from_str(
            r#"{
  "storyTitle": "雨夜碎镜",
  "openingHook": "雨夜来客叩门。",
  "chapters": [
    { "number": 1, "title": "换名", "content": "斋主以镜换名，来客留下一个再未使用的名字。", "charCount": 20 },
    { "number": 2, "title": "窥影", "content": "镜中人影与来客同貌，交易自此刻开始。", "charCount": 18 }
  ],
  "rawContent": ""
}"#,
        )
        .unwrap()
    }

    #[test]
    fn golden_batch_draft_round_trips_strictly() {
        let draft = sample_draft();
        let written = serde_json::to_value(&draft).unwrap();
        let reparsed: ShortFictionBatchDraft = serde_json::from_value(written).unwrap();
        assert_eq!(reparsed, draft);
        // strict 拒未知键（zod strict 对齐）
        let bad = r#"{"storyTitle":"x","chapters":[],"rawContent":"","sneaky":1}"#;
        assert!(serde_json::from_str::<ShortFictionBatchDraft>(bad).is_err());
    }

    #[test]
    fn golden_render_markdown_matches_ts_shared_string() {
        let draft = sample_draft();
        assert_eq!(
            render_short_fiction_draft_markdown(&draft, WritingLanguage::Zh).unwrap(),
            GOLDEN_RENDERED_MARKDOWN,
            "渲染金标准与 TS vitest 逐字共用"
        );
    }

    #[test]
    fn golden_measurements_match_ts_shared_values() {
        let draft = sample_draft();
        let measurements = measure_short_fiction_draft(&draft, WritingLanguage::Zh);
        assert_eq!(measurements.chapter_count, 2);
        assert_eq!(measurements.unit, ShortFictionLengthUnit::NonWhitespaceCharacters);
        assert_eq!(measurements.opening_hook_length, 7);
        assert_eq!(measurements.chapter_lengths.len(), 2);
        assert_eq!(measurements.total_length, measurements.opening_hook_length
            + measurements.chapter_lengths.iter().map(|item| item.length).sum::<u32>());
    }

    #[test]
    fn corrupted_and_incomplete_chapters_are_detected() {
        let mut draft = sample_draft();
        draft.chapters[0].content = "含替换字符\u{FFFD}的正文".to_string();
        let error = validate_short_fiction_draft_for_final(&draft, &ShortDraftBounds::default())
            .unwrap_err();
        assert_eq!(error.code, "SHORT_CHAPTER_TEXT_CORRUPTED");
        assert_eq!(error.chapters, vec![1]);

        let mut draft = sample_draft();
        draft.chapters[1].content = String::new();
        let error = validate_short_fiction_draft_for_final(&draft, &ShortDraftBounds::default())
            .unwrap_err();
        assert_eq!(error.code, "SHORT_CHAPTER_INCOMPLETE");
        assert_eq!(error.chapters, vec![2]);
    }

    #[test]
    fn incomplete_chapters_are_skipped_in_render() {
        // 2.0 形态差：未完成章节不渲染（旧形态渲染标题行——886 处置表实证）。
        let mut draft = sample_draft();
        draft.chapters[1].title = String::new();
        let rendered = render_short_fiction_draft_markdown(&draft, WritingLanguage::Zh).unwrap();
        assert!(!rendered.contains("窥影"), "未完成章不渲染：{rendered}");
        assert!(rendered.contains("换名"));
    }

    #[test]
    fn chapter_batches_respect_budget_and_dedup() {
        // budget=24576-2048=22536；per=1000*2.2=2200 → batch=10（floor 22536/2200=10）；
        // 输入含重复 2 → 去重后 12 章。
        let batches = build_short_fiction_chapter_batches(
            &[12, 3, 1, 2, 2, 5, 8, 9, 10, 11, 4, 6, 7],
            1000,
            24576.0,
            None,
        );
        assert_eq!(batches, vec![vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10], vec![11, 12]]);
        let capped = build_short_fiction_chapter_batches(&[1, 2, 3, 4], 1000, 24576.0, Some(2));
        assert_eq!(capped, vec![vec![1, 2], vec![3, 4]]);
    }

    #[test]
    fn opening_hook_contract_bounds() {
        assert!(validate_opening_hook(Some(&"x".repeat(100)), 100, WritingLanguage::Zh).is_ok());
        let error = validate_opening_hook(Some(&"x".repeat(50)), 100, WritingLanguage::Zh).unwrap_err();
        assert_eq!(error.code, "SHORT_OPENING_HOOK_CONTRACT");
        assert_eq!(error.details.as_ref().unwrap()["minimum"], 75);
        assert_eq!(error.details.as_ref().unwrap()["maximum"], 125);
    }

    #[test]
    fn sales_package_renders_zh_headings() {
        let sales = ShortFictionSalesPackage {
            title: "雨夜碎镜".into(),
            intro: "一个以镜换名的故事。".into(),
            selling_points: vec!["短".into(), "冷".into()],
            cover_prompt: "碎镜中的雨夜。".into(),
            raw_content: String::new(),
        };
        let rendered = render_short_fiction_sales_package(&sales, WritingLanguage::Zh);
        assert!(rendered.starts_with("# 雨夜碎镜\n\n## 简介\n"));
        assert!(rendered.contains("## 卖点\n\n- 短\n- 冷"));
        assert!(rendered.ends_with("## 封面提示词\n\n碎镜中的雨夜。"));
    }

    #[test]
    fn author_scope_assert_rejects_protected_changes() {
        let before = sample_draft();
        let mut after = sample_draft();
        after.story_title = "新标题".into();
        let scope = ShortAuthorScope {
            whole_manuscript: false,
            story_title: None,
            chapter_numbers: vec![1, 2],
            chapter_title_numbers: None,
            opening: false,
            outline: false,
        };
        let error = assert_short_author_scope(&scope, &before, &after, "", "").unwrap_err();
        assert_eq!(error.code, "SHORT_REVISION_OUT_OF_SCOPE");
        assert_eq!(error.details.as_ref().unwrap()["changedParts"], serde_json::json!(["title"]));
        // 整稿授权仍不含改名（TS：title 检查在 wholeManuscript 早退之前）——
        // 须显式 storyTitle 授权才放行。
        let scope = ShortAuthorScope { whole_manuscript: true, ..scope };
        assert!(assert_short_author_scope(&scope, &before, &after, "", "").is_err());
        let scope = ShortAuthorScope { story_title: Some(true), ..scope };
        assert!(assert_short_author_scope(&scope, &before, &after, "", "").is_ok());
    }

    #[test]
    fn state_input_hash_is_key_order_insensitive() {
        let a = serde_json::json!({ "b": 1, "a": { "y": 2, "x": [1, 2] } });
        let b = serde_json::json!({ "a": { "x": [1, 2], "y": 2 }, "b": 1 });
        assert_eq!(short_input_hash(&a), short_input_hash(&b));
        let c = serde_json::json!({ "b": 2, "a": { "y": 2, "x": [1, 2] } });
        assert_ne!(short_input_hash(&a), short_input_hash(&c));
    }

    #[tokio::test]
    async fn production_state_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let state = ShortProductionState {
            version: 2,
            intent: "雨夜来客的故事".into(),
            outline_source: None,
            target: Some(ShortProductionTarget {
                title: Some("雨夜碎镜".into()),
                chapter_count: 2,
                chars_per_chapter: 1000,
                min_chapter_length: Some(1),
                max_chapter_length: None,
                opening_hook_chars: None,
                cover_required: Some(true),
                language: WritingLanguage::Zh,
            }),
            revision_request: None,
            review_scope: Some("whole-story".into()),
            delivery: None,
            stages: ShortProductionStages::default(),
        };
        write_short_production_state(root, std::path::Path::new("works/x"), &state)
            .await
            .unwrap();
        let loaded = read_short_production_state(root, std::path::Path::new("works/x"))
            .unwrap()
            .unwrap();
        assert_eq!(loaded, state);
        let raw = std::fs::read_to_string(root.join("works/x/production-state.json")).unwrap();
        assert!(raw.ends_with('\n'), "TS stringify+\\n 同款尾换行");
        // 缺文件 → None
        assert!(read_short_production_state(root, std::path::Path::new("works/ghost"))
            .unwrap()
            .is_none());
    }

    /// 金标准渲染文本（与 TS vitest short-fiction-cross-engine.test.ts 逐字共用）。
    const GOLDEN_RENDERED_MARKDOWN: &str = "# 雨夜碎镜\n\n## 开篇钩子\n\n雨夜来客叩门。\n\n## 第1章 换名\n\n斋主以镜换名，来客留下一个再未使用的名字。\n\n## 第2章 窥影\n\n镜中人影与来客同貌，交易自此刻开始。";
}
