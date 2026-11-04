//! 短篇四代理面（889 号 M3c）：`packages/core/src/agents/short-fiction.ts` 的
//! outline/packaging/reviewer 三代理 + writer 主链（writeDraft 批次循环）+
//! 提交工具 schema 构造族——全部经 [`super::structured_agent`] 基座环。
//!
//! ## M3c 范围裁定（886 §三 M3c 拆 889/890）
//! - 本轮：outline/packaging/reviewer 全量 + writer writeDraft 主干 +
//!   validateRequestedShortChapter/asShortDraftBatch 归一。
//! - 890 轮：writer continueDraft/reviseDraft/applyRevisionPlan（依赖 ReviserAgent
//!   新形态链——reviser.ts 252 行 + source-locator/author-permission 闭包家族）。
//! - **备案**：TS submitStructured 的 appendTaskSkillGuidance（authorRequest 前缀
//!   +技能 guidance+上下文编译）不落基座——runner 层（M3d）装配 runtimes 时以
//!   system 消息显式传入（等价注入点在 messages 构造侧）。

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use super::short_fiction::{
    ShortFictionBatchDraft, ShortFictionChapter, ShortFictionSalesPackage,
};
use super::short_fiction_prompts::{
    self, DraftPromptInput, DraftReviewPromptInput, OutlinePromptInput, PackagePromptInput,
};
use super::structured_agent::{
    run_structured_tool, SupportingToolSpec, StructuredAgentError, StructuredDefaults,
    StructuredRunOptions, ValidateFn, WorkerToolSpec,
};
use crate::llm::provider::LLMMessage;
use crate::models::observation::{
    resolve_observation_sources, Observation, ReviewObservationSubmission,
};
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{count_chapter_length, resolve_length_counting_mode};

/// TS `AgentContext` 的基座等价面（client+model+defaults；技能/上下文编译注入
/// 点在 messages 构造侧——见模块头备案）。
#[derive(Clone)]
pub struct ShortAgents {
    pub chat: super::structured_agent::ChatTurnFn,
    pub model: String,
    pub defaults: StructuredDefaults,
}

// ── 提交工具 schema 构造（short-fiction-tool.ts 的 JSON Schema 形态）──

/// TS `shortOutlineToolSchema(chapterCount, title?)`。
pub fn short_outline_tool_schema(chapter_count: u32, title: Option<&str>) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert(
        "storyTitle".into(),
        match title.map(str::trim).filter(|title| !title.is_empty()) {
            Some(title) => serde_json::json!({ "const": title }),
            None => serde_json::json!({ "type": "string", "minLength": 1 }),
        },
    );
    properties.insert(
        "planMarkdown".into(),
        serde_json::json!({
            "type": "string", "minLength": 1,
            "description": "Shared premise, protagonist and central conflict. Do not repeat the chapter plans here.",
        }),
    );
    for index in 1..=chapter_count {
        properties.insert(
            format!("chapter_{index}_plan"),
            serde_json::json!({
                "type": "string", "minLength": 1,
                "description": format!(
                    "Chapter {index}: a concise complete scene plan, about 150–250 Chinese characters or 80–120 English words. State the action, conflict, causal evidence, payoff and transition; keep later chapters' events in their own plans."
                ),
            }),
        );
    }
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": properties.keys().cloned().collect::<Vec<_>>(),
        "additionalProperties": false,
    })
}

/// TS `ShortDraftBatchToolSchema`（无 additionalProperties 限制——TypeBox 默认）。
pub fn short_draft_batch_tool_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "storyTitle": { "type": "string", "minLength": 1 },
            "openingHook": { "type": "string" },
            "chapters": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "number": { "type": "integer", "minimum": 1 },
                        "title": { "type": "string", "minLength": 1, "description": "Chapter title only; do not include a chapter number or heading prefix." },
                        "content": { "type": "string", "minLength": 1 },
                    },
                    "required": ["number", "title", "content"],
                },
            },
        },
        "required": ["storyTitle", "chapters"],
    })
}

/// TS `ShortDraftChapterToolSchema`。
pub fn short_draft_chapter_tool_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "storyTitle": { "type": "string", "minLength": 1 },
            "openingHook": { "type": "string" },
            "number": { "type": "integer", "minimum": 1 },
            "title": { "type": "string", "minLength": 1, "description": "Chapter title without a number or heading prefix." },
            "content": { "type": "string", "minLength": 1, "description": "Complete prose of the requested chapter." },
        },
        "required": ["storyTitle", "number", "title", "content"],
    })
}

/// TS `ShortRevisionChapterToolSchema`（title+content，additionalProperties:false）。
pub fn short_revision_chapter_tool_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "minLength": 1, "description": "Chapter title without a number or heading prefix." },
            "content": { "type": "string", "minLength": 1, "description": "Complete prose of the requested chapter." },
        },
        "required": ["title", "content"],
        "additionalProperties": false,
    })
}

/// TS `shortDraftBatchToolSchema(chapterNumbers, options)`（扁平字段形态）。
pub fn short_flat_batch_tool_schema(
    chapter_numbers: &[u32],
    title: Option<&str>,
    opening_hook_chars: Option<u32>,
) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert(
        "storyTitle".into(),
        match title.map(str::trim).filter(|title| !title.is_empty()) {
            Some(title) => serde_json::json!({ "const": title }),
            None => serde_json::json!({ "type": "string", "minLength": 1 }),
        },
    );
    properties.insert(
        "openingHook".into(),
        if opening_hook_chars
            .map(|_chars| chapter_numbers.contains(&1))
            .unwrap_or(false)
        {
            let chars = opening_hook_chars.unwrap_or_default();
            serde_json::json!({
                "type": "string", "minLength": 1,
                "description": format!(
                    "Independent opening scene, about {chars} native language units, before chapter one. Keep the complete first chapter separately."
                ),
            })
        } else {
            serde_json::json!({ "type": "string" })
        },
    );
    for number in chapter_numbers {
        properties.insert(
            format!("chapter_{number}_title"),
            serde_json::json!({
                "type": "string", "minLength": 1,
                "description": format!("Chapter {number} title, without numbering."),
            }),
        );
        properties.insert(
            format!("chapter_{number}_content"),
            serde_json::json!({
                "type": "string", "minLength": 1,
                "description": format!("Complete chapter {number} prose. Submit the text directly, not a JSON document."),
            }),
        );
    }
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": ["storyTitle"],
        "additionalProperties": false,
    })
}

/// TS `ShortPackageToolSchema`。
pub fn short_package_tool_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "minLength": 1 },
            "intro": { "type": "string", "minLength": 1 },
            "sellingPoints": { "type": "array", "items": { "type": "string", "minLength": 1 }, "minItems": 1 },
            "coverPrompt": { "type": "string", "minLength": 1 },
        },
        "required": ["title", "intro", "sellingPoints", "coverPrompt"],
    })
}

// ── OutlineAgent ─────────────────────────────────────────────────────

pub struct OutlineInput<'a> {
    pub title: Option<&'a str>,
    pub direction: &'a str,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub reference_text: Option<&'a str>,
}

/// TS `ShortFictionOutlineAgent.createOutline`：结构化提交 → rawContent 拼接
///（planMarkdown + 逐章 `## 第N章` 标题段）。
pub async fn outline_create_outline(
    agents: &ShortAgents,
    input: &OutlineInput<'_>,
    language: WritingLanguage,
) -> Result<ShortFictionOutlineResult, StructuredAgentError> {
    let system = short_fiction_prompts::build_short_fiction_outline_system_prompt(language);
    let user = short_fiction_prompts::build_short_fiction_outline_user_prompt(
        &OutlinePromptInput {
            title: input.title,
            direction: input.direction,
            chapter_count: input.chapter_count,
            chars_per_chapter: input.chars_per_chapter,
            reference: None,
        },
        language,
    );
    let messages = vec![llm_system(&system), llm_user(&user)];
    // TS maxTokens = min(8192, defaults.maxTokens)。
    let max_tokens = std::cmp::min(8192, agents.defaults.max_tokens);
    let schema = short_outline_tool_schema(input.chapter_count, input.title);
    let outcome = run_structured_tool(
        Arc::clone(&agents.chat),
        &agents.model,
        &messages,
        agents.defaults.clone(),
        WorkerToolSpec {
            name: "submit_short_outline".into(),
            description: "Submit the story title and complete readable plan.".into(),
            parameters_schema: schema,
            max_turns: None,
        },
        None,
        Vec::new(),
        StructuredRunOptions { temperature: None, max_tokens: Some(max_tokens) },
    )
    .await?;
    let result = &outcome.result;
    let story_title = result
        .get("storyTitle")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let plan_markdown = result
        .get("planMarkdown")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let mut sections = vec![plan_markdown.to_string()];
    for index in 1..=input.chapter_count {
        let heading = if language == WritingLanguage::En {
            format!("## Chapter {index}")
        } else {
            format!("## 第{index}章")
        };
        let plan = result
            .get(format!("chapter_{index}_plan"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        sections.push(format!("{heading}\n\n{plan}"));
    }
    Ok(ShortFictionOutlineResult {
        story_title,
        raw_content: sections.join("\n\n"),
        usage: outcome.usage,
    })
}

pub struct ShortFictionOutlineResult {
    pub story_title: String,
    pub raw_content: String,
    pub usage: super::structured_agent::ChatTurnUsage,
}

// ── PackagingAgent ───────────────────────────────────────────────────

pub struct PackageInput<'a> {
    pub direction: &'a str,
    pub draft: &'a ShortFictionBatchDraft,
    pub language: WritingLanguage,
}

/// TS `ShortFictionPackagingAgent.generatePackage`。
pub async fn packaging_generate(
    agents: &ShortAgents,
    input: &PackageInput<'_>,
) -> Result<ShortFictionSalesPackage, StructuredAgentError> {
    let system =
        short_fiction_prompts::build_short_fiction_package_system_prompt(input.language);
    let draft_markdown = super::short_fiction::render_short_fiction_draft_markdown(
        input.draft,
        input.language,
    )
    .map_err(|error| StructuredAgentError {
        code: "WORKER_DOMAIN_INVALID".into(),
        message: error.to_string(),
        attempts: None,
        result_tool: Some("submit_short_package".into()),
    })?;
    let user = short_fiction_prompts::build_short_fiction_package_user_prompt(
        &PackagePromptInput {
            direction: input.direction,
            draft_markdown: &draft_markdown,
            draft_title: &input.draft.story_title,
        },
        input.language,
    );
    let messages = vec![llm_system(&system), llm_user(&user)];
    let max_tokens = std::cmp::min(4096, agents.defaults.max_tokens);
    let outcome = run_structured_tool(
        Arc::clone(&agents.chat),
        &agents.model,
        &messages,
        agents.defaults.clone(),
        WorkerToolSpec {
            name: "submit_short_package".into(),
            description: "Submit title, synopsis, selling points, and cover prompt.".into(),
            parameters_schema: short_package_tool_schema(),
            max_turns: None,
        },
        None,
        Vec::new(),
        StructuredRunOptions { temperature: None, max_tokens: Some(max_tokens) },
    )
    .await?;
    let result = &outcome.result;
    let title = result.get("title").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    let intro = result.get("intro").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    let selling_points: Vec<String> = result
        .get("sellingPoints")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::trim).map(str::to_string).collect())
        .unwrap_or_default();
    let cover_prompt = result
        .get("coverPrompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if title.is_empty() || intro.is_empty() || selling_points.iter().any(|point| point.is_empty()) {
        return Err(StructuredAgentError {
            code: "WORKER_DOMAIN_INVALID".into(),
            message: "Short-fiction packaging returned incomplete structured fields.".into(),
            attempts: Some(outcome.turns),
            result_tool: Some("submit_short_package".into()),
        });
    }
    // TS rawContent 拼接：`# 标题` + `## Intro\n…` + 卖点列表 + `## Cover Prompt\n…`
    let raw_content = [
        format!("# {title}"),
        format!("## Intro\n{intro}"),
        format!(
            "## Selling Points\n{}",
            selling_points.iter().map(|point| format!("- {point}")).collect::<Vec<_>>().join("\n")
        ),
        format!("## Cover Prompt\n{cover_prompt}"),
    ]
    .join("\n\n");
    Ok(ShortFictionSalesPackage {
        title: title.to_string(),
        intro: intro.to_string(),
        selling_points,
        cover_prompt: cover_prompt.to_string(),
        raw_content,
    })
}

// ── WriterAgent：writeDraft 主干 ─────────────────────────────────────

pub struct WriteDraftInput<'a> {
    pub title: Option<&'a str>,
    pub opening_hook_chars: Option<u32>,
    pub direction: &'a str,
    pub outline_markdown: &'a str,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub min_chapter_length: Option<u32>,
    pub max_chapter_length: Option<u32>,
    pub max_chapters_per_call: Option<u32>,
    pub chapter_numbers: Option<Vec<u32>>,
    pub language: WritingLanguage,
}

pub struct WriteDraftOutcome {
    pub draft: ShortFictionBatchDraft,
    pub usage: super::structured_agent::ChatTurnUsage,
}

/// TS `asShortDraftBatch`：batch/chapter/flat 三形态 → chapters 数组。
fn as_short_draft_batch(
    result: &Value,
    numbers: Option<&[u32]>,
) -> Result<(String, Option<String>, Vec<ShortFictionChapter>), String> {
    if result.get("chapters").and_then(Value::as_array).is_some() {
        let chapters = result
            .get("chapters")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|chapter| ShortFictionChapter {
                        number: chapter.get("number").and_then(Value::as_u64).unwrap_or(0) as u32,
                        title: chapter.get("title").and_then(Value::as_str).unwrap_or_default().to_string(),
                        content: chapter.get("content").and_then(Value::as_str).unwrap_or_default().to_string(),
                        char_count: 0,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return Ok((
            result.get("storyTitle").and_then(Value::as_str).unwrap_or_default().to_string(),
            result.get("openingHook").and_then(Value::as_str).map(str::to_string),
            chapters,
        ));
    }
    if result.get("number").is_some() {
        return Ok((
            result.get("storyTitle").and_then(Value::as_str).unwrap_or_default().to_string(),
            result.get("openingHook").and_then(Value::as_str).map(str::to_string),
            vec![ShortFictionChapter {
                number: result.get("number").and_then(Value::as_u64).unwrap_or(0) as u32,
                title: result.get("title").and_then(Value::as_str).unwrap_or_default().to_string(),
                content: result.get("content").and_then(Value::as_str).unwrap_or_default().to_string(),
                char_count: 0,
            }],
        ));
    }
    let Some(numbers) = numbers else {
        return Err("Flat batch submission requires its requested chapter numbers".into());
    };
    let chapters = numbers
        .iter()
        .map(|number| ShortFictionChapter {
            number: *number,
            title: result
                .get(format!("chapter_{number}_title"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            content: result
                .get(format!("chapter_{number}_content"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            char_count: 0,
        })
        .collect();
    Ok((
        result.get("storyTitle").and_then(Value::as_str).unwrap_or_default().to_string(),
        result.get("openingHook").and_then(Value::as_str).map(str::to_string),
        chapters,
    ))
}

/// TS `validateRequestedShortChapter`：标题精确/章号面/长度合同（enforce）。
#[allow(clippy::too_many_arguments)]
fn validate_requested_short_chapter(
    result: &Value,
    numbers: &[u32],
    input: &WriteDraftInput<'_>,
    enforce_length: bool,
) -> Result<(String, Option<String>, Vec<ShortFictionChapter>), StructuredAgentError> {
    let fail = |code: &'static str, message: String| StructuredAgentError {
        code: code.into(),
        message,
        attempts: None,
        result_tool: Some("short-fiction writer".into()),
    };
    if let Some(title) = input.title {
        let submitted = result.get("storyTitle").and_then(Value::as_str).unwrap_or_default().trim();
        if submitted != title.trim() {
            return Err(fail(
                "SHORT_TITLE_MISMATCH",
                "Submit the requested story title exactly".into(),
            ));
        }
    }
    let (story_title, opening_hook, chapters) =
        as_short_draft_batch(result, Some(numbers)).map_err(|message| fail("SHORT_BATCH_INVALID", message))?;
    if chapters.len() != numbers.len()
        || chapters.iter().any(|chapter| !numbers.contains(&chapter.number))
        || chapters.iter().map(|chapter| chapter.number).collect::<std::collections::BTreeSet<_>>().len()
            != chapters.len()
    {
        return Err(fail(
            "SHORT_REVISION_OUT_OF_SCOPE",
            format!("Submit only chapters {}", numbers.iter().map(|number| number.to_string()).collect::<Vec<_>>().join(", ")),
        ));
    }
    if enforce_length {
        let mode = resolve_length_counting_mode(input.language);
        if let Some(min) = input.min_chapter_length {
            let short: Vec<u32> = chapters
                .iter()
                .filter(|chapter| count_chapter_length(&chapter.content, mode) < min)
                .map(|chapter| chapter.number)
                .collect();
            if !short.is_empty() {
                return Err(fail(
                    "SHORT_CHAPTER_TOO_SHORT",
                    serde_json::to_string(&serde_json::json!({
                        "code": "SHORT_CHAPTER_TOO_SHORT",
                        "minChapterLength": min,
                        "chapters": short,
                        "instruction": "Expand the requested scene within its chapter boundary. Preserve the events assigned to neighboring chapters.",
                    }))
                    .unwrap_or_default(),
                ));
            }
        }
        if let Some(max) = input.max_chapter_length {
            let overlong: Vec<u32> = chapters
                .iter()
                .filter(|chapter| count_chapter_length(&chapter.content, mode) > max)
                .map(|chapter| chapter.number)
                .collect();
            if !overlong.is_empty() {
                return Err(fail(
                    "SHORT_CHAPTER_TOO_LONG",
                    serde_json::to_string(&serde_json::json!({
                        "code": "SHORT_CHAPTER_TOO_LONG",
                        "maxChapterLength": max,
                        "chapters": overlong,
                        "instruction": "Submit complete scenes within their own chapter plan and length limit.",
                    }))
                    .unwrap_or_default(),
                ));
            }
        }
    }
    Ok((story_title, opening_hook, chapters))
}

/// TS `WriterAgent.writeDraft`：批次循环 + 每批 merge + onBatchComplete。
pub async fn writer_write_draft(
    agents: &ShortAgents,
    input: &WriteDraftInput<'_>,
    mut on_batch_complete: impl FnMut(&ShortFictionBatchDraft, &[u32]),
) -> Result<WriteDraftOutcome, StructuredAgentError> {
    let chapter_numbers: Vec<u32> = input
        .chapter_numbers
        .clone()
        .unwrap_or_else(|| super::short_fiction::all_chapter_numbers(input.chapter_count));
    // TS outputBudget：非流式且未限批 → min(8192, defaults)；否则 defaults 全额。
    let output_budget = std::cmp::min(8192, agents.defaults.max_tokens);
    let batches = super::short_fiction::build_short_fiction_chapter_batches(
        &chapter_numbers,
        input.chars_per_chapter,
        output_budget as f64,
        input.max_chapters_per_call,
    );
    let system = short_fiction_prompts::build_short_fiction_writer_system_prompt(input.language);
    let mut current: Option<ShortFictionBatchDraft> = None;
    let mut usage_total = super::structured_agent::ChatTurnUsage::default();

    for chapter_batch in &batches {
        let user = short_fiction_prompts::build_short_fiction_writer_user_prompt(
            &DraftPromptInput {
                title: input.title,
                opening_hook_chars: input.opening_hook_chars,
                direction: input.direction,
                outline_markdown: input.outline_markdown,
                chapter_count: input.chapter_count,
                chars_per_chapter: input.chars_per_chapter,
                max_chapter_length: input.max_chapter_length,
                min_chapter_length: input.min_chapter_length,
                chapter_numbers: Some(chapter_batch),
                previous_draft_markdown: current.as_ref().map(|draft| draft.raw_content.as_str()),
            },
            input.language,
        );
        let messages = vec![llm_system(&system), llm_user(&user)];
        let schema = if chapter_batch.len() == 1 {
            short_draft_chapter_tool_schema()
        } else {
            short_flat_batch_tool_schema(chapter_batch, input.title, input.opening_hook_chars)
        };
        let tool_name = if chapter_batch.len() == 1 { "submit_short_chapter" } else { "submit_short_draft_batch" };
        // validate 闭包捕获取字段所有权快照（WriteDraftInput 含引用不可整体克隆）。
        let batch_numbers = chapter_batch.clone();
        let title_owned = input.title.map(str::to_string);
        let min_owned = input.min_chapter_length;
        let max_owned = input.max_chapter_length;
        let language = input.language;
        let validate: ValidateFn = Arc::new(move |value: Value| {
            let batch_numbers = batch_numbers.clone();
            let title_view = title_owned.clone();
            Box::pin(async move {
                // 轻量 input 视图（validate 面只需 title/min/max/language）。
                let input_view = WriteDraftInput {
                    title: title_view.as_deref(),
                    opening_hook_chars: None,
                    direction: "",
                    outline_markdown: "",
                    chapter_count: batch_numbers.len() as u32,
                    chars_per_chapter: 0,
                    min_chapter_length: min_owned,
                    max_chapter_length: max_owned,
                    max_chapters_per_call: None,
                    chapter_numbers: None,
                    language,
                };
                let (story_title, opening_hook, chapters) = validate_requested_short_chapter(
                    &value,
                    &batch_numbers,
                    &input_view,
                    false,
                )
                .map_err(|error| super::structured_agent::StructuredToolError {
                    code: Some(error.code),
                    message: error.message,
                })?;
                let payload = serde_json::json!({
                    "storyTitle": story_title,
                    "openingHook": opening_hook,
                    "chapters": chapters.iter().map(|chapter| serde_json::json!({
                        "number": chapter.number, "title": chapter.title, "content": chapter.content,
                    })).collect::<Vec<_>>(),
                });
                Ok(payload)
            })
        });
        let max_tokens = std::cmp::min(
            8192u32.saturating_sub(1) + 1,
            super::short_fiction::estimate_short_fiction_max_tokens(
                chapter_batch.len() as u32,
                input.chars_per_chapter,
                output_budget as f64,
            ),
        );
        let outcome = run_structured_tool(
            Arc::clone(&agents.chat),
            &agents.model,
            &messages,
            agents.defaults.clone(),
            WorkerToolSpec {
                name: tool_name.into(),
                description: "Submit the requested complete chapter drafts.".into(),
                parameters_schema: schema,
                max_turns: None,
            },
            Some(validate),
            Vec::new(),
            StructuredRunOptions { temperature: None, max_tokens: Some(max_tokens) },
        )
        .await?;
        usage_total.prompt_tokens += outcome.usage.prompt_tokens;
        usage_total.completion_tokens += outcome.usage.completion_tokens;
        usage_total.total_tokens += outcome.usage.total_tokens;
        let (story_title, opening_hook, chapters) =
            as_short_draft_batch(&outcome.result, Some(chapter_batch)).map_err(|message| {
                StructuredAgentError {
                    code: "SHORT_BATCH_INVALID".into(),
                    message,
                    attempts: Some(outcome.turns),
                    result_tool: Some(tool_name.into()),
                }
            })?;
        let merged = super::short_fiction::merge_short_fiction_batch(
            current.as_ref(),
            &story_title,
            opening_hook.as_deref(),
            &chapters
                .into_iter()
                .map(|chapter| super::short_fiction::ShortDraftChapterSubmission {
                    number: chapter.number,
                    title: chapter.title,
                    content: chapter.content,
                })
                .collect::<Vec<_>>(),
            input.chapter_count,
            input.language,
        )
        .map_err(|error| StructuredAgentError {
            code: error.code.into(),
            message: error.message,
            attempts: Some(outcome.turns),
            result_tool: Some(tool_name.into()),
        })?;
        current = Some(merged);
        let completed: Vec<u32> = current
            .as_ref()
            .map(|draft| {
                super::short_fiction::find_incomplete_short_fiction_chapters(
                    draft,
                    &super::short_fiction::ShortDraftBounds {
                        min_chapter_length: input.min_chapter_length,
                        max_chapter_length: input.max_chapter_length,
                        language: Some(input.language),
                        ..super::short_fiction::ShortDraftBounds::default()
                    },
                )
            })
            .unwrap_or_default();
        let complete_numbers: Vec<u32> = current
            .as_ref()
            .map(|draft| {
                draft
                    .chapters
                    .iter()
                    .filter(|chapter| !completed.contains(&chapter.number))
                    .map(|chapter| chapter.number)
                    .collect()
            })
            .unwrap_or_default();
        if let Some(draft) = &current {
            on_batch_complete(draft, &complete_numbers);
        }
    }
    let Some(draft) = current else {
        return Err(StructuredAgentError {
            code: "WORKER_DOMAIN_INVALID".into(),
            message: "Short-fiction writer returned no chapter batch".into(),
            attempts: None,
            result_tool: Some("short-fiction writer".into()),
        });
    };
    Ok(WriteDraftOutcome { draft, usage: usage_total })
}

// ── ReviewerAgent：submitSourcedReview 等价 ──────────────────────────

pub struct ReviewDraftInput<'a> {
    pub direction: &'a str,
    pub outline_markdown: &'a str,
    pub draft: &'a ShortFictionBatchDraft,
    pub reference_text: Option<&'a str>,
    pub revision_request: Option<&'a str>,
    pub review_scope: Option<&'a str>,
    pub measurements: Option<&'a super::short_fiction::ShortFictionMeasurements>,
}

#[derive(Debug)]
pub struct ReviewOutcome {
    pub summary: String,
    pub observations: Vec<Observation>,
    pub usage: super::structured_agent::ChatTurnUsage,
}

/// TS `submitSourcedReview`（BaseAgent L85-136）等价：record_review_observation
/// supporting（宿主截取原文回执）+ maxTurns 16 + unknown-codes validate + 三条
/// 追加 system 消息（逐字金标准）。`reviewScope`/comparison 面由 prompts 层承载。
#[allow(clippy::too_many_lines)]
pub async fn reviewer_review_draft(
    agents: &ShortAgents,
    input: &ReviewDraftInput<'_>,
    language: WritingLanguage,
) -> Result<ReviewOutcome, StructuredAgentError> {
    let sources: BTreeMap<String, String> = super::short_fiction::short_review_sources(
        input.outline_markdown,
        input.draft,
        input.reference_text,
        None,
    );
    let user = short_fiction_prompts::build_short_fiction_draft_review_user_prompt(
        &DraftReviewPromptInput {
            base: DraftPromptInput {
                title: None,
                opening_hook_chars: None,
                direction: input.direction,
                outline_markdown: input.outline_markdown,
                chapter_count: input.draft.chapters.len() as u32,
                chars_per_chapter: 0,
                max_chapter_length: None,
                min_chapter_length: None,
                chapter_numbers: None,
                previous_draft_markdown: None,
            },
            draft_markdown: &input.draft.raw_content,
            reference: None,
            revision_request: input.revision_request,
            review_scope: input.review_scope,
            measurements: input.measurements,
            comparison: None,
        },
        language,
    );
    // TS 三条追加 system 消息（base.ts L102-104 逐字）。
    let mut messages = vec![
        llm_system(&short_fiction_prompts::build_short_fiction_draft_review_system_prompt(language)),
        llm_user(&user),
        llm_system(
            "The original author request and confirmed constraints define the acceptance requirements. Prior critiques and delegated revision suggestions are claims to recheck, not evidence that a defect exists or additional author requirements. Assess the current text independently. Explain why an issue conflicts with the author goals or the supplied text; classify a compatible interpretation or stylistic alternative as an observation. Baseline text establishes what changed, not what is still present in the current draft.",
        ),
        llm_system(
            "Before recording an arithmetic finding, use calculate_review_values and read its result. Submit independent calculations together. Verify the source values, units and time periods first; a correct calculation does not establish that unlike quantities should be compared.",
        ),
        llm_system(
            "Record each finding with record_review_observation. Independent findings may be submitted together in one response. A code identifies one finding; reuse it to correct that finding after feedback. Read the exact source excerpts returned by the tool: resolving an address does not establish that its text supports the finding. Correct the references, assessment or explanation when the excerpts do not establish your claim. In a later response, call submit_short_fiction_review with the review summary and supported finding codes in the desired order. Do not embed a list of findings in a string. Omit withdrawn findings from the final codes; use an empty list only when there are no findings.",
        ),
    ];
    let _ = &mut messages;

    let recorded: Arc<Mutex<BTreeMap<String, ReviewObservationSubmission>>> =
        Arc::new(Mutex::new(BTreeMap::new()));
    let sources_arc = Arc::new(sources.clone());
    let recorded_for_support = Arc::clone(&recorded);
    let supporting = vec![SupportingToolSpec {
        name: "record_review_observation".into(),
        description: "Record an assessed finding after checking its reasoning and any arithmetic. This tool validates source addresses and returns their excerpts; it does not verify whether the claim is true. Resubmitting the same code replaces that finding.".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "minLength": 1 },
                "summary": { "type": "string" },
                "category": { "enum": ["execution", "quality", "scope"] },
                "assessment": { "enum": ["issue", "resolved", "unavailable", "observation"] },
                "scope": { "type": "string" },
                "target": {
                    "type": "object",
                    "properties": {
                        "workId": { "type": "string" },
                        "artifactId": { "type": "string" },
                        "revisionId": { "type": "string" },
                    },
                    "required": ["workId", "artifactId", "revisionId"],
                    "additionalProperties": false,
                },
                "sourceRefs": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "sourceId": { "type": "string", "minLength": 1 },
                            "startLine": { "type": "integer", "minimum": 1 },
                            "endLine": { "type": "integer", "minimum": 1 },
                        },
                        "required": ["sourceId", "startLine", "endLine"],
                        "additionalProperties": false,
                    },
                },
            },
            "required": ["code", "summary", "sourceRefs"],
            "additionalProperties": false,
        }),
        execute: Arc::new(move |arguments: Value| {
            let recorded = Arc::clone(&recorded_for_support);
            let sources = Arc::clone(&sources_arc);
            Box::pin(async move {
                let submission: ReviewObservationSubmission =
                    serde_json::from_value(arguments).map_err(|error| {
                        super::structured_agent::StructuredToolError::coded(
                            "WORKER_SCHEMA_INVALID",
                            error.to_string(),
                        )
                    })?;
                let resolved = resolve_observation_sources(std::slice::from_ref(&submission), &sources)
                    .map_err(|error| {
                        super::structured_agent::StructuredToolError::coded(&error.code, error.message)
                    })?;
                let receipt = serde_json::json!({
                    "code": submission.code,
                    "status": "source_resolved",
                    "sourceRefs": resolved[0].source_refs,
                    "instruction": "These are the actual selected excerpts. Check that they support this finding before keeping its code in the final review.",
                });
                recorded
                    .lock()
                    .unwrap()
                    .insert(submission.code.clone(), submission);
                Ok(receipt)
            })
        }),
    }];
    let recorded_for_validate = Arc::clone(&recorded);
    let validate: ValidateFn = Arc::new(move |value: Value| {
        let recorded = Arc::clone(&recorded_for_validate);
        Box::pin(async move {
            let codes: Vec<String> = value
                .get("observationCodes")
                .and_then(Value::as_array)
                .map(|items| {
                    items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect()
                })
                .unwrap_or_default();
            let recorded = recorded.lock().unwrap();
            let unknown: Vec<String> = codes
                .iter()
                .filter(|code| !recorded.contains_key(*code))
                .cloned()
                .collect();
            if !unknown.is_empty()
                || codes.iter().collect::<std::collections::BTreeSet<_>>().len() != codes.len()
            {
                return Err(super::structured_agent::StructuredToolError::coded(
                    "REVIEW_OBSERVATIONS_UNRECORDED",
                    serde_json::to_string(&serde_json::json!({
                        "code": "REVIEW_OBSERVATIONS_UNRECORDED",
                        "unknownCodes": unknown,
                        "acceptedCodes": recorded.keys().cloned().collect::<Vec<_>>(),
                        "instruction": "Submit and correct each selected finding through record_review_observation, then finish with its accepted code exactly once. Remove withdrawn findings from the final list.",
                    }))
                    .unwrap_or_default(),
                ));
            }
            Ok(value)
        })
    });
    let index_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "observationCodes": { "type": "array", "items": { "type": "string", "minLength": 1 } },
        },
        "required": ["summary", "observationCodes"],
        "additionalProperties": false,
    });
    let max_tokens = std::cmp::min(4096 * 2, agents.defaults.max_tokens);
    let outcome = run_structured_tool(
        Arc::clone(&agents.chat),
        &agents.model,
        &messages,
        agents.defaults.clone(),
        WorkerToolSpec {
            name: "submit_short_fiction_review".into(),
            description: "Submit evidence-backed observations for the persisted short-fiction draft.".into(),
            parameters_schema: index_schema,
            max_turns: Some(16),
        },
        Some(validate),
        supporting,
        StructuredRunOptions { temperature: None, max_tokens: Some(max_tokens) },
    )
    .await?;

    // 依最终 codes 顺序 resolve recorded 条目。
    let recorded = recorded.lock().unwrap();
    let codes: Vec<String> = outcome
        .result
        .get("observationCodes")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let submissions: Vec<ReviewObservationSubmission> = codes
        .iter()
        .filter_map(|code| recorded.get(code).cloned())
        .collect();
    let observations = resolve_observation_sources(&submissions, &sources).map_err(|error| {
        StructuredAgentError {
            code: error.code,
            message: error.message,
            attempts: Some(outcome.turns),
            result_tool: Some("submit_short_fiction_review".into()),
        }
    })?;
    Ok(ReviewOutcome {
        summary: outcome
            .result
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        observations,
        usage: outcome.usage,
    })
}

// ── helpers ──────────────────────────────────────────────────────────

fn llm_system(content: &str) -> LLMMessage {
    LLMMessage { role: crate::llm::provider::LLMRole::System, content: content.into(), tool_calls: None, tool_call_id: None }
}

fn llm_user(content: &str) -> LLMMessage {
    LLMMessage { role: crate::llm::provider::LLMRole::User, content: content.into(), tool_calls: None, tool_call_id: None }
}

use std::sync::Mutex;

// ReviewSourceError re-export 供上层（endpoint 兑换面）引用。
pub use crate::models::observation::ReviewSourceError as ShortReviewSourceError;

// ── 测试（mock 注入：同 prompt+同 mock 响应 → 同产物字节）────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::production::structured_agent::{
        ChatTurnError, ChatTurnResponse, ChatTurnToolCall,
    };
    use std::collections::VecDeque;

    fn scripted(script: Vec<ChatTurnResponse>) -> (ShortAgents, Arc<Mutex<Vec<CapturedAgentCall>>>) {
        let captured: Arc<Mutex<Vec<CapturedAgentCall>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_for_fn = Arc::clone(&captured);
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        let chat: crate::production::structured_agent::ChatTurnFn = Arc::new(move |request| {
            let captured_for_fn = Arc::clone(&captured_for_fn);
            let script = Arc::clone(&script);
            let system = request
                .messages
                .first()
                .map(|message| message.content.clone())
                .unwrap_or_default();
            let user = request
                .messages
                .last()
                .map(|message| message.content.clone())
                .unwrap_or_default();
            let tools = request.tools.clone();
            let tool_choice = request.tool_choice.map(str::to_string);
            Box::pin(async move {
                captured_for_fn.lock().unwrap().push(CapturedAgentCall {
                    system,
                    user,
                    tools,
                    tool_choice,
                });
                let mut queue = script.lock().unwrap();
                queue.pop_front().ok_or_else(|| ChatTurnError {
                    message: "script exhausted".into(),
                    output_limit: false,
                })
            })
        });
        let agents = ShortAgents {
            chat,
            model: "test-model".into(),
            defaults: StructuredDefaults { temperature: 0.7, max_tokens: 8192 },
        };
        (agents, captured)
    }

    #[derive(Clone)]
    #[allow(dead_code)] // user 面为后续 prompts 对比断言预留（888 号捕获器同型）
    struct CapturedAgentCall {
        system: String,
        user: String,
        tools: Value,
        tool_choice: Option<String>,
    }

    fn tool_response(tool_name: &str, payload: Value) -> ChatTurnResponse {
        ChatTurnResponse {
            content: String::new(),
            tool_calls: vec![ChatTurnToolCall {
                id: "c1".into(),
                name: tool_name.into(),
                arguments: serde_json::to_string(&payload).unwrap(),
            }],
            finish_reason: Some("tool_calls".into()),
            usage: crate::production::structured_agent::ChatTurnUsage {
                prompt_tokens: 11,
                completion_tokens: 7,
                total_tokens: 18,
            },
        }
    }

    const GOLDEN_OUTLINE_RAW: &str = "## 总纲\n\n雨夜碎镜，以镜换名。\n\n## 第1章\n\n换名之夜。\n\n## 第2章\n\n窥影初现。";

    #[tokio::test]
    async fn outline_golden_mock_response_to_raw_content_bytes() {
        // 同 mock 响应 → 同 rawContent 拼接字节（planMarkdown + 逐章标题段）。
        let (agents, captured) = scripted(vec![tool_response("submit_short_outline", serde_json::json!({
            "storyTitle": "雨夜碎镜",
            "planMarkdown": "## 总纲\n\n雨夜碎镜，以镜换名。",
            "chapter_1_plan": "换名之夜。",
            "chapter_2_plan": "窥影初现。",
        }))]);
        let outcome = outline_create_outline(
            &agents,
            &OutlineInput {
                title: None,
                direction: "雨夜来客的故事",
                chapter_count: 2,
                chars_per_chapter: 1000,
                reference_text: None,
            },
            WritingLanguage::Zh,
        )
        .await
        .unwrap();
        assert_eq!(outcome.story_title, "雨夜碎镜");
        assert_eq!(outcome.raw_content, GOLDEN_OUTLINE_RAW);
        // 形态面：schema 的 chapter_N_plan 动态键 + tool_choice required。
        let call = &captured.lock().unwrap()[0];
        assert_eq!(call.tools[0]["function"]["name"], "submit_short_outline");
        assert!(call.tools[0]["function"]["parameters"]["properties"]
            .get("chapter_2_plan")
            .is_some());
        assert_eq!(call.tool_choice.as_deref(), Some("required"));
        // system = outline system prompt 逐字 + finish-by-calling。
        assert!(call.system.starts_with("按已激活的短篇写作 Skill 和用户材料生成完整短篇方案，通过方案工具提交。\n\nFinish by calling submit_short_outline exactly once."));
    }

    #[tokio::test]
    async fn packaging_golden_mock_response_to_sales_bytes() {
        let draft = ShortFictionBatchDraft {
            story_title: "雨夜碎镜".into(),
            opening_hook: None,
            chapters: vec![ShortFictionChapter {
                number: 1,
                title: "换名".into(),
                content: "斋主以镜换名。".into(),
                char_count: 7,
            }],
            raw_content: String::new(),
        };
        let (agents, _captured) = scripted(vec![tool_response("submit_short_package", serde_json::json!({
            "title": "雨夜碎镜",
            "intro": "一个以镜换名的故事。",
            "sellingPoints": ["短", "冷"],
            "coverPrompt": "碎镜中的雨夜。",
        }))]);
        let sales = packaging_generate(
            &agents,
            &PackageInput { direction: "雨夜来客的故事", draft: &draft, language: WritingLanguage::Zh },
        )
        .await
        .unwrap();
        assert_eq!(
            sales.raw_content,
            "# 雨夜碎镜\n\n## Intro\n一个以镜换名的故事。\n\n## Selling Points\n- 短\n- 冷\n\n## Cover Prompt\n碎镜中的雨夜。"
        );
    }

    #[tokio::test]
    async fn writer_write_draft_flat_batch_merges_in_order() {
        // 批次（≤预算 → 单批全章）：flat schema 响应 → merge → rawContent 渲染。
        let (agents, captured) = scripted(vec![tool_response("submit_short_draft_batch", serde_json::json!({
            "storyTitle": "雨夜碎镜",
            "openingHook": "雨夜来客叩门。",
            "chapter_1_title": "换名",
            "chapter_1_content": "斋主以镜换名，来客留下一个再未使用的名字。",
            "chapter_2_title": "窥影",
            "chapter_2_content": "镜中人影与来客同貌，交易自此刻开始。",
        }))]);
        let outcome = writer_write_draft(
            &agents,
            &WriteDraftInput {
                title: None,
                opening_hook_chars: Some(7),
                direction: "雨夜来客的故事",
                outline_markdown: "## 总纲",
                chapter_count: 2,
                chars_per_chapter: 1000,
                min_chapter_length: None,
                max_chapter_length: None,
                max_chapters_per_call: None,
                chapter_numbers: None,
                language: WritingLanguage::Zh,
            },
            |_draft, _complete| {},
        )
        .await
        .unwrap();
        assert_eq!(outcome.draft.story_title, "雨夜碎镜");
        assert_eq!(outcome.draft.chapters.len(), 2);
        assert_eq!(outcome.draft.chapters[0].char_count, 21);
        assert_eq!(
            outcome.draft.raw_content,
            "# 雨夜碎镜\n\n## 开篇钩子\n\n雨夜来客叩门。\n\n## 第1章 换名\n\n斋主以镜换名，来客留下一个再未使用的名字。\n\n## 第2章 窥影\n\n镜中人影与来客同貌，交易自此刻开始。"
        );
        // flat schema 的动态键面（chapter_N_title/content）+ openingHook 描述注入。
        let call = &captured.lock().unwrap()[0];
        assert_eq!(call.tools[0]["function"]["name"], "submit_short_draft_batch");
        assert!(call.tools[0]["function"]["parameters"]["properties"]
            .get("chapter_2_content")
            .is_some());
    }

    #[tokio::test]
    async fn reviewer_resolves_line_ranges_and_validates_codes() {
        // sourced review：supporting 记录 → 最终 codes 依序 resolve → quote 截取。
        let outline = "## 总纲\n\n雨夜碎镜，以镜换名。";
        let (agents, _captured) = scripted(vec![
            // 轮 1：record 一条观察（行区间引用）。
            tool_response("record_review_observation", serde_json::json!({
                "code": "PLOT-1",
                "summary": "大纲与正文冲突。",
                "category": "quality",
                "assessment": "issue",
                "sourceRefs": [{ "sourceId": "outline", "startLine": 1, "endLine": 1 }],
            })),
            // 轮 2：终审 summary + codes。
            tool_response("submit_short_fiction_review", serde_json::json!({
                "summary": "一处大纲差异。",
                "observationCodes": ["PLOT-1"],
            })),
        ]);
        let draft = ShortFictionBatchDraft {
            story_title: "雨夜碎镜".into(),
            opening_hook: None,
            chapters: vec![ShortFictionChapter {
                number: 1,
                title: "换名".into(),
                content: "斋主以镜换名。".into(),
                char_count: 7,
            }],
            raw_content: "# 雨夜碎镜\n\n## 第1章 换名\n\n斋主以镜换名。".into(),
        };
        let outcome = reviewer_review_draft(
            &agents,
            &ReviewDraftInput {
                direction: "雨夜来客的故事",
                outline_markdown: outline,
                draft: &draft,
                reference_text: None,
                revision_request: None,
                review_scope: None,
                measurements: None,
            },
            WritingLanguage::Zh,
        )
        .await
        .unwrap();
        assert_eq!(outcome.summary, "一处大纲差异。");
        assert_eq!(outcome.observations.len(), 1);
        let observation = &outcome.observations[0];
        assert_eq!(observation.code, "PLOT-1");
        let source_refs = observation.source_refs.as_ref().unwrap();
        assert_eq!(source_refs[0].source_id, "outline");
        assert_eq!(source_refs[0].quote, "## 总纲", "行区间宿主截取（1 基含尾）");
    }

    #[tokio::test]
    async fn reviewer_unknown_code_exhausts_to_unrecorded() {
        // 变异：终审 codes 含未记录项 → REVIEW_OBSERVATIONS_UNRECORDED 回喂
        // → 预算（16 轮）耗尽必红、末次码透传。
        let (agents, _captured) = scripted(
            (1..=20)
                .map(|index| {
                    // json! 数组元素用预构造标识符（内联 format! 表达式会触发
                    // json_internal 数组臂的 delimiter 解析问题）。
                    let code = Value::String(format!("GHOST-{index}"));
                    tool_response("submit_short_fiction_review", serde_json::json!({
                        "summary": "占位",
                        "observationCodes": [code],
                    }))
                })
                .collect(),
        );
        let draft = ShortFictionBatchDraft {
            story_title: "x".into(),
            opening_hook: None,
            chapters: Vec::new(),
            raw_content: String::new(),
        };
        let error = reviewer_review_draft(
            &agents,
            &ReviewDraftInput {
                direction: "d",
                outline_markdown: "o",
                draft: &draft,
                reference_text: None,
                revision_request: None,
                review_scope: None,
                measurements: None,
            },
            WritingLanguage::Zh,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "REVIEW_OBSERVATIONS_UNRECORDED");
        assert_eq!(error.attempts, Some(16), "maxTurns 16 预算闸");
    }
}
