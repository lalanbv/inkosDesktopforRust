//! writer 继续/修订链（892 号 M3d）：TS `agents/short-fiction.ts` 的
//! `ShortFictionWriterAgent.continueDraft`（L451-508）与
//! `reviseDraft`/`planRevision`（L207-242），基于 891 [`apply_revision_plan`]
//! 主干与 892 [`ReviseChapterFn`] 装配（compress 回退）。
//!
//! 备案：TS `reviseDraft` 的 resume 面与 scoped 执行分支（authorScope/author
//! Source → ReviserAgent + chapter title 提交）随 893 revise 入口接线——本轮
//! planRevision/continueDraft 的调用面 author_scope=None。

use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::observation::Observation;
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::{count_chapter_length, resolve_length_counting_mode};

use super::short_fiction::{
    find_incomplete_short_fiction_chapters, merge_short_fiction_batch,
    render_short_fiction_draft_markdown, validate_opening_hook, opening_hook_length_contract,
    ShortDraftBounds, ShortFictionBatchDraft, ShortRevisionPlan, ShortRevisionPlanChapter,
};
use super::short_fiction_revision::{
    apply_revision_plan, validate_revision_plan, ApplyRevisionPlanContext, ReviseChapterFn,
    ReviseChapterOutcome, ReviseChapterRequest, RevisionPlanContext,
};
use super::short_fiction_tools::{ShortAuthorScope, short_revision_plan_submission_schema};
use super::structured_agent::{
    run_structured_tool, StructuredRunOptions, StructuredToolError, ValidateFn, WorkerToolSpec,
};

/// 统一错误面（code + message；TS Error+code 附着形态）。
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct WriterChainError {
    pub code: String,
    pub message: String,
}

impl WriterChainError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }

    fn from_revision(error: super::short_fiction_revision::RevisionPlanError) -> Self {
        Self { code: error.code.into(), message: error.message }
    }
}

// ── ReviseChapterFn 装配（compress 回退的 ReviserAgent 包装）────────────

/// TS compressCandidate 的 ReviserAgent 调用（applyRevisionPlan L329-353）：
/// spot-fix 受控改写 + revision-scope governed context + 长度合同。候选 sink
/// 由 [`apply_revision_plan`] 侧按序复刻采纳面。
#[allow(clippy::too_many_arguments)]
pub fn compress_revise_chapter_fn(
    chat: super::structured_agent::ChatTurnFn,
    model: impl Into<String>,
    defaults: super::structured_agent::StructuredDefaults,
    language: WritingLanguage,
    chars_per_chapter: u32,
    min_chapter_length: Option<u32>,
    max_chapter_length: Option<u32>,
    direction: impl Into<String>,
) -> ReviseChapterFn {
    let model = model.into();
    let direction = direction.into();
    Arc::new(move |request: ReviseChapterRequest| {
        let chat = Arc::clone(&chat);
        let model = model.clone();
        let defaults = defaults.clone();
        let language = language;
        let direction = direction.clone();
        let length_spec = super::length_contract::build_length_spec(
            chars_per_chapter,
            language,
            (min_chapter_length, max_chapter_length),
        );
        let selected_context = vec![super::narrative_context::ContextSource {
            source: "revision-scope".into(),
            reason: "Keep this revision's scope while fitting its candidate to the length contract.".into(),
            excerpt: Some(direction),
            protection: super::narrative_context::ContextProtection::Protected,
        }];
        let observations = request.observations.clone();
        let sink = request.candidate_sink.clone();
        Box::pin(async move {
            let agents = super::short_fiction_reviser::ReviseChapterAgents {
                chat: &chat,
                model: &model,
                defaults: &defaults,
            };
            let input = super::short_fiction_reviser::ReviseChapterInput {
                chapter_content: &request.pending_content,
                chapter_number: request.chapter_number,
                observations: &observations,
                mode: super::short_fiction_reviser::ReviseMode::SpotFix,
                language,
                chapter_title: None,
                instruction: Some(""),
                context_package_chapter: request.chapter_number,
                selected_context: &selected_context,
                length_spec: Some(&length_spec),
                candidate_sink: sink.clone(),
            };
            let output = super::short_fiction_reviser::revise_chapter(&agents, input, None)
                .await
                .map_err(|message| {
                    StructuredToolError::coded("SHORT_REVISION_SUBMIT_FAILED", message)
                })
                .map_err(|error| error.message)?;
            let accepted = sink
                .map(|sink| sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone())
                .unwrap_or_default();
            Ok(ReviseChapterOutcome {
                revised_content: output.revised_content,
                accepted_candidates: accepted,
            })
        })
    })
}

// ── continueDraft（完成环）────────────────────────────────────────────

pub struct ContinueDraftInput<'a> {
    pub title: Option<&'a str>,
    pub opening_hook_chars: Option<u32>,
    pub direction: &'a str,
    pub outline_markdown: &'a str,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub min_chapter_length: Option<u32>,
    pub max_chapter_length: Option<u32>,
    pub language: WritingLanguage,
    pub draft: &'a ShortFictionBatchDraft,
}

/// TS `onBatchComplete` 的共享形态（async 回调——runner 的落盘面）。
pub type OnBatchComplete = super::short_fiction_revision::RevisionProgressCallback;

/// TS `onRevisionProgress` 的共享形态（async 回调）。
pub type OnRevisionProgress = super::short_fiction_revision::RevisionProgressCallback;

/// TS `ShortFictionWriterAgent.continueDraft`：标题一致守卫 → 独立开篇场景
/// 修复环 → 缺章完成（completion plan → applyRevisionPlan）。
#[allow(clippy::too_many_lines)]
pub async fn writer_continue_draft(
    agents: &super::short_fiction_agents::ShortAgents,
    input: &ContinueDraftInput<'_>,
    revise_chapter: Option<&ReviseChapterFn>,
    on_batch_complete: OnBatchComplete,
) -> Result<ShortFictionBatchDraft, WriterChainError> {
    let mode = resolve_length_counting_mode(input.language);
    // TS：空批 merge——槽位 padding 至 chapterCount（标题/开篇保留）。
    let mut current = merge_short_fiction_batch(
        Some(input.draft),
        &input.draft.story_title,
        input.draft.opening_hook.as_deref(),
        &[],
        input.chapter_count,
        input.language,
    )
    .map_err(|error| WriterChainError::new("SHORT_BATCH_INVALID", error.message))?;
    if let Some(title) = input.title {
        if current.story_title.trim() != title.trim() {
            return Err(WriterChainError::new(
                "SHORT_TITLE_MISMATCH",
                "The saved draft has a different story title",
            ));
        }
    }
    // 独立开篇修复环（TS L456-481）。
    if let Some(target) = input.opening_hook_chars {
        let valid = validate_opening_hook(current.opening_hook.as_deref(), target, input.language).is_ok();
        if !valid {
        let (minimum, maximum) = opening_hook_length_contract(target, input.language);
        let state: Arc<Mutex<ShortFictionBatchDraft>> = Arc::new(Mutex::new(current));
            let system = "Write the requested independent opening scene before chapter one. Preserve the supplied title, story events and first chapter. Submit only the opening scene through the tool.";
            let user = {
                let guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                serde_json::json!({
                    "title": guard.story_title,
                    "targetLength": target,
                    "lengthContract": { "minimum": minimum, "maximum": maximum },
                    "direction": input.direction,
                    "outline": input.outline_markdown,
                    "firstChapter": guard.chapters.iter().find(|chapter| chapter.number == 1),
                    "currentOpeningHook": guard.opening_hook,
                })
                .to_string()
            };
            let complete_for_validate = on_batch_complete.clone();
            let state_for_validate = Arc::clone(&state);
            let (target_for_validate, minimum_for_validate, maximum_for_validate) =
                (target, minimum, maximum);
            let language_for_validate = input.language;
            let mode_for_validate = mode;
            let validate: ValidateFn = Arc::new(move |value: Value| {
                let state = Arc::clone(&state_for_validate);
                let on_batch_complete = Arc::clone(&complete_for_validate);
                Box::pin(async move {
                    let candidate = value
                        .get("openingHook")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    let distance = |text: Option<&str>| {
                        let count = count_chapter_length(text.unwrap_or(""), mode_for_validate);
                        std::cmp::max(
                            minimum_for_validate.saturating_sub(count),
                            count.saturating_sub(maximum_for_validate),
                        )
                    };
                    let improves = {
                        let guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        distance(Some(&candidate)) <= distance(guard.opening_hook.as_deref())
                    };
                    if improves {
                        {
                            let mut guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                            guard.opening_hook = Some(candidate.clone());
                            guard.raw_content = render_short_fiction_draft_markdown(&guard, language_for_validate)
                                .map_err(|error| WriterChainError::new("SHORT_RENDER_FAILED", error.message))
                                .map_err(|error| StructuredToolError::coded(&error.code, error.message))?;
                        }
                        let (snapshot, completed) = {
                            let guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                            let invalid: std::collections::BTreeSet<u32> =
                                find_incomplete_short_fiction_chapters(
                                    &guard,
                                    &ShortDraftBounds {
                                        title: None,
                                        opening_hook_chars: None,
                                        expected_chapters: None,
                                        min_chapter_length: None,
                                        max_chapter_length: None,
                                        language: Some(language_for_validate),
                                    },
                                )
                                .into_iter()
                                .collect();
                            let completed: Vec<u32> = guard
                                .chapters
                                .iter()
                                .filter(|chapter| !invalid.contains(&chapter.number))
                                .map(|chapter| chapter.number)
                                .collect();
                            (guard.clone(), completed)
                        };
                        on_batch_complete(&snapshot, &completed).await.map_err(|message| {
                            StructuredToolError::coded("WORKER_PERSIST_FAILED", message)
                        })?;
                    }
                    validate_opening_hook(Some(&candidate), target_for_validate, language_for_validate)
                        .map_err(|error| {
                            StructuredToolError::coded(error.code, error.message)
                        })?;
                    Ok(value)
                })
            });
            let outcome = run_structured_tool(
                Arc::clone(&agents.chat),
                &agents.model,
                &agents.messages_with_prelude(
                    vec![llm_system(system), llm_user(&user)],
                    true,
                ),
                agents.defaults.clone(),
                WorkerToolSpec {
                    name: "submit_short_opening_hook".into(),
                    description: "Submit the requested independent opening scene.".into(),
                    parameters_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "openingHook": { "type": "string", "minLength": 1 },
                        },
                        "required": ["openingHook"],
                        "additionalProperties": false,
                    }),
                    max_turns: None,
                },
                Some(validate),
                Vec::new(),
                StructuredRunOptions {
                    temperature: None,
                    max_tokens: Some(std::cmp::min(2048, agents.defaults.max_tokens)),
                },
            )
            .await
            .map_err(|error| WriterChainError { code: error.code, message: error.message })?;
            let hook = outcome
                .result
                .get("openingHook")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            let mut guard = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.opening_hook = Some(hook);
            guard.raw_content = render_short_fiction_draft_markdown(&guard, input.language)
                .map_err(|error| WriterChainError::new("SHORT_RENDER_FAILED", error.message))?;
            current = guard.clone();
        }
    }
    let missing = find_incomplete_short_fiction_chapters(
        &current,
        &ShortDraftBounds {
            title: None,
            opening_hook_chars: None,
            expected_chapters: None,
            min_chapter_length: input.min_chapter_length,
            max_chapter_length: input.max_chapter_length,
            language: Some(input.language),
        },
    );
    if missing.is_empty() {
        return Ok(current);
    }
    // 完成计划（TS L486-506）：宿主选出的无效槽位 → 修订执行链。
    let chapters: Vec<ShortRevisionPlanChapter> = missing
        .iter()
        .filter_map(|number| {
            current.chapters.iter().find(|chapter| chapter.number == *number).map(|chapter| {
                let length = count_chapter_length(&chapter.content, mode);
                let operation = if chapter.content.trim().is_empty() {
                    "Write the missing complete scene"
                } else if input
                    .max_chapter_length
                    .map(|max| length > max)
                    .unwrap_or(false)
                {
                    "Condense this existing chapter: retain the core conflict, evidence, decisions and transitions; remove repeated explanation and nonessential beats"
                } else {
                    "Complete this chapter with concrete action, dialogue and evidence"
                };
                let maximum = input
                    .max_chapter_length
                    .map(|max| max.to_string())
                    .unwrap_or_else(|| "not specified".into());
                ShortRevisionPlanChapter {
                    number: *number,
                    source_number: Some(if chapter.content.trim().is_empty() { 0 } else { *number }),
                    instruction: Some(format!(
                        "{operation}. Current measured length: {length}. Target: {}; minimum: {}; maximum: {}. Do not import the following chapter's events.",
                        input.chars_per_chapter,
                        input.min_chapter_length.unwrap_or(1),
                        maximum,
                    )),
                }
            })
        })
        .collect();
    let plan = ShortRevisionPlan {
        story_title: None,
        revision_brief: "Complete only the selected invalid chapters. Preserve valid chapters, the independent opening scene, chronology and evidence. Keep each chapter within its own story boundary.".into(),
        outline_markdown: Some(input.outline_markdown.to_string()),
        opening_hook: None,
        chapters,
    };
    let current_snapshot = current.clone();
    let complete_for_progress = on_batch_complete.clone();
    let language_for_progress = input.language;
    let bounds_for_progress = (input.min_chapter_length, input.max_chapter_length);
    let on_revision_progress: OnRevisionProgress = Arc::new(
        move |draft: &ShortFictionBatchDraft, _completed: &[u32]| {
            let current_snapshot = current_snapshot.clone();
            let complete_for_progress = Arc::clone(&complete_for_progress);
            let (min_bound, max_bound) = bounds_for_progress;
            let language_for_progress = language_for_progress;
            let draft_owned = draft.clone();
            Box::pin(async move {
                // TS：progress.draft===currentDraft 时跳过（无变化不重复通知）。
                if draft_owned == current_snapshot {
                    return Ok(());
                }
                let invalid: std::collections::BTreeSet<u32> =
                    find_incomplete_short_fiction_chapters(
                        &draft_owned,
                        &ShortDraftBounds {
                            title: None,
                            opening_hook_chars: None,
                            expected_chapters: None,
                            min_chapter_length: min_bound,
                            max_chapter_length: max_bound,
                            language: Some(language_for_progress),
                        },
                    )
                    .into_iter()
                    .collect();
                let completed: Vec<u32> = draft_owned
                    .chapters
                    .iter()
                    .filter(|chapter| !invalid.contains(&chapter.number))
                    .map(|chapter| chapter.number)
                    .collect();
                complete_for_progress(&draft_owned, &completed).await
            })
        },
    );
    let context = ApplyRevisionPlanContext {
        chat: &agents.chat,
        model: &agents.model,
        defaults: &agents.defaults,
        language: input.language,
        chapter_count: input.chapter_count,
        chars_per_chapter: input.chars_per_chapter,
        min_chapter_length: input.min_chapter_length,
        max_chapter_length: input.max_chapter_length,
        outline_markdown: input.outline_markdown,
        direction: input.direction,
        revise_chapter: revise_chapter.cloned(),
        on_revision_progress,
        chapter_numbers: Some(missing.clone()),
        author_scope: None,
        author_source: None,
        opening_hook_chars: input.opening_hook_chars,
    };
    let revised = apply_revision_plan(&context, &current, &plan)
        .await
        .map_err(WriterChainError::from_revision)?;
    Ok(revised.draft)
}

// ── planRevision / reviseDraft ───────────────────────────────────────

/// 894 号 scoped 分支装配：ReviseChapterFn 双分发（scoped=select→resolve→
/// ReviserAgent 注入；compress=892 回退面）。TS applyRevisionPlan scoped
/// 分支内联 `new ReviserAgent(this.ctx).reviseChapter(...)` 的注入等价物。
#[allow(clippy::too_many_arguments)]
pub fn scoped_or_compress_revise_chapter_fn(
    chat: super::structured_agent::ChatTurnFn,
    model: impl Into<String>,
    defaults: super::structured_agent::StructuredDefaults,
    language: WritingLanguage,
    chars_per_chapter: u32,
    min_chapter_length: Option<u32>,
    max_chapter_length: Option<u32>,
    direction: impl Into<String>,
    author_source: super::short_fiction_revision::ScopedAuthorSource,
) -> ReviseChapterFn {
    let model = model.into();
    let direction = direction.into();
    Arc::new(move |request: ReviseChapterRequest| {
        let chat = Arc::clone(&chat);
        let model = model.clone();
        let defaults = defaults.clone();
        let direction = direction.clone();
        let author_source = author_source.clone();
        let scoped_for_compress = request.scoped.is_none();
        let Some(scoped) = request.scoped else {
            // compress 回退（892 面）。author_source 仅在 scoped 分支消费
            // （scoped_for_compress=false 时 compress 装配不用它——克隆面由
            // 返回 future 的捕获面决定；此处显式消费防 unused 告警）。
            let _ = &author_source;
            let _ = scoped_for_compress;
            let compress = compress_revise_chapter_fn(
                Arc::clone(&chat),
                model,
                defaults,
                language,
                chars_per_chapter,
                min_chapter_length,
                request.max_chapter_length,
                direction,
            );
            return compress(request);
        };
        Box::pin(async move {
            // select 回调（SourceLocatorAgent——TS reviser 内联同款）。
            let chapter_number = request.chapter_number;
            let chapter_title = scoped.chapter_title.clone();
            let select_language = language;
            let select_chat = Arc::clone(&chat);
            let select_model = model.clone();
            let select_defaults = defaults.clone();
            let select = move |source: String, select_request: String| {
                let chat = Arc::clone(&select_chat);
                let model = select_model.clone();
                let defaults = select_defaults.clone();
                let title = chapter_title.clone();
                Box::pin(async move {
                    let agents = super::source_locator::LocatorAgents {
                        chat: &chat,
                        model: &model,
                        defaults: &defaults,
                    };
                    let identity = super::source_locator::SourceIdentity {
                        kind: "chapter",
                        chapter_number: Some(chapter_number),
                        title: title.as_deref(),
                        path: None,
                    };
                    super::source_locator::select(&agents, &source, &select_request, Some(&identity), select_language)
                        .await
                        .map_err(|error| error.message)
                })
                    as std::pin::Pin<
                        Box<dyn std::future::Future<Output = Result<crate::production::author_edit_scope::AuthorTextScope, String>> + Send>,
                    >
            };
            // TS body()：章题在场时剥文档壳（chapterDocumentBody）。
            let body = |content: &str| match &scoped.chapter_title {
                Some(title) => super::chapter_document::chapter_document_body(
                    content,
                    request.chapter_number,
                    title,
                    language,
                ),
                None => content.to_string(),
            };
            let original_body = body(&scoped.author_original_content);
            let current_body = body(&request.pending_content);
            let permission = crate::production::author_text_permission::resolve_author_text_permission(
                select,
                crate::production::author_text_permission::ResolveAuthorTextPermissionInput {
                    work_id: &scoped.author_work_id,
                    artifact_id: &scoped.author_artifact_id,
                    revision_id: &scoped.author_revision_id,
                    original_content: &original_body,
                    current_content: &current_body,
                    author_request: &scoped.author_request,
                    selector_version: 7,
                },
            )
            .await
            .map_err(|error| error.message)?;
            // TS：ranges[0] 有 startOffset 才走 authorized 直接执行；行区间
            // 面走 edit_ranges 自主（未授权 spot-fix 同形态）。
            let resolved = match permission.contract {
                crate::production::author_edit_scope::AuthorTextEditContract::Scoped(scoped_contract) => {
                    Some(super::short_fiction_reviser::ResolvedPermission {
                        scoped: Arc::new(scoped_contract),
                        original_selection_texts: permission.original_selection_texts,
                    })
                }
                crate::production::author_edit_scope::AuthorTextEditContract::Ranges(_) => None,
            };
            let length_spec = super::length_contract::build_length_spec(
                chars_per_chapter,
                language,
                (min_chapter_length, max_chapter_length),
            );
            let agents = super::short_fiction_reviser::ReviseChapterAgents {
                chat: &chat,
                model: &model,
                defaults: &defaults,
            };
            let input = super::short_fiction_reviser::ReviseChapterInput {
                chapter_content: &request.pending_content,
                chapter_number: request.chapter_number,
                observations: &request.observations,
                mode: super::short_fiction_reviser::ReviseMode::SpotFix,
                language,
                chapter_title: scoped.chapter_title.as_deref(),
                instruction: Some(&scoped.instruction_json),
                context_package_chapter: request.chapter_number,
                selected_context: &scoped.selected_context,
                length_spec: Some(&length_spec),
                candidate_sink: request.candidate_sink.clone(),
            };
            let output = super::short_fiction_reviser::revise_chapter(&agents, input, resolved.as_ref()).await?;
            let accepted = request
                .candidate_sink
                .map(|sink| sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone())
                .unwrap_or_default();
            Ok(ReviseChapterOutcome {
                revised_content: output.revised_content,
                accepted_candidates: accepted,
            })
        })
    })
}

pub struct PlanRevisionInput<'a> {
    pub direction: &'a str,
    pub outline_markdown: &'a str,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub min_chapter_length: Option<u32>,
    pub max_chapter_length: Option<u32>,
    pub opening_hook_chars: Option<u32>,
    pub language: WritingLanguage,
    pub draft: &'a ShortFictionBatchDraft,
    pub review: &'a str,
    pub chapter_numbers: Option<Vec<u32>>,
    pub author_scope: Option<&'a ShortAuthorScope>,
    /// TS `input.authorSource?.request`（author 权限链的原始作者请求）。
    pub author_source_request: Option<&'a str>,
}

/// TS `planRevision`（L220-242）：plan 代理环（maxTurns 6 + 双 supporting 工具
/// + validateRevisionPlan 回喂）→ normalize 扁平章槽。
#[allow(clippy::too_many_lines)]
pub async fn writer_plan_revision(
    agents: &super::short_fiction_agents::ShortAgents,
    input: &PlanRevisionInput<'_>,
) -> Result<ShortRevisionPlan, WriterChainError> {
    let system = "Plan only the changes required by the author's current request. Omit unchanged chapter fields, the opening scene and the outline. An opening-only edit needs no chapter edits. Chapter fields refer to FINAL slots: sourceNumber copies an original chapter unchanged, and an instruction revises that slot. Use sourceNumber 0 and a writing instruction for a new scene. When restructuring, provide the updated complete outline and map every displaced slot; each unchanged source scene can appear once. Use the supplied final chapter count, which includes the author's explicit updates. Preserve all other author constraints, chronology and scope. Reviewer and coordinator suggestions are candidate diagnoses, not permission or established story facts. Verify their proposed corrections against the original manuscript and the author's request before using them.";
    let mut user = serde_json::Map::new();
    user.insert("title".into(), Value::String(input.draft.story_title.clone()));
    user.insert(
        "userRequest".into(),
        Value::String(input
            .author_source_request
            .unwrap_or(input.direction)
            .to_string()),
    );
    user.insert(
        "allowedParts".into(),
        input
            .author_scope
            .map(|scope| serde_json::to_value(scope).unwrap_or(Value::Null))
            .unwrap_or(Value::Null),
    );
    if input.author_source_request.is_some() {
        // TS revisionGuidance：source 'coordinator' + authority 'advice_within_author_scope'。
        user.insert(
            "revisionGuidance".into(),
            serde_json::json!({
                "source": "coordinator",
                "instruction": input.direction,
                "authority": "advice_within_author_scope",
            }),
        );
    }
    user.insert("requiredChapterCount".into(), Value::from(input.chapter_count));
    user.insert(
        "allowedChapterNumbers".into(),
        input
            .chapter_numbers
            .as_ref()
            .map(|numbers| serde_json::to_value(numbers).unwrap_or(Value::Null))
            .unwrap_or(Value::Null),
    );
    user.insert("outline".into(), Value::String(input.outline_markdown.to_string()));
    user.insert("review".into(), Value::String(input.review.to_string()));
    user.insert(
        "originalManuscript".into(),
        serde_json::to_value(&input.draft.chapters).unwrap_or(Value::Null),
    );
    user.insert(
        "openingHook".into(),
        input
            .draft
            .opening_hook
            .as_ref()
            .map(|hook| Value::String(hook.clone()))
            .unwrap_or(Value::Null),
    );
    if let Some(target) = input.opening_hook_chars {
        let (minimum, maximum) = opening_hook_length_contract(target, input.language);
        user.insert(
            "openingHookTarget".into(),
            serde_json::json!({ "minimum": minimum, "maximum": maximum }),
        );
    }
    let user = Value::Object(user).to_string();
    let allow_story_title = input.author_scope.map(|scope| scope.story_title).unwrap_or(None).unwrap_or(false);
    let schema = short_revision_plan_submission_schema(
        input.chapter_count,
        input.draft.chapters.len() as u32,
        allow_story_title,
    );

    // validate：normalize → validateRevisionPlan 回喂。
    let draft_story_title = input.draft.story_title.clone();
    let draft_opening_hook = input.draft.opening_hook.clone();
    let source_count = input.draft.chapters.len() as u32;
    let chapter_count = input.chapter_count;
    let chapter_numbers = input.chapter_numbers.clone();
    let opening_hook_chars = input.opening_hook_chars;
    let language = input.language;
    let outline_markdown = input.outline_markdown.to_string();
    let author_scope_story_title =
        input.author_scope.map(|scope| scope.story_title.unwrap_or(false)).unwrap_or(false);
    let author_scope_opening = input.author_scope.map(|scope| scope.opening).unwrap_or(false);
    let validate: ValidateFn = Arc::new(move |value: Value| {
        // Fn 语义：每次调用重建借用（捕获值在闭包内克隆）。
        let chapter_numbers = chapter_numbers.clone();
        let outline_markdown = outline_markdown.clone();
        let draft_story_title = draft_story_title.clone();
        let draft_opening_hook = draft_opening_hook.clone();
        Box::pin(async move {
            let plan = normalize_plan_result(&value, chapter_count)
                .map_err(|message| StructuredToolError::coded("WORKER_SCHEMA_INVALID", message))?;
            validate_revision_plan(
                &plan,
                &RevisionPlanContext {
                    chapter_count,
                    source_chapter_count: source_count as usize,
                    chapter_numbers: chapter_numbers.as_deref(),
                    author_scope_story_title,
                    author_scope_opening,
                    outline_markdown: &outline_markdown,
                    opening_hook_chars,
                    language,
                    draft_story_title: &draft_story_title,
                    draft_opening_hook: draft_opening_hook.as_deref(),
                },
            )
            .map_err(|error| StructuredToolError::coded(error.code, error.message))?;
            Ok(value)
        })
    });
    let outcome = run_structured_tool(
        Arc::clone(&agents.chat),
        &agents.model,
        &agents.messages_with_prelude(vec![llm_system(system), llm_user(&user)], true),
        agents.defaults.clone(),
        WorkerToolSpec {
            name: "submit_short_revision_plan".into(),
            description: "Plan requested revision".into(),
            parameters_schema: schema,
            max_turns: Some(6),
        },
        Some(validate),
        vec![
            super::short_fiction_tools::review_calculation_tool(),
            super::short_fiction_tools::story_interval_tool(),
        ],
        StructuredRunOptions {
            temperature: None,
            max_tokens: Some(std::cmp::min(16384, agents.defaults.max_tokens)),
        },
    )
    .await
    .map_err(|error| WriterChainError { code: error.code, message: error.message })?;
    let plan = normalize_plan_result(&outcome.result, input.chapter_count)
        .map_err(|message| WriterChainError::new("SHORT_REVISION_PLAN_INVALID", message))?;
    validate_revision_plan(
        &plan,
        &RevisionPlanContext {
            chapter_count: input.chapter_count,
            source_chapter_count: input.draft.chapters.len(),
            chapter_numbers: input.chapter_numbers.as_deref(),
            author_scope_story_title,
            author_scope_opening,
            outline_markdown: input.outline_markdown,
            opening_hook_chars: input.opening_hook_chars,
            language: input.language,
            draft_story_title: &input.draft.story_title,
            draft_opening_hook: input.draft.opening_hook.as_deref(),
        },
    )
    .map_err(WriterChainError::from_revision)?;
    Ok(plan)
}

/// TS `normalize(result)`：扁平 chapter_N_sourceNumber/chapter_N_instruction →
/// 计划章列表（变化槽位才保留）。
fn normalize_plan_result(result: &Value, chapter_count: u32) -> Result<ShortRevisionPlan, String> {
    let story_title = result
        .get("storyTitle")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string);
    let revision_brief = result
        .get("revisionBrief")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let outline_markdown = result
        .get("outlineMarkdown")
        .and_then(Value::as_str)
        .map(str::to_string);
    let opening_hook = result.get("openingHook").and_then(Value::as_str).map(str::to_string);
    let chapters = (1..=chapter_count)
        .filter_map(|number| {
            let source_number = result.get(format!("chapter_{number}_sourceNumber")).and_then(Value::as_u64).map(|value| value as u32);
            let instruction = result
                .get(format!("chapter_{number}_instruction"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string);
            let changed = source_number.map(|source| source != number).unwrap_or(false)
                || instruction.is_some();
            if changed {
                Some(ShortRevisionPlanChapter { number, source_number, instruction })
            } else {
                None
            }
        })
        .collect();
    Ok(ShortRevisionPlan { story_title, revision_brief, outline_markdown, opening_hook, chapters })
}

pub struct ReviseDraftInput<'a> {
    pub direction: &'a str,
    pub outline_markdown: &'a str,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub min_chapter_length: Option<u32>,
    pub max_chapter_length: Option<u32>,
    pub opening_hook_chars: Option<u32>,
    pub language: WritingLanguage,
    pub draft: &'a ShortFictionBatchDraft,
    pub review: &'a str,
    pub chapter_numbers: Option<Vec<u32>>,
    pub author_scope: Option<&'a ShortAuthorScope>,
    pub author_source_request: Option<&'a str>,
    /// 894 scoped 分支：author 源引用（resolveAuthorTextPermission + 原稿）。
    pub author_source: Option<super::short_fiction_revision::ScopedAuthorSource>,
}

/// TS `reviseDraft`（L207-218）：单章快路径（无 opening/outline/title 权限）→
/// planRevision；再 `plan.outlineMarkdown ?? input.outlineMarkdown` 补齐。
pub async fn writer_revise_draft(
    agents: &super::short_fiction_agents::ShortAgents,
    input: &ReviseDraftInput<'_>,
    revise_chapter: Option<&ReviseChapterFn>,
    on_revision_progress: OnRevisionProgress,
) -> Result<(ShortFictionBatchDraft, String), WriterChainError> {
    let fast_path = matches!(input.chapter_numbers.as_deref(), Some([_]))
        && input.author_scope.map(|scope| !scope.opening && !scope.outline && !scope.story_title.unwrap_or(false)).unwrap_or(true);
    let submitted = if fast_path {
        let numbers = input.chapter_numbers.clone().unwrap_or_default();
        ShortRevisionPlan {
            story_title: None,
            revision_brief: input.direction.to_string(),
            outline_markdown: None,
            opening_hook: None,
            chapters: numbers
                .iter()
                .map(|number| ShortRevisionPlanChapter {
                    number: *number,
                    source_number: None,
                    instruction: Some(input.direction.to_string()),
                })
                .collect(),
        }
    } else {
        writer_plan_revision(
            agents,
            &PlanRevisionInput {
                direction: input.direction,
                outline_markdown: input.outline_markdown,
                chapter_count: input.chapter_count,
                chars_per_chapter: input.chars_per_chapter,
                min_chapter_length: input.min_chapter_length,
                max_chapter_length: input.max_chapter_length,
                opening_hook_chars: input.opening_hook_chars,
                language: input.language,
                draft: input.draft,
                review: input.review,
                chapter_numbers: input.chapter_numbers.clone(),
                author_scope: input.author_scope,
                author_source_request: input.author_source_request,
            },
        )
        .await?
    };
    let plan = ShortRevisionPlan {
        outline_markdown: Some(
            submitted
                .outline_markdown
                .clone()
                .unwrap_or_else(|| input.outline_markdown.to_string()),
        ),
        ..submitted
    };
    let context = ApplyRevisionPlanContext {
        chat: &agents.chat,
        model: &agents.model,
        defaults: &agents.defaults,
        language: input.language,
        chapter_count: input.chapter_count,
        chars_per_chapter: input.chars_per_chapter,
        min_chapter_length: input.min_chapter_length,
        max_chapter_length: input.max_chapter_length,
        outline_markdown: input.outline_markdown,
        direction: input.direction,
        revise_chapter: revise_chapter.cloned(),
        on_revision_progress,
        chapter_numbers: input.chapter_numbers.clone(),
        author_scope: input.author_scope.cloned(),
        author_source: input.author_source.clone(),
        opening_hook_chars: input.opening_hook_chars,
    };
    let revised = apply_revision_plan(&context, input.draft, &plan)
        .await
        .map_err(WriterChainError::from_revision)?;
    Ok((revised.draft, revised.outline_markdown))
}

fn llm_system(content: &str) -> LLMMessage {
    LLMMessage {
        role: LLMRole::System,
        content: content.into(),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn llm_user(content: &str) -> LLMMessage {
    LLMMessage {
        role: LLMRole::User,
        content: content.into(),
        tool_calls: None,
        tool_call_id: None,
    }
}

// 未使用导入防位（Observation 经 compress 装配使用；保留显式引用）。
#[allow(unused)]
fn _observation_shape(_: &Observation) {}

// ── 测试 ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::production::short_fiction::ShortFictionChapter;
    use crate::production::short_fiction_agents::ShortAgents;
    use crate::production::structured_agent::{
        ChatTurnResponse, ChatTurnToolCall, StructuredDefaults,
    };
    use std::collections::VecDeque;

    fn tokio() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    fn scripted(script: Vec<ChatTurnResponse>) -> ShortAgents {
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        let chat: crate::production::structured_agent::ChatTurnFn = Arc::new(move |_request| {
            let script = Arc::clone(&script);
            Box::pin(async move {
                let mut guard = script.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.pop_front().ok_or_else(|| {
                    crate::production::structured_agent::ChatTurnError {
                        message: "script exhausted".into(),
                        output_limit: false,
                    }
                })
            })
        });
        let mut agents = ShortAgents::new(
            chat,
            "test-model",
            StructuredDefaults { temperature: 0.7, max_tokens: 8192 },
        );
        agents.author_request = None;
        agents
    }

    fn response_tool(name: &str, arguments: Value) -> ChatTurnResponse {
        ChatTurnResponse {
            content: String::new(),
            tool_calls: vec![ChatTurnToolCall {
                id: format!("call-{name}"),
                name: name.to_string(),
                arguments: arguments.to_string(),
            }],
            finish_reason: None,
            usage: Default::default(),
        }
    }

    fn draft_of(chapters: &[(u32, &str, &str)]) -> ShortFictionBatchDraft {
        ShortFictionBatchDraft {
            story_title: "测试故事".into(),
            opening_hook: None,
            raw_content: String::new(),
            chapters: chapters
                .iter()
                .map(|(number, title, content)| ShortFictionChapter {
                    number: *number,
                    title: (*title).into(),
                    content: (*content).into(),
                    char_count: content.chars().count() as u32,
                })
                .collect(),
        }
    }

    #[test]
    fn continue_draft_completes_missing_chapter_via_plan() {
        // 章 2 缺失 → completion plan → submit_short_revision_chapter 补齐。
        let source = draft_of(&[(1, "第一章", "第一章正文，已经完成。")]);
        let agents = scripted(vec![response_tool(
            "submit_short_revision_chapter",
            serde_json::json!({ "title": "第二章", "content": "第二章补齐的正文，长度足够完成本章节内容要求。" }),
        )]);
        let revise = compress_revise_chapter_fn(
            Arc::clone(&agents.chat),
            "test-model",
            agents.defaults.clone(),
            WritingLanguage::Zh,
            30,
            Some(1),
            Some(200),
            "完成缺章",
        );
        let notified = Arc::new(Mutex::new(Vec::new()));
        let notified_for_cb = Arc::clone(&notified);
        let on_batch: OnBatchComplete = Arc::new(
            move |draft: &ShortFictionBatchDraft, completed: &[u32]| {
                let notified_for_cb = Arc::clone(&notified_for_cb);
                let contents =
                    draft.chapters.iter().map(|chapter| chapter.content.clone()).collect::<Vec<_>>();
                let completed: Vec<u32> = completed.to_vec();
                Box::pin(async move {
                    notified_for_cb.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push((contents, completed));
                    Ok(())
                })
            },
        );
        let result = tokio().block_on(writer_continue_draft(
            &agents,
            &ContinueDraftInput {
                title: None,
                opening_hook_chars: None,
                direction: "完成缺章",
                outline_markdown: "# 大纲",
                chapter_count: 2,
                chars_per_chapter: 30,
                min_chapter_length: Some(1),
                max_chapter_length: None,
                language: WritingLanguage::Zh,
                draft: &source,
            },
            Some(&revise),
            on_batch,
        ))
        .unwrap();
        assert_eq!(result.chapters.len(), 2);
        assert_eq!(result.chapters[1].title, "第二章");
        assert!(!result.chapters[1].content.trim().is_empty());
        assert!(result.raw_content.contains("第二章"));
        let notifications = notified.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!notifications.is_empty());
    }

    #[test]
    fn continue_draft_title_mismatch_fails() {
        let source = draft_of(&[(1, "第一章", "正文。")]);
        let agents = scripted(vec![]);
        let error = tokio().block_on(writer_continue_draft(
            &agents,
            &ContinueDraftInput {
                title: Some("别的标题"),
                opening_hook_chars: None,
                direction: "方向",
                outline_markdown: "# 大纲",
                chapter_count: 1,
                chars_per_chapter: 30,
                min_chapter_length: None,
                max_chapter_length: None,
                language: WritingLanguage::Zh,
                draft: &source,
            },
            None,
            Arc::new(|_, _| Box::pin(async { Ok(()) })),
        ))
        .unwrap_err();
        assert_eq!(error.code, "SHORT_TITLE_MISMATCH");
    }

    #[test]
    fn revise_draft_single_chapter_fast_path() {
        // chapterNumbers 单章 + 无 scope 权限 → 快路径（不进 plan 代理）。
        let source = draft_of(&[(1, "第一章", "原始正文。"), (2, "第二章", "原始正文二。")]);
        let agents = scripted(vec![response_tool(
            "submit_short_revision_chapter",
            serde_json::json!({ "title": "第一章改", "content": "修订后的正文内容，保持长度要求与事实一致。" }),
        )]);
        let (revised, outline) = tokio().block_on(writer_revise_draft(
            &agents,
            &ReviseDraftInput {
                direction: "重写第一章",
                outline_markdown: "# 大纲",
                chapter_count: 2,
                chars_per_chapter: 30,
                min_chapter_length: Some(1),
                max_chapter_length: None,
                opening_hook_chars: None,
                language: WritingLanguage::Zh,
                draft: &source,
                review: "",
                chapter_numbers: Some(vec![1]),
                author_scope: None,
                author_source_request: None,
                author_source: None,
            },
            None,
            Arc::new(|_, _| Box::pin(async { Ok(()) })),
        ))
        .unwrap();
        assert_eq!(revised.chapters[0].title, "第一章改");
        assert_eq!(revised.chapters[1].content, "原始正文二。");
        assert_eq!(outline, "# 大纲");
    }

    #[test]
    fn normalize_plan_reads_flat_slots() {
        let value = serde_json::json!({
            "revisionBrief": "改两章",
            "chapter_1_sourceNumber": 2,
            "chapter_2_instruction": "重写第二章",
            "chapter_3_sourceNumber": 3,
        });
        let plan = normalize_plan_result(&value, 3).unwrap();
        assert_eq!(plan.chapters.len(), 2);
        assert_eq!(plan.chapters[0].source_number, Some(2));
        assert_eq!(plan.chapters[1].instruction.as_deref(), Some("重写第二章"));
    }
}
