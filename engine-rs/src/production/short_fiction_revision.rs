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
use super::structured_agent::{run_structured_tool, StructuredToolError, WorkerToolSpec};
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

/// TS async `onRevisionProgress` 的共享回调形态（validate 候选改良与逐章完成）。
pub type RevisionProgressCallback = Arc<
    dyn Fn(&ShortFictionBatchDraft, &[u32]) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;

/// compress 回退的 boxed future（闭包返回型别名）。
type CompressOutcomeFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(String, Vec<String>, ShortFictionChapter), RevisionPlanError>> + Send>,
>;

/// ReviserAgent.reviseChapter 的注入请求（892 号装配面——compress 分支消费；
/// 894 号扩 scoped 分支面——author 权限解析 + governed 上下文逐字）。
pub struct ReviseChapterRequest {
    pub chapter_number: u32,
    pub pending_content: String,
    pub observations: Vec<Observation>,
    pub max_chapter_length: Option<u32>,
    /// TS `onCandidate`：候选收集 sink（环内 push；调用方环后按序复刻采纳面）。
    pub candidate_sink: Option<Arc<std::sync::Mutex<Vec<String>>>>,
    /// scoped 分支面（None = compress 回退形态）。
    pub scoped: Option<ScopedReviseInput>,
}

/// TS applyRevisionPlan scoped 分支的逐章输入（L296-312 逐字面）。
pub struct ScopedReviseInput {
    /// `JSON.stringify({revisionBrief, chapterInstruction: chapterPlan.instruction})`。
    pub instruction_json: String,
    /// select identity 的章题（original.title）。
    pub chapter_title: Option<String>,
    /// governed 上下文（otherChapters 摘录——TS original-manuscript selectedContext）。
    pub selected_context: Vec<super::narrative_context::ContextSource>,
    /// author 源引用（resolveAuthorTextPermission 输入 + 原章内容）。
    pub author_work_id: String,
    pub author_artifact_id: String,
    pub author_revision_id: String,
    pub author_request: String,
    pub author_original_content: String,
}

pub struct ReviseChapterOutcome {
    pub revised_content: String,
    /// revise 环内已达到「更优长度」的候选（TS onCandidate 采纳面——失败时
    /// 保留的中断恢复语义）。
    pub accepted_candidates: Vec<String>,
}

/// ReviserAgent.reviseChapter 的注入面（compress 分支调用——890 基座的
/// revise_chapter 由 [`super::short_fiction_writer_chain`] 装配）。
pub type ReviseChapterFn = Arc<
    dyn Fn(
            ReviseChapterRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ReviseChapterOutcome, String>> + Send>,
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
    /// Reviser 注入（compress 分支；None = 未接 ReviserAgent——SHORT_CHAPTER_
    /// TOO_LONG 直抛）。由 [`super::short_fiction_writer_chain`] 装配。
    pub revise_chapter: Option<ReviseChapterFn>,
    /// TS `onRevisionProgress`（async 回调——validate 候选改良与逐章完成都触发）。
    pub on_revision_progress: RevisionProgressCallback,
    /// TS `input.chapterNumbers`（修订范围——validateRevisionPlan 的范围面）。
    pub chapter_numbers: Option<Vec<u32>>,
    /// TS `input.authorScope`（892：validate 的 title/opening 权限面；894：
    /// scoped 执行分支的允许面）。
    pub author_scope: Option<super::short_fiction_tools::ShortAuthorScope>,
    /// TS `input.authorSource`（894 scoped 分支——原稿+源引用+作者请求）。
    pub author_source: Option<ScopedAuthorSource>,
    /// TS `input.openingHookChars`（validateRevisionPlan 的开篇合同面）。
    pub opening_hook_chars: Option<u32>,
}

/// TS `input.authorSource` 的宿主引用面（episode 原始成稿登记 + 作者请求）。
#[derive(Debug, Clone)]
pub struct ScopedAuthorSource {
    pub work_id: String,
    pub artifact_id: String,
    pub revision_id: String,
    pub request: String,
    pub draft: ShortFictionBatchDraft,
}

pub struct ApplyRevisionPlanOutcome {
    pub draft: ShortFictionBatchDraft,
    pub outline_markdown: String,
}

/// 单章提交的严格/宽松校验（TS `validateRequestedShortChapter` 单章投影——
/// input.title=None、openingHookChars=None 的 applyRevisionPlan 调用面）。
fn validate_single_chapter_submission(
    bound: &Value,
    number: u32,
    language: WritingLanguage,
    min_chapter_length: Option<u32>,
    max_chapter_length: Option<u32>,
    enforce_length: bool,
) -> Result<(), RevisionPlanError> {
    let fail = |code: &'static str, message: String| RevisionPlanError {
        code,
        message,
        details: None,
    };
    if bound.get("number").and_then(Value::as_u64) != Some(number as u64) {
        return Err(fail(
            "SHORT_REVISION_OUT_OF_SCOPE",
            format!("Submit only chapter {number}"),
        ));
    }
    let title = bound.get("title").and_then(Value::as_str).unwrap_or_default();
    let content = bound.get("content").and_then(Value::as_str).unwrap_or_default();
    if title.is_empty() || content.is_empty() {
        return Err(fail(
            "SHORT_BATCH_INVALID",
            "Submitted chapter requires a title and complete prose".into(),
        ));
    }
    if enforce_length {
        let mode = resolve_length_counting_mode(language);
        let length = count_chapter_length(content, mode);
        if let Some(min) = min_chapter_length {
            if length < min {
                return Err(RevisionPlanError {
                    code: "SHORT_CHAPTER_TOO_SHORT",
                    message: serde_json::to_string(&serde_json::json!({
                        "code": "SHORT_CHAPTER_TOO_SHORT",
                        "minChapterLength": min,
                        "chapters": [number],
                        "instruction": "Expand the requested scene within its chapter boundary. Preserve the events assigned to neighboring chapters.",
                    }))
                    .unwrap_or_default(),
                    details: None,
                });
            }
        }
        if let Some(max) = max_chapter_length {
            if length > max {
                return Err(RevisionPlanError {
                    code: "SHORT_CHAPTER_TOO_LONG",
                    message: serde_json::to_string(&serde_json::json!({
                        "code": "SHORT_CHAPTER_TOO_LONG",
                        "maxChapterLength": max,
                        "chapters": [number],
                        "instruction": "Submit complete scenes within their own chapter plan and length limit.",
                    }))
                    .unwrap_or_default(),
                    details: None,
                });
            }
        }
    }
    Ok(())
}

/// TS compressCandidate 的 onCandidate 复刻（apply_revision_plan 侧）：候选按
/// 序并入——超上限且短于当前最优才采纳（TS 实时逐个；注入面在 revise 调用后
/// 按序复刻，终态等价）。
async fn merge_improved_candidates(
    draft_state: &Arc<std::sync::Mutex<ShortFictionBatchDraft>>,
    number: u32,
    candidates: &[String],
    mode: crate::models::length_governance::LengthCountingMode,
    max_chapter_length: Option<u32>,
    on_revision_progress: &RevisionProgressCallback,
    completed: &[u32],
) -> Result<(), RevisionPlanError> {
    for candidate in candidates {
        let count = count_chapter_length(candidate, mode);
        let improves = {
            let mut draft = draft_state.lock().unwrap();
            let current = draft
                .chapters
                .iter()
                .find(|item| item.number == number)
                .cloned()
                .unwrap_or_else(|| ShortFictionChapter {
                    number,
                    title: String::new(),
                    content: String::new(),
                    char_count: 0,
                });
            let improves = max_chapter_length
                .map(|max| count > max && count < count_chapter_length(&current.content, mode))
                .unwrap_or(false);
            if improves {
                if let Some(slot) = draft.chapters.iter_mut().find(|item| item.number == number) {
                    slot.content = candidate.clone();
                    slot.char_count = count;
                }
            }
            improves
        };
        if improves {
            let snapshot = draft_state.lock().unwrap().clone();
            on_revision_progress(&snapshot, completed)
                .await
                .map_err(|message| RevisionPlanError {
                    code: "SHORT_REVISION_PROGRESS_FAILED",
                    message,
                    details: None,
                })?;
        }
    }
    Ok(())
}

/// TS `applyRevisionPlan`（非 scoped 主干 + compress 回退经 ReviseChapterFn
/// 装配——892 号 M3d 接线；894 号 scoped 执行分支接通：authorScope→逐章
/// reviseChapter + 章题提交）。
#[allow(clippy::too_many_lines)]
pub async fn apply_revision_plan(
    context: &ApplyRevisionPlanContext<'_>,
    original: &ShortFictionBatchDraft,
    plan: &ShortRevisionPlan,
) -> Result<ApplyRevisionPlanOutcome, RevisionPlanError> {
    // TS applyRevisionPlan 首行：this.validateRevisionPlan(plan, input)。
    validate_revision_plan(
        plan,
        &RevisionPlanContext {
            chapter_count: context.chapter_count,
            source_chapter_count: original.chapters.len(),
            chapter_numbers: context.chapter_numbers.as_deref(),
            author_scope_story_title: context
                .author_scope
                .as_ref()
                .and_then(|scope| scope.story_title)
                .unwrap_or(false),
            author_scope_opening: context.author_scope.as_ref().map(|scope| scope.opening).unwrap_or(false),
            outline_markdown: context.outline_markdown,
            opening_hook_chars: context.opening_hook_chars,
            language: context.language,
            draft_story_title: &original.story_title,
            draft_opening_hook: original.opening_hook.as_deref(),
        },
    )?;
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
    let draft_state: Arc<std::sync::Mutex<ShortFictionBatchDraft>> =
        Arc::new(std::sync::Mutex::new(draft));
    let restructuring = context.chapter_count as usize != original.chapters.len()
        || plan.chapters.iter().any(|chapter| {
            chapter.source_number.map(|source| source != chapter.number).unwrap_or(false)
        });
    // TS：scoped = authorSource && authorScope && (!wholeManuscript || !restructuring)。
    let scoped_active = context.author_source.is_some()
        && context.author_scope.is_some()
        && (!context
            .author_scope
            .as_ref()
            .map(|scope| scope.whole_manuscript)
            .unwrap_or(false)
            || !restructuring);
    for chapter_plan in &plan.chapters {
        let number = chapter_plan.number;
        let original_chapter = if chapter_plan.source_number == Some(0) {
            None
        } else {
            original
                .chapters
                .iter()
                .find(|item| item.number == chapter_plan.source_number.unwrap_or(number))
                .cloned()
        };
        let has_instruction = chapter_plan
            .instruction
            .as_deref()
            .map(str::trim)
            .map(|text| !text.is_empty())
            .unwrap_or(false);

        // ── scoped 执行分支（TS L291-328）──
        if scoped_active {
            let scope = context.author_scope.as_ref().expect("scoped_active 已保证");
            let author_source = context.author_source.as_ref().expect("scoped_active 已保证");
            let Some(author_original) = author_source.draft.chapters.iter().find(|item| item.number == number).cloned() else {
                return Err(plan_error(
                    "SHORT_REVISION_OUT_OF_SCOPE",
                    "The authorized chapter is absent from the original manuscript.",
                ));
            };
            let mut content = {
                let guard = draft_state.lock().unwrap();
                guard
                    .chapters
                    .iter()
                    .find(|item| item.number == number)
                    .cloned()
                    .expect("padding 已保证槽位在场")
                    .content
            };
            let mut title = author_original.title.clone();
            let prose_authorized = scope.chapter_numbers.contains(&number)
                || (scope.whole_manuscript && scope.chapter_numbers.is_empty());
            if prose_authorized {
                let revise = context.revise_chapter.clone().ok_or_else(|| RevisionPlanError {
                    code: "SHORT_REVISION_SUBMIT_FAILED",
                    message: "scoped revision requires a wired ReviserAgent".into(),
                    details: None,
                })?;
                let other_chapters: Vec<ShortFictionChapter> = {
                    let guard = draft_state.lock().unwrap();
                    guard
                        .chapters
                        .iter()
                        .filter(|item| item.number != number)
                        .cloned()
                        .collect()
                };
                let sink: Arc<std::sync::Mutex<Vec<String>>> =
                    Arc::new(std::sync::Mutex::new(Vec::new()));
                let request = ReviseChapterRequest {
                    chapter_number: number,
                    pending_content: content.clone(),
                    observations: Vec::new(),
                    max_chapter_length: context.max_chapter_length,
                    candidate_sink: Some(Arc::clone(&sink)),
                    scoped: Some(ScopedReviseInput {
                        instruction_json: serde_json::json!({
                            "revisionBrief": plan.revision_brief,
                            "chapterInstruction": chapter_plan.instruction.clone().unwrap_or_default(),
                        })
                        .to_string(),
                        chapter_title: Some(author_original.title.clone()),
                        selected_context: vec![super::narrative_context::ContextSource {
                            source: "original-manuscript".into(),
                            reason: "Preserve established facts in the other chapters.".into(),
                            excerpt: Some(serde_json::to_string(&other_chapters).unwrap_or_default()),
                            protection: super::narrative_context::ContextProtection::Protected,
                        }],
                        author_work_id: author_source.work_id.clone(),
                        author_artifact_id: author_source.artifact_id.clone(),
                        author_revision_id: author_source.revision_id.clone(),
                        author_request: author_source.request.clone(),
                        author_original_content: author_original.content.clone(),
                    }),
                };
                let outcome = revise(request).await.map_err(|message| RevisionPlanError {
                    code: "SHORT_REVISION_SUBMIT_FAILED",
                    message,
                    details: None,
                })?;
                content = outcome.revised_content;
                // TS scoped onCandidate（L304-311）：距离改良即并入。
                let min_length = context.min_chapter_length.unwrap_or(1);
                let distance = |text: &str| {
                    let length = count_chapter_length(text, mode);
                    std::cmp::max(
                        min_length.saturating_sub(length),
                        length.saturating_sub(context.max_chapter_length.unwrap_or(u32::MAX)),
                    )
                };
                // guard 临时不跨 await（for 头临时活整个循环——先取 owned）。
                let candidates = sink.lock().unwrap().clone();
                for candidate in candidates {
                    let (current_content, original_content) = {
                        let guard = draft_state.lock().unwrap();
                        let current = guard
                            .chapters
                            .iter()
                            .find(|item| item.number == number)
                            .cloned()
                            .expect("padding 已保证槽位在场");
                        (current.content, author_original.content.clone())
                    };
                    let improves = current_content == original_content
                        || distance(&candidate) < distance(&current_content);
                    if improves {
                        {
                            let mut guard = draft_state.lock().unwrap();
                            if let Some(slot) = guard.chapters.iter_mut().find(|item| item.number == number) {
                                slot.content = candidate.clone();
                                slot.char_count = count_chapter_length(&candidate, mode);
                            }
                        }
                        let snapshot = draft_state.lock().unwrap().clone();
                        (context.on_revision_progress)(&snapshot, &[]).await.map_err(|message| {
                            RevisionPlanError { code: "SHORT_REVISION_PROGRESS_FAILED", message, details: None }
                        })?;
                    }
                }
            }
            if scope
                .chapter_title_numbers
                .as_ref()
                .map(|numbers| numbers.contains(&number))
                .unwrap_or(false)
            {
                // 章题提交（TS L315-321 逐字）。
                let current_title = draft_state
                    .lock()
                    .unwrap()
                    .chapters
                    .iter()
                    .find(|item| item.number == number)
                    .map(|item| item.title.clone())
                    .unwrap_or_default();
                let title_messages = vec![
                    LLMMessage {
                        role: crate::llm::provider::LLMRole::System,
                        content: "Revise only this chapter title as requested by the author. Keep its promise grounded in the supplied chapter.".into(),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    LLMMessage {
                        role: crate::llm::provider::LLMRole::User,
                        content: serde_json::json!({
                            "authorRequest": author_source.request,
                            "chapterNumber": number,
                            "currentTitle": current_title,
                            "content": content,
                        })
                        .to_string(),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                ];
                let title_schema = serde_json::json!({
                    "type": "object",
                    "properties": { "title": { "type": "string", "minLength": 1 } },
                    "required": ["title"],
                    "additionalProperties": false,
                });
                let title_outcome = run_structured_tool(
                    Arc::clone(context.chat),
                    context.model,
                    &title_messages,
                    context.defaults.clone(),
                    WorkerToolSpec {
                        name: "submit_short_revision_title".into(),
                        description: "Revise an authorized chapter title".into(),
                        parameters_schema: title_schema,
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
                    code: "SHORT_REVISION_TITLE_SUBMIT_FAILED",
                    message: error.message,
                    details: None,
                })?;
                title = title_outcome
                    .result
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            // 严格单章校验（TS L323）+ 并槽 + 进度。
            let story_title = draft_state.lock().unwrap().story_title.clone();
            let bound = serde_json::json!({
                "storyTitle": story_title,
                "number": number,
                "title": title,
                "content": content,
            });
            validate_single_chapter_submission(
                &bound,
                number,
                context.language,
                context.min_chapter_length,
                context.max_chapter_length,
                true,
            )?;
            {
                let mut guard = draft_state.lock().unwrap();
                if let Some(slot) = guard.chapters.iter_mut().find(|item| item.number == number) {
                    slot.title = title;
                    slot.content = content;
                    slot.char_count = count_chapter_length(&slot.content, mode);
                }
            }
            let completed: Vec<u32> = {
                let guard = draft_state.lock().unwrap();
                plan.chapters
                    .iter()
                    .filter(|item| {
                        guard
                            .chapters
                            .iter()
                            .any(|slot| slot.number == item.number && !slot.content.trim().is_empty())
                    })
                    .map(|item| item.number)
                    .collect()
            };
            {
                let snapshot = draft_state.lock().unwrap().clone();
                (context.on_revision_progress)(&snapshot, &completed).await.map_err(|message| {
                    RevisionPlanError { code: "SHORT_REVISION_PROGRESS_FAILED", message, details: None }
                })?;
            }
            continue;
        }
        if !has_instruction {
            // 无 instruction 且非 scoped：原章复制（TS 非 scoped 复制分支）。
            if let Some(source_chapter) = original_chapter {
                let mut guard = draft_state.lock().unwrap();
                if let Some(slot) = guard.chapters.iter_mut().find(|item| item.number == number) {
                    slot.title = source_chapter.title;
                    slot.content = source_chapter.content;
                    slot.char_count = source_chapter.char_count;
                }
            }
            continue;
        }
        let instruction = chapter_plan.instruction.clone().unwrap_or_default();
        let (chapter, story_title, other_chapters) = {
            let guard = draft_state.lock().unwrap();
            let current = guard
                .chapters
                .iter()
                .find(|item| item.number == number)
                .cloned()
                .expect("padding 已保证槽位在场");
            let others: Vec<ShortFictionChapter> = guard
                .chapters
                .iter()
                .filter(|item| item.number != number)
                .cloned()
                .collect();
            (current, guard.story_title.clone(), others)
        };
        let original_destination = original.chapters.iter().find(|item| item.number == number).cloned();
        let completed_for_progress: Vec<u32> = plan
            .chapters
            .iter()
            .filter(|item| item.number != number)
            .filter(|item| {
                let guard = draft_state.lock().unwrap();
                guard
                    .chapters
                    .iter()
                    .any(|slot| slot.number == item.number && !slot.content.trim().is_empty())
            })
            .map(|item| item.number)
            .collect();

        // compress 回退（TS compressCandidate——ReviserAgent spot-fix 受控改写）。
        let compress_candidate = || -> Result<CompressOutcomeFuture, RevisionPlanError> {
            let revise = context.revise_chapter.clone().ok_or_else(|| RevisionPlanError {
                code: "SHORT_REVISION_SUBMIT_FAILED",
                message: "SHORT_CHAPTER_TOO_LONG requires a wired ReviserAgent".into(),
                details: None,
            })?;
            let max = context.max_chapter_length.ok_or_else(|| RevisionPlanError {
                code: "SHORT_REVISION_SUBMIT_FAILED",
                message: "SHORT_CHAPTER_TOO_LONG requires maxChapterLength".into(),
                details: None,
            })?;
            let guard = draft_state.lock().unwrap();
            let pending = guard
                .chapters
                .iter()
                .find(|item| item.number == number)
                .cloned()
                .expect("padding 已保证槽位在场");
            drop(guard);
            let sink: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
            let request = ReviseChapterRequest {
                chapter_number: number,
                pending_content: pending.content.clone(),
                observations: vec![Observation {
                    code: "CHAPTER_LENGTH_OUT_OF_RANGE".into(),
                    summary: "Compress this existing candidate to the supplied length range. Remove repeated explanation or redundant phrasing while preserving its events, facts, knowledge, chronology, decisions and complete scenes. Do not repeat the semantic rewrite or change its title.".into(),
                    evidence: Vec::new(),
                    category: Some(crate::models::observation::ObservationCategory::Quality),
                    assessment: Some(crate::models::observation::ObservationAssessment::Issue),
                    scope: None,
                    target_hash: None,
                    target: None,
                    source_refs: None,
                }],
                max_chapter_length: Some(max),
                candidate_sink: Some(Arc::clone(&sink)),
                scoped: None,
            };
            Ok(Box::pin(async move {
                let outcome = revise(request).await.map_err(|message| RevisionPlanError {
                    code: "SHORT_REVISION_SUBMIT_FAILED",
                    message,
                    details: None,
                })?;
                Ok((outcome.revised_content, outcome.accepted_candidates, pending))
            }))
        };

        // 提交环 + compress 回退（TS L358-393）。
        let system_content = if context.language == WritingLanguage::En {
            "Use the activated Skill, user request and revision plan to revise the specified chapter. Submit its title and complete prose within the supplied length range. Express the revision as in-world actions and dialogue; keep editorial chapter references and review instructions in the plan, outside the story prose."
        } else {
            "按已激活的 Skill、用户修改要求与修订方案修改指定章节，提交本章标题和完整正文，并满足给定篇幅范围。把修改落实为故事中的行动与对白；章号引用、审稿意见和修改说明留在方案中，不写进故事正文。"
        };
        let current_length = count_chapter_length(&chapter.content, mode);
        let mut user_payload = serde_json::Map::new();
        user_payload.insert("storyTitle".into(), Value::String(story_title));
        user_payload.insert("userRequest".into(), Value::String(context.direction.to_string()));
        user_payload.insert("chapterNumber".into(), Value::from(number));
        user_payload.insert("currentLength".into(), Value::from(current_length));
        user_payload.insert("targetLength".into(), Value::from(context.chars_per_chapter));
        user_payload.insert(
            "minLength".into(),
            context.min_chapter_length.map(Value::from).unwrap_or(Value::Null),
        );
        user_payload.insert(
            "maxLength".into(),
            context.max_chapter_length.map(Value::from).unwrap_or(Value::Null),
        );
        user_payload.insert("corrections".into(), Value::String(plan.revision_brief.clone()));
        user_payload.insert(
            "outline".into(),
            Value::String(plan.outline_markdown.clone().unwrap_or_else(|| context.outline_markdown.to_string())),
        );
        user_payload.insert("instruction".into(), Value::String(instruction));
        // TS 条件 spread：sourceNumber 映射非本章时携带 originalChapter。
        if let Some(source) = &original_chapter {
            if chapter_plan.source_number.map(|source_number| source_number != number).unwrap_or(false) {
                user_payload.insert("originalChapter".into(), serde_json::to_value(source).unwrap_or(Value::Null));
            }
        }
        user_payload.insert("currentChapter".into(), serde_json::to_value(&chapter).unwrap_or(Value::Null));
        user_payload.insert(
            "otherChapters".into(),
            serde_json::to_value(&other_chapters).unwrap_or(Value::Null),
        );
        let user_payload = Value::Object(user_payload);
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
        // validate（TS L365-387）：候选改良距离面 + 严格终验（闭包仅捕获纯值）。
        let draft_for_validate = Arc::clone(&draft_state);
        let progress_for_validate = Arc::clone(&context.on_revision_progress);
        let completed_for_validate = completed_for_progress.clone();
        let language_for_validate = context.language;
        let min_for_validate = context.min_chapter_length;
        let max_for_validate = context.max_chapter_length;
        let chapter_count_for_validate = context.chapter_count;
        let original_destination_for_validate =
            original_destination.as_ref().map(|item| item.content.clone());
        let validate: Option<super::structured_agent::ValidateFn> =
            Some(Arc::new(move |value: Value| {
                let draft_state = Arc::clone(&draft_for_validate);
                let on_revision_progress = Arc::clone(&progress_for_validate);
                let completed = completed_for_validate.clone();
                let original_content = original_destination_for_validate.clone();
                Box::pin(async move {
                    let title = value.get("title").and_then(Value::as_str).unwrap_or_default().to_string();
                    let content = value.get("content").and_then(Value::as_str).unwrap_or_default().to_string();
                    let story_title = draft_state.lock().unwrap().story_title.clone();
                    let bound = serde_json::json!({
                        "storyTitle": story_title,
                        "number": number,
                        "title": title,
                        "content": content,
                    });
                    validate_single_chapter_submission(
                        &bound,
                        number,
                        language_for_validate,
                        min_for_validate,
                        max_for_validate,
                        false,
                    )
                    .map_err(|error| StructuredToolError::coded(error.code, error.message))?;
                    // 候选改良（TS L368-384）：distance 改善即并入。
                    let candidate = {
                        let guard = draft_state.lock().unwrap();
                        super::short_fiction::merge_short_fiction_batch(
                            Some(&guard),
                            &guard.story_title,
                            guard.opening_hook.as_deref(),
                            &[ShortDraftChapterSubmission {
                                number,
                                title: title.clone(),
                                content: content.clone(),
                            }],
                            chapter_count_for_validate,
                            language_for_validate,
                        )
                        .map_err(|error| StructuredToolError::coded(error.code, error.message))?
                    };
                    let changed = candidate
                        .chapters
                        .iter()
                        .find(|item| item.number == number)
                        .cloned()
                        .unwrap_or_else(|| ShortFictionChapter {
                            number,
                            title: title.clone(),
                            content: content.clone(),
                            char_count: 0,
                        });
                    super::short_fiction::validate_short_fiction_draft_for_final(
                        &super::short_fiction::ShortFictionBatchDraft {
                            story_title: candidate.story_title.clone(),
                            opening_hook: candidate.opening_hook.clone(),
                            raw_content: candidate.raw_content.clone(),
                            chapters: vec![changed.clone()],
                        },
                        &super::short_fiction::ShortDraftBounds {
                            title: None,
                            opening_hook_chars: None,
                            expected_chapters: Some(1),
                            min_chapter_length: Some(1),
                            max_chapter_length: None,
                            language: Some(language_for_validate),
                        },
                    )
                    .map_err(|error| StructuredToolError::coded(error.code, error.message))?;
                    let distance = |text: &str| {
                        let length = count_chapter_length(text, mode);
                        std::cmp::max(
                            min_for_validate.unwrap_or(1).saturating_sub(length),
                            length.saturating_sub(max_for_validate.unwrap_or(u32::MAX)),
                        )
                    };
                    let (current_content, original_snapshot) = {
                        let guard = draft_state.lock().unwrap();
                        let current = guard
                            .chapters
                            .iter()
                            .find(|item| item.number == number)
                            .cloned()
                            .unwrap_or_else(|| ShortFictionChapter {
                                number,
                                title: String::new(),
                                content: String::new(),
                                char_count: 0,
                            });
                        (current.content.clone(), original_content)
                    };
                    let first_replacement = current_content.trim().is_empty()
                        || original_snapshot.as_deref() == Some(current_content.as_str());
                    if distance(&changed.content) > 0
                        && (first_replacement || distance(&changed.content) < distance(&current_content))
                    {
                        {
                            let mut guard = draft_state.lock().unwrap();
                            if let Some(slot) = guard.chapters.iter_mut().find(|item| item.number == number) {
                                slot.title = changed.title.clone();
                                slot.content = changed.content.clone();
                                slot.char_count = changed.char_count;
                            }
                        }
                        let snapshot = draft_state.lock().unwrap().clone();
                        on_revision_progress(&snapshot, &completed).await.map_err(|message| {
                            StructuredToolError::coded("SHORT_REVISION_PROGRESS_FAILED", message)
                        })?;
                    }
                    // 严格终验（TS L385-386）。
                    validate_single_chapter_submission(
                        &bound,
                        number,
                        language_for_validate,
                        min_for_validate,
                        max_for_validate,
                        true,
                    )
                    .map_err(|error| StructuredToolError::coded(error.code, error.message))?;
                    super::short_fiction::validate_short_fiction_draft_for_final(
                        &super::short_fiction::ShortFictionBatchDraft {
                            story_title: bound.get("storyTitle").and_then(Value::as_str).unwrap_or_default().into(),
                            opening_hook: None,
                            raw_content: String::new(),
                            chapters: vec![super::short_fiction::ShortFictionChapter {
                                number,
                                title: bound.get("title").and_then(Value::as_str).unwrap_or_default().into(),
                                content: bound.get("content").and_then(Value::as_str).unwrap_or_default().into(),
                                char_count: 0,
                            }],
                        },
                        &super::short_fiction::ShortDraftBounds {
                            title: None,
                            opening_hook_chars: None,
                            expected_chapters: Some(1),
                            min_chapter_length: min_for_validate,
                            max_chapter_length: None,
                            language: Some(language_for_validate),
                        },
                    )
                    .map_err(|error| StructuredToolError::coded(error.code, error.message))?;
                    Ok(value)
                })
            }));
        let submit = run_structured_tool(
            Arc::clone(context.chat),
            context.model,
            &messages,
            context.defaults.clone(),
            WorkerToolSpec {
                name: "submit_short_revision_chapter".into(),
                description: format!("Submit the title and prose for chapter {number}."),
                parameters_schema: schema,
                max_turns: None,
            },
            validate,
            vec![
                super::short_fiction_tools::review_calculation_tool(),
                super::short_fiction_tools::story_interval_tool(),
            ],
            super::structured_agent::StructuredRunOptions {
                temperature: None,
                max_tokens: Some(std::cmp::min(8192, context.defaults.max_tokens)),
            },
        );
        let (title, content) = match submit.await {
            Ok(outcome) => {
                let title = outcome.result.get("title").and_then(Value::as_str).unwrap_or_default().to_string();
                let content = outcome.result.get("content").and_then(Value::as_str).unwrap_or_default().to_string();
                (title, content)
            }
            Err(error) => {
                // TS catch：SHORT_CHAPTER_TOO_LONG 且 pending 超上限 → compress 回退。
                let too_long_pending = {
                    let guard = draft_state.lock().unwrap();
                    let pending = guard.chapters.iter().find(|item| item.number == number).cloned().unwrap();
                    context
                        .max_chapter_length
                        .map(|max| count_chapter_length(&pending.content, mode) > max)
                        .unwrap_or(false)
                };
                if error.code != "SHORT_CHAPTER_TOO_LONG" || !too_long_pending {
                    return Err(RevisionPlanError {
                        code: if error.code == "SHORT_CHAPTER_TOO_LONG" { "SHORT_CHAPTER_TOO_LONG" } else { "SHORT_REVISION_SUBMIT_FAILED" },
                        message: error.message,
                        details: None,
                    });
                }
                let (revised_content, accepted, pending) = compress_candidate()?.await?;
                merge_improved_candidates(
                    &draft_state,
                    number,
                    &accepted,
                    mode,
                    context.max_chapter_length,
                    &context.on_revision_progress,
                    &completed_for_progress,
                )
                .await?;
                // 严格校验（TS compressCandidate 出口）。
                let bound = serde_json::json!({
                    "storyTitle": draft_state.lock().unwrap().story_title,
                    "number": number,
                    "title": pending.title,
                    "content": revised_content,
                });
                validate_single_chapter_submission(
                    &bound,
                    number,
                    context.language,
                    context.min_chapter_length,
                    context.max_chapter_length,
                    true,
                )?;
                (pending.title, revised_content)
            }
        };
        {
            let mut guard = draft_state.lock().unwrap();
            if let Some(slot) = guard.chapters.iter_mut().find(|item| item.number == number) {
                slot.title = title;
                slot.content = content;
                slot.char_count = count_chapter_length(&slot.content, mode);
            }
        }
        let completed: Vec<u32> = {
            let guard = draft_state.lock().unwrap();
            plan.chapters
                .iter()
                .filter(|item| {
                    guard
                        .chapters
                        .iter()
                        .any(|slot| slot.number == item.number && !slot.content.trim().is_empty())
                })
                .map(|item| item.number)
                .collect()
        };
        {
            let snapshot = draft_state.lock().unwrap().clone();
            (context.on_revision_progress)(&snapshot, &completed).await.map_err(|message| {
                RevisionPlanError { code: "SHORT_REVISION_PROGRESS_FAILED", message, details: None }
            })?;
        }
    }
    let mut final_draft = {
        let guard = draft_state.lock().unwrap();
        guard.clone()
    };
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
