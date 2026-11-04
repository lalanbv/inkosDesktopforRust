//! ReviserAgent.reviseChapter（891 号 M3c 终）：`packages/core/src/agents/reviser.ts`
//! 新形态（252 行）的短篇链消费面——spot-fix 行区间替换合同 + replacementBudget
//! 预算 + submitRewrite 长度合同。
//!
//! ## 范围裁定（886 §三 M3c 终拆分备案）
//! - 本模块 = reviser 的**双提交面**（submitSpotFix 两段结构化 + submitRewrite
//!   长度合同）+ buildRevisionProtocol 提示词 + replacementBudget 预算数学。
//! - author 权限链（SourceLocatorAgent.select→resolveAuthorTextPermission）经
//!   `AuthorPermissionResolver` 注入（890 基座装配；短篇链场景 = authorSource
//!   已知 → 跳过 locator 直入 resolve——由调用方在 revise 选项携带 resolved
//!   contract，本模块消费 `Option<AuthorTextEditContract>`）。
//! - 旧 reviser.rs（write-next 主链活体）不动。

use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

use super::length_contract::{assert_chapter_length, ShortLengthSpec};
use super::narrative_context::{render_narrative_selected_context, ContextSource};
use super::structured_agent::{
    run_structured_tool, ChatTurnUsage, StructuredDefaults, StructuredRunOptions, ValidateFn,
    WorkerToolSpec,
};
use crate::llm::provider::LLMMessage;
use crate::models::length_governance::LengthCountingMode;
use crate::models::observation::Observation;
use crate::utils::language::WritingLanguage;
use crate::utils::length_metrics::count_chapter_length;
use crate::utils::source_text::number_source_lines;

/// spot-fix 两段合同的联合形态（plan 段=行区间；authorized/恢复段=scoped）。
pub enum EditPlanContract {
    Ranges(super::text_range_edits::RangeEditContract),
    Scoped(super::text_range_edits::ScopedSelectionContract),
}

impl EditPlanContract {
    /// 固定骨架（区间外字节保留，替换位为空）。
    pub fn apply_empty(&self) -> Result<String, String> {
        match self {
            EditPlanContract::Ranges(ranges) => {
                let empty: Vec<String> = ranges.ranges.iter().map(|_| String::new()).collect();
                ranges.apply(&empty)
            }
            EditPlanContract::Scoped(scoped) => {
                let empty: Vec<String> = scoped.spans.iter().map(|_| String::new()).collect();
                scoped.apply(&empty)
            }
        }
    }

    /// 替换字段 schema（按字段名序）。
    pub fn parameters_schema(&self) -> Value {
        match self {
            EditPlanContract::Ranges(ranges) => ranges.parameters_schema(),
            EditPlanContract::Scoped(scoped) => scoped.parameters_schema(),
        }
    }

    /// 替换收集（按字段名序）。
    pub fn collect_replacements(&self, value: &Value) -> Result<Vec<String>, String> {
        match self {
            EditPlanContract::Ranges(ranges) => {
                let mut replacements = Vec::new();
                for range in &ranges.ranges {
                    replacements.push(
                        value
                            .get(format!("range_{}_content", range.index))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    );
                }
                Ok(replacements)
            }
            EditPlanContract::Scoped(scoped) => {
                let mut replacements = Vec::new();
                for index in 0..scoped.spans.len() {
                    replacements.push(
                        value
                            .get(format!("selection_{index}_text"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    );
                }
                Ok(replacements)
            }
        }
    }

    /// apply（字段序替换）。
    pub fn apply_replacements(&self, replacements: &[String]) -> Result<String, String> {
        match self {
            EditPlanContract::Ranges(ranges) => ranges.apply(replacements),
            EditPlanContract::Scoped(scoped) => scoped.apply(replacements),
        }
    }

    /// 字段长度明细（CHAPTER_LENGTH_OUT_OF_RANGE 回喂面）。
    pub fn field_lengths(&self, replacements: &[String], counting_mode: LengthCountingMode) -> Value {
        match self {
            EditPlanContract::Ranges(ranges) => {
                let field_lengths: Vec<Value> = ranges
                    .ranges
                    .iter()
                    .enumerate()
                    .map(|(index, range)| {
                        serde_json::json!({
                            "field": format!("range_{index}_content"),
                            "originalCount": count_chapter_length(&range.content, counting_mode),
                            "submittedCount": count_chapter_length(replacements.get(index).map(String::as_str).unwrap_or_default(), counting_mode),
                        })
                    })
                    .collect();
                serde_json::json!({ "fieldLengths": field_lengths })
            }
            EditPlanContract::Scoped(scoped) => {
                let field_lengths: Vec<Value> = scoped
                    .spans
                    .iter()
                    .enumerate()
                    .map(|(index, span)| {
                        serde_json::json!({
                            "field": format!("selection_{index}_text"),
                            "originalCount": count_chapter_length(&span.text, counting_mode),
                            "submittedCount": count_chapter_length(replacements.get(index).map(String::as_str).unwrap_or_default(), counting_mode),
                        })
                    })
                    .collect();
                serde_json::json!({ "fieldLengths": field_lengths })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviseMode {
    Polish,
    Rewrite,
    Rework,
    #[serde(rename = "anti-detect")]
    AntiDetect,
    #[serde(rename = "spot-fix")]
    SpotFix,
}

pub const DEFAULT_REVISE_MODE: ReviseMode = ReviseMode::Rewrite;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviseOutput {
    pub revised_content: String,
    pub word_count: u32,
    pub token_usage: TokenUsage,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

impl TokenUsage {
    fn add(&mut self, other: &ChatTurnUsage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.total_tokens += other.total_tokens;
    }
}

/// TS `buildRevisionProtocol`。
pub fn build_revision_protocol(mode: ReviseMode, language: WritingLanguage) -> String {
    let (mode_text, tail) = match (mode, language) {
        (ReviseMode::Polish, WritingLanguage::En) => {
            ("Edit wording only; keep facts, events, characters, and causality.", "Revise with the activated professional Skill and governed context. ")
        }
        (ReviseMode::Rewrite, WritingLanguage::En) => (
            "Rewrite the affected passages; rewrite the whole chapter only when the instruction spans it.",
            "Revise with the activated professional Skill and governed context. ",
        ),
        (ReviseMode::Rework, WritingLanguage::En) => {
            ("Scenes and conflict may be restructured within the supplied authority.", "Revise with the activated professional Skill and governed context. ")
        }
        (ReviseMode::AntiDetect, WritingLanguage::En) => {
            ("Change wording only while preserving story facts and causality.", "Revise with the activated professional Skill and governed context. ")
        }
        (ReviseMode::SpotFix, WritingLanguage::En) => (
            "Select only the source line ranges needed for the local request, then replace those ranges while preserving surrounding text.",
            "Revise with the activated professional Skill and governed context. ",
        ),
        (ReviseMode::Polish, WritingLanguage::Zh) => {
            ("只改文字表面，保持事实、事件、人物和因果。", "按已激活的专业 Skill 和 governed context 修订。")
        }
        (ReviseMode::Rewrite, WritingLanguage::Zh) => {
            ("重写受影响段落；只有要求跨越整章时才重写整章。", "按已激活的专业 Skill 和 governed context 修订。")
        }
        (ReviseMode::Rework, WritingLanguage::Zh) => {
            ("可在输入权威范围内重构场景与冲突。", "按已激活的专业 Skill 和 governed context 修订。")
        }
        (ReviseMode::AntiDetect, WritingLanguage::Zh) => {
            ("只调整文字表面，保持剧情事实和因果。", "按已激活的专业 Skill 和 governed context 修订。")
        }
        (ReviseMode::SpotFix, WritingLanguage::Zh) => (
            "先选择局部请求涉及的原文行范围，再替换这些范围，保留范围外正文。",
            "按已激活的专业 Skill 和 governed context 修订。",
        ),
    };
    if language == WritingLanguage::En {
        format!("{tail}{mode_text} Submit the result through the required tool.")
    } else {
        format!("{tail}{mode_text}通过指定结果工具提交。")
    }
}

/// TS `ReviseOutput` 提交 schema：ChapterRewriteToolSchema（`revisedContent`
/// minLength 1）。
fn chapter_rewrite_tool_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "revisedContent": { "type": "string", "minLength": 1, "description": "Complete revised chapter prose." },
        },
        "required": ["revisedContent"],
    })
}

/// 编辑区间提交 schema（`submit_chapter_edit_ranges`：ranges minItems 1，
/// additionalProperties false）。
fn edit_ranges_tool_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "ranges": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "properties": {
                        "startLine": { "type": "integer", "minimum": 1 },
                        "endLine": { "type": "integer", "minimum": 1 },
                    },
                    "required": ["startLine", "endLine"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["ranges"],
        "additionalProperties": false,
    })
}

pub struct ReviseChapterInput<'a> {
    pub chapter_content: &'a str,
    pub chapter_number: u32,
    pub observations: &'a [Observation],
    pub mode: ReviseMode,
    pub language: WritingLanguage,
    pub chapter_title: Option<&'a str>,
    pub instruction: Option<&'a str>,
    /// TS governed 上下文（selectedContext 渲染 + 章号）。
    pub context_package_chapter: u32,
    pub selected_context: &'a [ContextSource],
    pub length_spec: Option<&'a ShortLengthSpec>,
    /// TS `onCandidate`：候选收集器（Send+Sync 面——环内 push、环后逐个回调；
    /// TS 的实时 await 回调在 mock golden 面等价于终态逐个通知）。
    pub candidate_sink: Option<Arc<std::sync::Mutex<Vec<String>>>>,
}

pub struct ReviseChapterAgents<'a> {
    pub chat: &'a super::structured_agent::ChatTurnFn,
    pub model: &'a str,
    pub defaults: &'a StructuredDefaults,
}

/// 已解析的 author 权限（890 基座产物——scoped 合同 + 原选择文本）。
pub struct ResolvedPermission {
    pub scoped: Arc<super::text_range_edits::ScopedSelectionContract>,
    pub original_selection_texts: Vec<String>,
}

/// TS `ReviserAgent.reviseChapter` 短篇链消费面：governed 上下文必需。
/// 权限面经 `resolved_permission` 注入（None = 非 spot-fix/未授权 → rewrite
/// 长度合同路径）。
#[allow(clippy::too_many_arguments)]
pub async fn revise_chapter(
    agents: &ReviseChapterAgents<'_>,
    input: ReviseChapterInput<'_>,
    resolved_permission: Option<&ResolvedPermission>,
) -> Result<ReviseOutput, String> {
    let is_english = input.language == WritingLanguage::En;
    // TS reviewObservations 渲染（有观察逐条；无观察回退行）。
    let review_observations = if input.observations.is_empty() {
        if is_english {
            "- Follow the user's explicit revision instruction in the governed context.".to_string()
        } else {
            "- 按 governed context 中的用户明确修订要求执行。".to_string()
        }
    } else {
        input
            .observations
            .iter()
            .map(|issue| {
                let mut lines = vec![format!("- {}: {}", issue.code, issue.summary)];
                if !issue.evidence.is_empty() {
                    let label = if is_english { "Evidence" } else { "证据" };
                    lines.push(format!("  {label}: {}", issue.evidence.join("; ")));
                }
                lines.join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut observation_parts: Vec<String> = Vec::new();
    if let Some(instruction) = input.instruction.filter(|text| !text.trim().is_empty()) {
        observation_parts.push(instruction.to_string());
    }
    observation_parts.push(review_observations);
    let observation_list = observation_parts.join("\n\n");
    let context = render_narrative_selected_context(input.selected_context, input.language);
    let length_block = input.length_spec.map(|spec| {
        let current_count = count_chapter_length(input.chapter_content, spec.counting_mode);
        if is_english {
            format!(
                "\n## Length contract\n{}",
                serde_json::to_string(&serde_json::json!({
                    "target": spec.target,
                    "countingMode": spec.counting_mode,
                    "minChapterLength": spec.min_chapter_length,
                    "maxChapterLength": spec.max_chapter_length,
                    "currentCount": current_count,
                    "unit": "words",
                }))
                .unwrap_or_default()
            )
        } else {
            format!(
                "\n## 篇幅要求\n{}",
                serde_json::to_string(&serde_json::json!({
                    "target": spec.target,
                    "countingMode": spec.counting_mode,
                    "minChapterLength": spec.min_chapter_length,
                    "maxChapterLength": spec.max_chapter_length,
                    "currentCount": current_count,
                    "unit": "正文非空白字符，含标点，不含标题",
                }))
                .unwrap_or_default()
            )
        }
    });
    let system_prompt = build_revision_protocol(input.mode, input.language);
    let source = if input.mode == ReviseMode::SpotFix || resolved_permission.is_some() {
        number_source_lines(input.chapter_content, 1)
    } else {
        input.chapter_content.to_string()
    };
    let user_prompt = if is_english {
        format!(
            "Revise chapter {}.\n\n## Observations or instruction\n{}\n\n## Governed context\n{}{}\n\n## Current chapter\n{}",
            input.chapter_number, observation_list, context, length_block.unwrap_or_default(), source
        )
    } else {
        format!(
            "修订第{}章。\n\n## 观察或用户指令\n{}\n\n## 权威上下文\n{}{}\n\n## 当前章节\n{}",
            input.chapter_number, observation_list, context, length_block.unwrap_or_default(), source
        )
    };
    let messages = vec![llm_system(&system_prompt), llm_user(&user_prompt)];
    // TS outputBudget = min(defaults.maxTokens, max(8192, ceil((target??len)*4)+8192))。
    let target_est = input
        .length_spec
        .map(|spec| spec.target as usize)
        .unwrap_or(input.chapter_content.len());
    let output_budget = std::cmp::min(
        agents.defaults.max_tokens,
        std::cmp::max(8192u32, ((target_est as u64 * 4).div_ceil(1)) as u32 + 8192),
    );

    let spot_fix = input.mode == ReviseMode::SpotFix || resolved_permission.is_some();
    let resolved_scoped = resolved_permission.map(|resolved| resolved.scoped.clone());
    if spot_fix {
        submit_spot_fix(agents, &messages, &input, resolved_scoped.as_deref(), output_budget).await
    } else {
        submit_rewrite(agents, &messages, &input, output_budget).await
    }
}

/// TS `submitSpotFix` 两段：authorized 注入=直接 scoped 合同；否则先选编辑
/// 区间（4096 预算）→ 行区间合同 → 骨架+replacement 预算 → 逐区间替换提交
///（CHAPTER_LENGTH_OUT_OF_RANGE 回喂 instruction+fieldLengths）。
#[allow(clippy::too_many_lines)]
async fn submit_spot_fix(
    agents: &ReviseChapterAgents<'_>,
    messages: &[LLMMessage],
    input: &ReviseChapterInput<'_>,
    resolved_scoped: Option<&super::text_range_edits::ScopedSelectionContract>,
    max_tokens: u32,
) -> Result<ReviseOutput, String> {
    let original_chapter = input.chapter_content;
    let mut usage = TokenUsage::default();
    let plan_contract: std::sync::Arc<EditPlanContract> = std::sync::Arc::new(if let Some(scoped) = resolved_scoped {
        EditPlanContract::Scoped(scoped.clone())
    } else {
        let plan = run_structured_tool(
            Arc::clone(agents.chat),
            agents.model,
            messages,
            agents.defaults.clone(),
            WorkerToolSpec {
                name: "submit_chapter_edit_ranges".into(),
                description: "Select the smallest non-overlapping inclusive source line ranges needed for the requested local changes. Do not submit prose or select unrelated passages.".into(),
                parameters_schema: edit_ranges_tool_schema(),
                max_turns: None,
            },
            None,
            Vec::new(),
            StructuredRunOptions {
                temperature: None,
                max_tokens: Some(std::cmp::min(max_tokens, 4096)),
            },
        )
        .await
        .map_err(|error| error.message)?;
        usage.add(&plan.usage);
        let ranges: Vec<crate::production::text_range_edits::TextEditRange> = plan
            .result
            .get("ranges")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|range| crate::production::text_range_edits::TextEditRange {
                        start_line: range.get("startLine").and_then(Value::as_u64).unwrap_or(0) as u32,
                        end_line: range.get("endLine").and_then(Value::as_u64).unwrap_or(0) as u32,
                    })
                    .collect()
            })
            .unwrap_or_default();
        EditPlanContract::Ranges(super::text_range_edits::text_range_edit_contract(original_chapter, &ranges)?)
    });
    // 骨架（区间外字节保留）与 replacement 预算。
    let fixed_content = plan_contract.apply_empty()?;
    let fixed_length = input
        .length_spec
        .map(|spec| count_chapter_length(&fixed_content, spec.counting_mode));
    let replacement_budget: Option<Value> = input.length_spec.and_then(|spec| {
        let fixed_length = fixed_length?;
        let mut budget = serde_json::json!({
            "countingMode": spec.counting_mode,
            "fixedContentLength": fixed_length,
            "unit": if spec.counting_mode == crate::models::length_governance::LengthCountingMode::EnWords {
                "words"
            } else {
                "non-whitespace-characters"
            },
            "minimum": spec.min_chapter_length.unwrap_or(0).saturating_sub(fixed_length),
        });
        if let Some(max) = spec.max_chapter_length {
            budget["maximum"] = json!(max.saturating_sub(fixed_length));
        }
        Some(budget)
    });
    if let Some(budget) = &replacement_budget {
        if budget.get("maximum").and_then(Value::as_i64).map(|max| max < 0).unwrap_or(false) {
            return Err(serde_json::json!({
                "code": "CHAPTER_EDIT_SCOPE_CONFLICT",
                "replacementBudget": budget,
            })
            .to_string());
        }
    }
    let mut revise_messages = messages.to_vec();
    revise_messages.push(llm_user(
        &serde_json::json!({
            "editableRanges": plan_contract_editable_ranges(&plan_contract),
            "replacementBudget": replacement_budget,
            "instruction": "Submit only replacement text in each named field. These selections identify the original authorized source, even if the current revision split a selection into multiple paragraphs. Continue improving the current prose within that same permission. The replacement budget is shared by all fields, not per field. Allocate it across the selections before writing. Rephrase redundant wording to make room for the requested change while preserving established facts. For non-whitespace-characters, punctuation, Latin letters and digits also count; only whitespace is excluded. Keep the original trailing newline when present. Preserve all source bytes outside the selected ranges.",
        })
        .to_string(),
    ));
    let spec_for_validate = input.length_spec.cloned();
    let original_for_validate = original_chapter.to_string();
    let candidate_sink: Option<Arc<std::sync::Mutex<Vec<String>>>> =
        input.candidate_sink.clone();
    let plan_for_validate = std::sync::Arc::clone(&plan_contract);
    let fixed_length_for_validate = fixed_length;
    let budget_for_validate = replacement_budget.clone();
    let validate: ValidateFn = Arc::new(move |value: Value| {
        let plan_for_validate = std::sync::Arc::clone(&plan_for_validate);
        let spec_for_validate = spec_for_validate.clone();
        let original_for_validate = original_for_validate.clone();
        let candidate_sink = candidate_sink.clone();
        let budget_for_validate = budget_for_validate.clone();
        Box::pin(async move {
            let replacements = plan_for_validate.collect_replacements(&value)
                .map_err(super::structured_agent::StructuredToolError::plain)?;
            let candidate = plan_for_validate.apply_replacements(&replacements)
                .map_err(super::structured_agent::StructuredToolError::plain)?;
            if let Some(sink) = &candidate_sink {
                sink.lock().unwrap().push(candidate.clone());
            }
            if assert_chapter_length(&candidate, spec_for_validate.as_ref()).is_err() {
                let source_length =
                    count_chapter_length(&original_for_validate, counting_mode_for(spec_for_validate.as_ref()));
                let mode = counting_mode_for(spec_for_validate.as_ref());
                let submitted_length = fixed_length_for_validate
                    .map(|fixed| count_chapter_length(&candidate, mode).saturating_sub(fixed));
                let measured_budget = budget_for_validate.as_ref().map(|budget| {
                    let mut measured = budget.clone();
                    if let (Some(minimum), Some(submitted)) = (
                        budget.get("minimum").and_then(Value::as_i64),
                        submitted_length,
                    ) {
                        measured["increaseByAtLeast"] =
                            json!(std::cmp::max(0, minimum - submitted as i64));
                    }
                    if let (Some(maximum), Some(submitted)) = (
                        budget.get("maximum").and_then(Value::as_i64),
                        submitted_length,
                    ) {
                        measured["reduceByAtLeast"] =
                            json!(std::cmp::max(0, submitted as i64 - maximum));
                    }
                    measured
                });
                let mut details = serde_json::json!({
                    "code": "CHAPTER_LENGTH_OUT_OF_RANGE",
                    "scope": "selected_ranges",
                    "replacementBudget": measured_budget,
                    "source": { "length": source_length, "unchangedByThisAttempt": true },
                    "candidateCommitted": false,
                    "instruction": "Revise only the named replacement fields. Their individual counts locate the excess or shortage; the assembled chapter measurement is authoritative. Meet the entire measured reduction or increase in the next submission, not just part of it. Rephrase the selected passages when small deletions cannot recover enough space; preserve facts and the requested creative effect. Do not repeat the rejected wording unchanged or modify protected surrounding text.",
                });
                let field_lengths = plan_for_validate.field_lengths(&replacements, counting_mode_for(spec_for_validate.as_ref()));
                if let (Value::Object(map), Value::Object(extra)) = (&mut details, &field_lengths) {
                    for (key, value) in extra {
                        map.insert(key.clone(), value.clone());
                    }
                }
                return Err(super::structured_agent::StructuredToolError::coded(
                    "CHAPTER_LENGTH_OUT_OF_RANGE",
                    details.to_string(),
                ));
            }
            Ok(value)
        })
    });
    let outcome = run_structured_tool(
        Arc::clone(agents.chat),
        agents.model,
        &revise_messages,
        agents.defaults.clone(),
        WorkerToolSpec {
            name: "submit_chapter_range_replacements".into(),
            description: "Submit replacement prose only within the author-authorized source selections, using their named fields.".into(),
            parameters_schema: plan_contract.parameters_schema(),
            max_turns: None,
        },
        Some(validate),
        Vec::new(),
        StructuredRunOptions { temperature: None, max_tokens: Some(max_tokens) },
    )
    .await
    .map_err(|error| error.message)?;
    usage.add(&outcome.usage);
    let replacements = plan_contract.collect_replacements(&outcome.result)?;
    let revised_content = plan_contract.apply_replacements(&replacements)?;
    let word_count = input
        .length_spec
        .map(|spec| count_chapter_length(&revised_content, spec.counting_mode))
        .unwrap_or(revised_content.len() as u32);
    Ok(ReviseOutput {
        revised_content,
        word_count: word_count as u32,
        token_usage: usage,
    })
}

/// editableRanges 面（scoped=ranges JSON；ranges 合同=ranges JSON 同形）。
fn plan_contract_editable_ranges(contract: &EditPlanContract) -> Value {
    match contract {
        EditPlanContract::Ranges(ranges) => serde_json::to_value(&ranges.ranges).unwrap_or(Value::Null),
        EditPlanContract::Scoped(scoped) => serde_json::to_value(&scoped.spans).unwrap_or(Value::Null),
    }
}

/// TS `submitRewrite`：整章重写提交（CHAPTER_BODY_EMPTY 守卫 + 长度合同）。
async fn submit_rewrite(
    agents: &ReviseChapterAgents<'_>,
    messages: &[LLMMessage],
    input: &ReviseChapterInput<'_>,
    max_tokens: u32,
) -> Result<ReviseOutput, String> {
    let candidate_sink = input.candidate_sink.clone();
    let spec = input.length_spec.cloned();
    let validate: ValidateFn = Arc::new(move |value: Value| {
        let candidate_sink = candidate_sink.clone();
        let spec = spec.clone();
        Box::pin(async move {
            let revised_content = value
                .get("revisedContent")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if revised_content.trim().is_empty() {
                return Err(super::structured_agent::StructuredToolError::coded(
                    "CHAPTER_BODY_EMPTY",
                    "Submit chapter prose in addition to its title",
                ));
            }
            if let Some(sink) = &candidate_sink {
                sink.lock().unwrap().push(revised_content.clone());
            }
            assert_chapter_length(&revised_content, spec.as_ref())
                .map_err(|error| super::structured_agent::StructuredToolError::coded(
                    "CHAPTER_LENGTH_OUT_OF_RANGE",
                    error.message,
                ))?;
            let mut updated = value.clone();
            if let Value::Object(map) = &mut updated {
                map.insert("revisedContent".into(), Value::String(revised_content));
            }
            Ok(updated)
        })
    });
    let outcome = run_structured_tool(
        Arc::clone(agents.chat),
        agents.model,
        messages,
        agents.defaults.clone(),
        WorkerToolSpec {
            name: "submit_revised_chapter".into(),
            description: "Submit the complete revised chapter and addressed observations.".into(),
            parameters_schema: chapter_rewrite_tool_schema(),
            max_turns: None,
        },
        Some(validate),
        Vec::new(),
        StructuredRunOptions { temperature: None, max_tokens: Some(max_tokens) },
    )
    .await
    .map_err(|error| error.message)?;
    let revised_content = outcome
        .result
        .get("revisedContent")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(ReviseOutput {
        revised_content: revised_content.clone(),
        word_count: revised_content.len() as u32,
        token_usage: TokenUsage {
            prompt_tokens: outcome.usage.prompt_tokens,
            completion_tokens: outcome.usage.completion_tokens,
            total_tokens: outcome.usage.total_tokens,
        },
    })
}

fn counting_mode_for(spec: Option<&ShortLengthSpec>) -> LengthCountingMode {
    spec.map(|spec| spec.counting_mode)
        .unwrap_or(LengthCountingMode::ZhChars)
}

fn llm_system(content: &str) -> LLMMessage {
    LLMMessage { role: crate::llm::provider::LLMRole::System, content: content.into(), tool_calls: None, tool_call_id: None }
}

fn llm_user(content: &str) -> LLMMessage {
    LLMMessage { role: crate::llm::provider::LLMRole::User, content: content.into(), tool_calls: None, tool_call_id: None }
}

// ── 测试（mock 注入：同 prompt+同 mock 响应 → 同产物字节）────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::production::structured_agent::{ChatTurnError, ChatTurnResponse, ChatTurnToolCall};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// 返回原始件（chat/defaults/captured），由调用方本地组装 agents——
    /// 借用先于任何 move 的次序由调用方保证（TS fake 同构面）。
    fn scripted(
        script: Vec<ChatTurnResponse>,
    ) -> (
        super::super::structured_agent::ChatTurnFn,
        StructuredDefaults,
        Arc<Mutex<Vec<Value>>>,
    ) {
        let defaults = StructuredDefaults { temperature: 0.7, max_tokens: 8192 };
        let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_for_fn = Arc::clone(&captured);
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        let chat: super::super::structured_agent::ChatTurnFn = Arc::new(move |request| {
            let captured_for_fn = Arc::clone(&captured_for_fn);
            let script = Arc::clone(&script);
            let mut payload = serde_json::json!({
                "system": request.messages.first().map(|message| message.content.clone()),
                "toolChoice": request.tool_choice,
                "tools": request.tools,
            });
            payload["lastUser"] = request.messages.last().map(|message| message.content.clone()).into();
            Box::pin(async move {
                captured_for_fn.lock().unwrap().push(payload);
                let next = script.lock().unwrap().pop_front();
                next.ok_or_else(|| ChatTurnError { message: "script exhausted".into(), output_limit: false })
            })
        });
        (chat, defaults, captured)
    }

    fn tool_response(name: &str, payload: Value) -> ChatTurnResponse {
        ChatTurnResponse {
            content: String::new(),
            tool_calls: vec![ChatTurnToolCall {
                id: "c1".into(),
                name: name.into(),
                arguments: serde_json::to_string(&payload).unwrap(),
            }],
            finish_reason: Some("tool_calls".into()),
            usage: ChatTurnUsage { prompt_tokens: 5, completion_tokens: 5, total_tokens: 10 },
        }
    }

    /// spot-fix 双段 golden：轮 1 选区间 → 轮 2 提交替换 → 字节恒等。
    #[tokio::test]
    async fn spot_fix_two_phase_plan_then_replacements() {
        let chapter = "第一章标题行\n\n正文第一段。\n\n正文第二段。\n";
        let (chat, defaults, captured) = scripted(vec![
            tool_response("submit_chapter_edit_ranges", serde_json::json!({
                "ranges": [{ "startLine": 5, "endLine": 5 }],
            })),
            tool_response("submit_chapter_range_replacements", serde_json::json!({
                "range_0_content": "修订后的第二段。\n",
            })),
        ]);
        let agents = ReviseChapterAgents {
            chat: &chat,
            model: "test-model",
            defaults: &defaults,
        };
        let output = revise_chapter(
            &agents,
            ReviseChapterInput {
                chapter_content: chapter,
                chapter_number: 1,
                observations: &[],
                mode: ReviseMode::SpotFix,
                language: WritingLanguage::Zh,
                chapter_title: Some("第一章"),
                instruction: Some("把第二段改掉"),
                context_package_chapter: 1,
                selected_context: &[],
                length_spec: None,
                candidate_sink: None,
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(output.revised_content, "第一章标题行\n\n正文第一段。\n\n修订后的第二段。\n");
        // 双段：轮 1=submit_chapter_edit_ranges，轮 2=submit_chapter_range_replacements
        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["tools"][0]["function"]["name"], "submit_chapter_edit_ranges");
        assert_eq!(calls[1]["tools"][0]["function"]["name"], "submit_chapter_range_replacements");
        assert_eq!(calls[0]["toolChoice"], "required");
    }

    /// submitRewrite 长度合同：越界回喂 → 第二轮合规 → 成功（重试环活性证明）。
    #[tokio::test]
    async fn rewrite_length_contract_rejects_then_accepts() {
        let (chat, defaults, _captured) = scripted(vec![
            tool_response("submit_revised_chapter", serde_json::json!({
                "revisedContent": "太短",
            })),
            tool_response("submit_revised_chapter", serde_json::json!({
                "revisedContent": "这段修订后的正文达到了最少十个非空白字符的长度要求。",
            })),
        ]);
        let agents = ReviseChapterAgents {
            chat: &chat,
            model: "test-model",
            defaults: &defaults,
        };
        let spec = ShortLengthSpec {
            target: 20,
            counting_mode: crate::models::length_governance::LengthCountingMode::ZhChars,
            min_chapter_length: Some(10),
            max_chapter_length: None,
        };
        let output = revise_chapter(
            &agents,
            ReviseChapterInput {
                chapter_content: "原章正文。\n",
                chapter_number: 1,
                observations: &[],
                mode: ReviseMode::Rewrite,
                language: WritingLanguage::Zh,
                chapter_title: Some("第一章"),
                instruction: Some("扩写"),
                context_package_chapter: 1,
                selected_context: &[],
                length_spec: Some(&spec),
                candidate_sink: None,
            },
            None,
        )
        .await
        .unwrap();
        assert!(output.revised_content.contains("最少十个"));
    }

    /// 长度合同恒红变异：两轮都越界 → 必红。
    #[tokio::test]
    async fn rewrite_always_short_exhausts_to_error() {
        let (chat, defaults, _captured) = scripted(vec![
            tool_response("submit_revised_chapter", serde_json::json!({ "revisedContent": "短" })),
            tool_response("submit_revised_chapter", serde_json::json!({ "revisedContent": "仍短" })),
            tool_response("submit_revised_chapter", serde_json::json!({ "revisedContent": "继续短" })),
        ]);
        let agents = ReviseChapterAgents {
            chat: &chat,
            model: "test-model",
            defaults: &defaults,
        };
        let spec = ShortLengthSpec {
            target: 100,
            counting_mode: crate::models::length_governance::LengthCountingMode::ZhChars,
            min_chapter_length: Some(50),
            max_chapter_length: None,
        };
        let error = revise_chapter(
            &agents,
            ReviseChapterInput {
                chapter_content: "原章正文。\n",
                chapter_number: 1,
                observations: &[],
                mode: ReviseMode::Rewrite,
                language: WritingLanguage::Zh,
                chapter_title: Some("第一章"),
                instruction: Some("扩写"),
                context_package_chapter: 1,
                selected_context: &[],
                length_spec: Some(&spec),
                candidate_sink: None,
            },
            None,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("CHAPTER_LENGTH_OUT_OF_RANGE"), "{error}");
    }
}