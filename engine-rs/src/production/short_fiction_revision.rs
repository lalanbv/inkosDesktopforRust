//! 短篇修订执行链（891 号 M3c 终）：`agents/short-fiction.ts` 的
//! `validateRevisionPlan`（纯逻辑）+ `reviseDraft`/`applyRevisionPlan`/
//! `continueDraft`（writer 修订执行——ReviserAgent 经 [`ReviseChapterFn`]
//! 注入，mock golden 面与生产共用）。
//!
//! 范围备案：TS planRevision 的 submitStructured 环（maxTurns 6 + supporting
//! tools + normalize）经 888 基座 [`run_structured_tool`] 承载；reviser 的
//! author 权限链经 890 基座注入。

use serde_json::Value;

use super::short_fiction::{
    ShortFictionBatchDraft, ShortDraftChapterSubmission, ShortFictionChapter,
    ShortRevisionPlan,
};
use super::short_fiction_agents::WriteDraftInput;
use super::structured_agent::{run_structured_tool, WorkerToolSpec};
use crate::llm::provider::LLMMessage;
use crate::models::observation::Observation;
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{count_chapter_length, resolve_length_counting_mode};

// ── validateRevisionPlan（纯逻辑）────────────────────────────────────

pub struct RevisionPlanContext<'a> {
    pub chapter_count: u32,
    pub source_chapter_count: usize,
    pub chapter_numbers: Option<&'a [u32]>,
    pub author_scope_story_title: bool,
    pub author_scope_opening: bool,
    pub outline_markdown: &'a str,
    pub opening_hook_chars: Option<u32>,
    pub language: WritingLanguage,
    pub draft_story_title: &'a str,
    pub draft_opening_hook: Option<&'a str>,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct RevisionPlanError {
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
}

fn plan_error(code: &'static str, message: impl Into<String>) -> RevisionPlanError {
    RevisionPlanError { code, message: message.into(), details: None }
}

fn plan_error_details(code: &'static str, details: Value) -> RevisionPlanError {
    RevisionPlanError {
        code,
        message: details.to_string(),
        details: Some(details),
    }
}

/// TS `validateRevisionPlan`（short-fiction.ts L244-270）纯逻辑校验。
pub fn validate_revision_plan(
    plan: &ShortRevisionPlan,
    context: &RevisionPlanContext<'_>,
) -> Result<(), RevisionPlanError> {
    let source_count = context.source_chapter_count;
    if let Some(story_title) = &plan.story_title {
        if story_title.trim().is_empty() {
            return Err(plan_error(
                "SHORT_REVISION_PLAN_INVALID",
                "A revised book title must contain text.",
            ));
        }
        if plan.story_title.as_deref() != Some(context.draft_story_title)
            && !context.author_scope_story_title
        {
            return Err(plan_error(
                "SHORT_REVISION_OUT_OF_SCOPE",
                "The book title is outside the author-authorized scope.",
            ));
        }
    }
    if let Some(chapter_numbers) = context.chapter_numbers {
        if plan.chapters.iter().any(|chapter| !chapter_numbers.contains(&chapter.number)) {
            return Err(plan_error(
                "SHORT_REVISION_OUT_OF_SCOPE",
                "The plan changes a chapter outside the author's selected scope.",
            ));
        }
    }
    if let Some(_chapter_numbers) = context.chapter_numbers.filter(|numbers| !numbers.is_empty()) {
        if !context.author_scope_opening {
            if let Some(opening) = &plan.opening_hook {
                if opening.trim() != context.draft_opening_hook.unwrap_or("") {
                    return Err(plan_error(
                        "SHORT_REVISION_OUT_OF_SCOPE",
                        "The opening is outside the selected chapter scope.",
                    ));
                }
            }
        }
    }
    if let (Some(target), Some(opening)) = (context.opening_hook_chars, &plan.opening_hook) {
        let mode = resolve_length_counting_mode(context.language);
        let length = count_chapter_length(opening, mode);
        let minimum = std::cmp::max(1, (target * 3) / 4);
        let maximum = (target as u64 * 5).div_ceil(4) as u32;
        if length < minimum || length > maximum {
            return Err(plan_error_details(
                "SHORT_OPENING_HOOK_CONTRACT",
                serde_json::json!({
                    "code": "SHORT_OPENING_HOOK_CONTRACT",
                    "length": length,
                    "minimum": minimum,
                    "maximum": maximum,
                }),
            ));
        }
    }
    let title_unchanged = plan.story_title.is_none()
        || plan.story_title.as_deref() == Some(context.draft_story_title);
    let opening_unchanged = plan
        .opening_hook
        .as_deref()
        .map(|hook| hook.trim() == context.draft_opening_hook.unwrap_or(""))
        .unwrap_or(true);
    let outline_unchanged = plan
        .outline_markdown
        .as_deref()
        .map(|outline| outline == context.outline_markdown)
        .unwrap_or(true);
    if plan.chapters.is_empty()
        && context.chapter_count == source_count as u32
        && title_unchanged
        && opening_unchanged
        && outline_unchanged
    {
        return Err(plan_error(
            "SHORT_REVISION_PLAN_EMPTY",
            "Select an authorized change to the book title, chapters, opening scene or outline.",
        ));
    }
    if plan.chapters.iter().map(|chapter| chapter.number).collect::<std::collections::BTreeSet<_>>().len()
        != plan.chapters.len()
        || plan.chapters.iter().any(|chapter| {
            chapter.number == 0
                || chapter.number > context.chapter_count
                || chapter
                    .source_number
                    .map(|source| source > source_count as u32)
                    .unwrap_or(false)
                || chapter
                    .instruction
                    .as_deref()
                    .map(str::trim)
                    .map(str::is_empty)
                    .unwrap_or(true)
                        && (chapter.source_number.map(|source| source == 0).unwrap_or(true))
        })
    {
        return Err(plan_error(
            "SHORT_REVISION_PLAN_INVALID",
            "Each new scene requires a writing instruction; every destination and original source must be valid.",
        ));
    }
    // 源章节多目标复制检查（copies map → duplicates）。
    let mut copies: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
    for number in 1..=context.chapter_count {
        let item = plan.chapters.iter().find(|chapter| chapter.number == number);
        if item.and_then(|chapter| chapter.instruction.as_deref()).map(str::trim).map(|text| !text.is_empty()).unwrap_or(false) {
            continue;
        }
        let source = item.and_then(|chapter| chapter.source_number).unwrap_or(number);
        if source > source_count as u32 {
            return Err(RevisionPlanError {
                code: "SHORT_REVISION_PLAN_INVALID",
                message: "A new chapter requires a writing instruction.".into(),
                details: Some(serde_json::json!({ "chapterNumber": number })),
            });
        }
        copies.entry(source).or_default().push(number);
    }
    let duplicates: Vec<Value> = copies
        .iter()
        .filter(|(_, destinations)| destinations.len() > 1)
        .map(|(source, destinations)| {
            serde_json::json!({ "sourceNumber": source, "destinations": destinations })
        })
        .collect();
    if !duplicates.is_empty() {
        return Err(plan_error_details(
            "SHORT_REVISION_SOURCE_REUSED",
            serde_json::json!({
                "code": "SHORT_REVISION_SOURCE_REUSED",
                "duplicates": duplicates,
                "requiredChapterCount": context.chapter_count,
                "instruction": "Assign every affected final slot. Move each unchanged original scene once and write a distinct scene in any remaining slot.",
            }),
        ));
    }
    let restructuring = context.chapter_count != source_count as u32
        || plan.chapters.iter().any(|chapter| {
            chapter.source_number.map(|source| source != chapter.number).unwrap_or(false)
        });
    if restructuring
        && plan.outline_markdown.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true)
    {
        return Err(plan_error(
            "SHORT_REVISION_OUTLINE_REQUIRED",
            "Provide the outline for the restructured manuscript.",
        ));
    }
    Ok(())
}

// ── applyRevisionPlan（修订执行）─────────────────────────────────────

/// ReviserAgent.reviseChapter 的注入面（scoped/compress 分支调用——890 基座
/// 的 revise_chapter 由调用方包装）。
pub type ReviseChapterFn = Arc<
    dyn Fn(
            u32,
            String,
            Vec<Observation>,
            Option<u32>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, String>> + Send>,
        > + Send
        + Sync,
>;

use std::sync::Arc;

pub struct ApplyRevisionPlanContext<'a> {
    pub chat: &'a super::structured_agent::ChatTurnFn,
    pub model: &'a str,
    pub defaults: &'a super::structured_agent::StructuredDefaults,
    pub language: WritingLanguage,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub min_chapter_length: Option<u32>,
    pub max_chapter_length: Option<u32>,
    pub outline_markdown: &'a str,
    pub direction: &'a str,
    /// Reviser 注入（compress 分支；None = M3c 未接 ReviserAgent——compress
    /// 走失败回喂）。
    pub revise_chapter: Option<ReviseChapterFn>,
    pub on_revision_progress: &'a mut dyn FnMut(&ShortFictionBatchDraft, &[u32]),
}

pub struct ApplyRevisionPlanOutcome {
    pub draft: ShortFictionBatchDraft,
    pub outline_markdown: String,
}

/// TS `applyRevisionPlan`（非 scoped 主干——scoped 分支经 revise_chapter 注入）。
#[allow(clippy::too_many_lines)]
pub async fn apply_revision_plan(
    context: &mut ApplyRevisionPlanContext<'_>,
    original: &ShortFictionBatchDraft,
    plan: &ShortRevisionPlan,
) -> Result<ApplyRevisionPlanOutcome, RevisionPlanError> {
    let mode = resolve_length_counting_mode(context.language);
    let mut draft = original.clone();
    if draft.chapters.len() != context.chapter_count as usize {
        draft.chapters = (1..=context.chapter_count)
            .map(|number| {
                draft
                    .chapters
                    .iter()
                    .find(|chapter| chapter.number == number)
                    .cloned()
                    .unwrap_or(ShortFictionChapter {
                        number,
                        title: String::new(),
                        content: String::new(),
                        char_count: 0,
                    })
            })
            .collect();
    }
    let _restructuring = context.chapter_count as usize != original.chapters.len()
        || plan.chapters.iter().any(|chapter| {
            chapter.source_number.map(|source| source != chapter.number).unwrap_or(false)
        });
    for chapter_plan in &plan.chapters {
        let chapter = draft
            .chapters
            .iter()
            .find(|item| item.number == chapter_plan.number)
            .cloned()
            .expect("padding 已保证槽位在场");
        let original_chapter = if chapter_plan.source_number == Some(0) {
            None
        } else {
            original
                .chapters
                .iter()
                .find(|item| {
                    item.number == chapter_plan.source_number.unwrap_or(chapter_plan.number)
                })
                .cloned()
        };
        let has_instruction = chapter_plan
            .instruction
            .as_deref()
            .map(str::trim)
            .map(|text| !text.is_empty())
            .unwrap_or(false);
        if !has_instruction {
            // 无 instruction 且非 scoped：原章复制（TS 非 scoped 复制分支）。
            if let Some(source_chapter) = original_chapter {
                if let Some(slot) = draft.chapters.iter_mut().find(|item| item.number == chapter_plan.number) {
                    slot.title = source_chapter.title;
                    slot.content = source_chapter.content;
                    slot.char_count = source_chapter.char_count;
                }
            }
            continue;
        }
        let instruction = chapter_plan.instruction.clone().unwrap_or_default();
        let other_chapters: Vec<ShortFictionChapter> = draft
            .chapters
            .iter()
            .filter(|item| item.number != chapter_plan.number)
            .cloned()
            .collect();
        // 修订提交（writer 自身 submitStructured——compress/Reviser 注入点备案：
        // 本轮以同环直改承载，ReviserAgent.compress 分支经 revise_chapter 注入
        // 由 892 轮接通）。
        let system_content = if context.language == WritingLanguage::En {
            "Use the activated Skill, user request and revision plan to revise the specified chapter. Submit its title and complete prose within the supplied length range. Express the revision as in-world actions and dialogue; keep editorial chapter references and review instructions in the plan, outside the story prose."
        } else {
            "按已激活的 Skill、用户修改要求与修订方案修改指定章节，提交本章标题和完整正文，并满足给定篇幅范围。把修改落实为故事中的行动与对白；章号引用、审稿意见和修改说明留在方案中，不写进故事正文。"
        };
        let current_length = count_chapter_length(&chapter.content, mode);
        let user_payload = serde_json::json!({
            "storyTitle": draft.story_title,
            "userRequest": context.direction,
            "chapterNumber": chapter.number,
            "currentLength": current_length,
            "targetLength": context.chars_per_chapter,
            "minLength": context.min_chapter_length,
            "maxLength": context.max_chapter_length,
            "corrections": plan.revision_brief,
            "outline": plan.outline_markdown.clone().unwrap_or_else(|| context.outline_markdown.to_string()),
            "instruction": instruction,
            "otherChapters": other_chapters,
        });
        let messages = vec![
            LLMMessage { role: crate::llm::provider::LLMRole::System, content: system_content.into(), tool_calls: None, tool_call_id: None },
            LLMMessage { role: crate::llm::provider::LLMRole::User, content: user_payload.to_string(), tool_calls: None, tool_call_id: None },
        ];
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "minLength": 1, "description": "Chapter title without a number or heading prefix." },
                "content": { "type": "string", "minLength": 1, "description": "Complete prose of the requested chapter." },
            },
            "required": ["title", "content"],
            "additionalProperties": false,
        });
        let outcome = run_structured_tool(
            Arc::clone(context.chat),
            context.model,
            &messages,
            context.defaults.clone(),
            WorkerToolSpec {
                name: "submit_short_revision_chapter".into(),
                description: format!("Submit the title and prose for chapter {}.", chapter.number),
                parameters_schema: schema,
                max_turns: None,
            },
            None,
            Vec::new(),
            super::structured_agent::StructuredRunOptions {
                temperature: None,
                max_tokens: Some(std::cmp::min(8192, context.defaults.max_tokens)),
            },
        )
        .await
        .map_err(|error| RevisionPlanError {
            code: if error.code == "SHORT_CHAPTER_TOO_LONG" { "SHORT_CHAPTER_TOO_LONG" } else { "SHORT_REVISION_SUBMIT_FAILED" },
            message: error.message,
            details: None,
        })?;
        let title = outcome
            .result
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let content = outcome
            .result
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if let Some(slot) = draft.chapters.iter_mut().find(|item| item.number == chapter_plan.number) {
            slot.title = title;
            slot.content = content;
            slot.char_count = count_chapter_length(&slot.content, mode);
        }
        // 进度回调（completed 面由调用方推断——TS 传 plan/draft/completed）。
        let completed: Vec<u32> = plan
            .chapters
            .iter()
            .take_while(|item| {
                draft.chapters.iter().any(|slot| {
                    slot.number == item.number && !slot.content.trim().is_empty()
                })
            })
            .map(|item| item.number)
            .collect();
        (context.on_revision_progress)(&draft, &completed);
    }
    let mut final_draft = draft;
    final_draft.story_title = plan
        .story_title
        .clone()
        .unwrap_or_else(|| final_draft.story_title.clone());
    final_draft.opening_hook = plan
        .opening_hook
        .as_deref()
        .map(|hook| if hook.trim().is_empty() { None } else { Some(hook.trim().to_string()) })
        .unwrap_or(final_draft.opening_hook.clone());
    final_draft.raw_content =
        super::short_fiction::render_short_fiction_draft_markdown(&final_draft, context.language)
            .map_err(|error| RevisionPlanError {
                code: "SHORT_REVISION_RENDER_FAILED",
                message: error.message,
                details: None,
            })?;
    Ok(ApplyRevisionPlanOutcome {
        draft: final_draft,
        outline_markdown: plan
            .outline_markdown
            .clone()
            .unwrap_or_else(|| context.outline_markdown.to_string()),
    })
}

/// writer 修订上下文的哑输入占位（供 WriteDraftInput 复用面——见 889 主干）。
pub fn writer_input_view<'a>(
    direction: &'a str,
    outline_markdown: &'a str,
    chapter_count: u32,
    language: WritingLanguage,
) -> WriteDraftInput<'a> {
    WriteDraftInput {
        title: None,
        opening_hook_chars: None,
        direction,
        outline_markdown,
        chapter_count,
        chars_per_chapter: 0,
        min_chapter_length: None,
        max_chapter_length: None,
        max_chapters_per_call: None,
        chapter_numbers: None,
        language,
    }
}

/// TS `mergeShortFictionBatch` 的章节提交收集别名（writer 修订链复用）。
pub fn chapters_to_submissions(
    chapters: &[ShortFictionChapter],
) -> Vec<ShortDraftChapterSubmission> {
    chapters
        .iter()
        .map(|chapter| ShortDraftChapterSubmission {
            number: chapter.number,
            title: chapter.title.clone(),
            content: chapter.content.clone(),
        })
        .collect()
}
