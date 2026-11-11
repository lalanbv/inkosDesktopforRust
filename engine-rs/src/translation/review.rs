//! 翻译审校 2.0（969 号 P3）：TS `packages/core/src/translation/review.ts`
//! `TranslationReviewAgent` 忠实移植。
//!
//! 两段式：①**独立源读**——原文每 16 段一批经 `submit_translation_source_reading`
//! 结构化提交逐段独立重译（先读原文、不暴露现译，作为比对辅助，不得作证据）；
//! ②**有源审校**——`submitSourcedReview` 等价环：`record_review_observation`
//! 记录发现（宿主按行区间截取原文/现译摘录回执）+ `calculate_review_values`/
//! `calculate_story_interval` + 终提交 `submit_translation_review {summary,
//! observationCodes}`（未记录/重复代码回喂重试，环预算 16）。`issue` 级发现必须
//! **成对引证** original-* 与 target-* 源（TRANSLATION_REVIEW_PAIR_REQUIRED，记录即
//! 校验回喂）。产出 `{summary, observations}`（observations 为宿主截取的精确引文）。
//!
//! 路由：TS 审校模型 = `createAgentContext("auditor")`——Rust 同走 "auditor" 端点
//! （任务路由可为审校配更强模型）。技能：structured 环旁路 AgentRouter::chat 出口，
//! 构造消息时显式注入当前激活集（968 号同款）。温度沿用翻译审校面既有 0.1。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::observation::{
    resolve_observation_sources, Observation, ObservationAssessment, ReviewObservationSubmission,
};
use crate::production::structured_agent::{
    router_chat_turn_fn, run_structured_tool, StructuredDefaults, StructuredRunOptions,
    StructuredToolError, SupportingToolSpec, ValidateFn, WorkerToolSpec,
};
use crate::translation::types::*;
use crate::utils::source_text::number_source_lines;

const REVIEW_AGENT: &str = "auditor";
const REVIEW_TEMPERATURE: f64 = 0.1;
const READING_BATCH: usize = 16;

/// TS 逐字 system 串（双端金标准面）。
pub const SOURCE_READING_SYSTEM: &str = "Prepare the independent source reading using the activated translation Skill. Return only the requested segments in the named fields.";
pub const SOURCED_REVIEW_SYSTEM: &str = "Review the supplied segment pairs using the activated translation Skill. Submit findings only. Cite original and target sources for each issue; independent readings are comparison inputs and cannot be cited as evidence.";
const AUTHOR_GOALS_SYSTEM: &str = "The original author request and confirmed constraints define the acceptance requirements. Prior critiques and delegated revision suggestions are claims to recheck, not evidence that a defect exists or additional author requirements. Assess the current text independently. Explain why an issue conflicts with the author goals or the supplied text; classify a compatible interpretation or stylistic alternative as an observation. Baseline text establishes what changed, not what is still present in the current draft.";
const ARITHMETIC_SYSTEM: &str = "Before recording an arithmetic finding, use calculate_review_values and read its result. Submit independent calculations together. Verify the source values, units and time periods first; a correct calculation does not establish that unlike quantities should be compared.";
const PAIR_INSTRUCTION: &str = "Cite both the original and current target passages establishing each issue, or withdraw it. The independent reading is not source evidence.";

fn message(role: LLMRole, content: impl Into<String>) -> LLMMessage {
    LLMMessage { role, content: content.into(), tool_calls: None, tool_call_id: None }
}

fn with_skills(mut messages: Vec<LLMMessage>) -> Vec<LLMMessage> {
    if let Some(skills) = crate::skills::production_bindings::current_operation_skills() {
        crate::agents::append_activated_skill_guidance(&mut messages, &skills);
    }
    messages
}

fn record_instruction(final_tool: &str) -> String {
    format!("Record each finding with record_review_observation. Independent findings may be submitted together in one response. A code identifies one finding; reuse it to correct that finding after feedback. Read the exact source excerpts returned by the tool: resolving an address does not establish that its text supports the finding. Correct the references, assessment or explanation when the excerpts do not establish your claim. In a later response, call {final_tool} with the review summary and supported finding codes in the desired order. Do not embed a list of findings in a string. Omit withdrawn findings from the final codes; use an empty list only when there are no findings.")
}

/// TS validateObservations：issue 级发现须同时引证 original-* 与 target-*。
fn check_pairs(observations: &[Observation]) -> Result<(), StructuredToolError> {
    let unpaired: Vec<String> = observations
        .iter()
        .filter(|observation| observation.assessment == Some(ObservationAssessment::Issue))
        .filter(|observation| {
            let refs = observation.source_refs.as_deref().unwrap_or(&[]);
            !refs.iter().any(|r| r.source_id.starts_with("original-"))
                || !refs.iter().any(|r| r.source_id.starts_with("target-"))
        })
        .map(|observation| observation.code.clone())
        .collect();
    if unpaired.is_empty() {
        return Ok(());
    }
    Err(StructuredToolError::coded(
        "TRANSLATION_REVIEW_PAIR_REQUIRED",
        json!({ "code": "TRANSLATION_REVIEW_PAIR_REQUIRED", "observationCodes": unpaired, "instruction": PAIR_INSTRUCTION })
            .to_string(),
    ))
}

/// 宿主截取 + 成对引证校验（TS `resolve`——记录与终提交两处共用）。
fn resolve_and_check(
    submissions: &[ReviewObservationSubmission],
    sources: &BTreeMap<String, String>,
) -> Result<Vec<Observation>, StructuredToolError> {
    let normalized: Vec<ReviewObservationSubmission> = submissions.to_vec();
    let resolved = resolve_observation_sources(&normalized, sources)
        .map_err(|error| StructuredToolError::coded(&error.code, error.message))?;
    check_pairs(&resolved)?;
    Ok(resolved)
}

/// 段①：独立源读（16 段一批；返回 index → 独立译文）。
async fn independent_readings(
    router: &Arc<AgentRouter>,
    model: &str,
    input: &ReviewChapterInput<'_>,
) -> Result<BTreeMap<u32, String>, String> {
    let originals: Vec<Value> = input
        .segments
        .iter()
        .map(|segment| json!({ "index": segment.index, "source": segment.source }))
        .collect();
    let mut readings = BTreeMap::new();
    for batch in input.segments.chunks(READING_BATCH) {
        let indices: Vec<u32> = batch.iter().map(|segment| segment.index).collect();
        let mut properties = serde_json::Map::new();
        for index in &indices {
            properties.insert(
                format!("segment_{index}_target"),
                json!({ "type": "string", "minLength": 1, "description": format!("Translate original segment {index} completely.") }),
            );
        }
        let schema = json!({
            "type": "object",
            "properties": properties,
            "required": indices.iter().map(|index| format!("segment_{index}_target")).collect::<Vec<_>>(),
            "additionalProperties": false,
        });
        let mut user = json!({
            "sourceLanguage": input.source_language,
            "targetLanguage": input.target_language,
            "glossary": input.glossary,
            "originals": originals,
            "requestedIndices": indices,
        });
        if let Some(title) = input.source_chapter_title.filter(|title| !title.is_empty()) {
            user["chapterTitle"] = json!(title);
        }
        let messages = with_skills(vec![
            message(LLMRole::System, SOURCE_READING_SYSTEM),
            message(LLMRole::User, user.to_string()),
        ]);
        let batch_indices = indices.clone();
        let validate: ValidateFn = Arc::new(move |args: Value| {
            let batch_indices = batch_indices.clone();
            Box::pin(async move {
                let object = args.as_object().ok_or_else(|| {
                    StructuredToolError::coded("WORKER_SCHEMA_INVALID", "Invalid submit_translation_source_reading arguments: expected an object")
                })?;
                if let Some(extra) = object.keys().find(|key| {
                    !batch_indices.iter().any(|index| **key == format!("segment_{index}_target"))
                }) {
                    return Err(StructuredToolError::coded(
                        "WORKER_SCHEMA_INVALID",
                        format!("Invalid submit_translation_source_reading arguments: unknown field {extra}"),
                    ));
                }
                let mut empty = Vec::new();
                for index in &batch_indices {
                    match object.get(&format!("segment_{index}_target")).and_then(Value::as_str) {
                        Some(text) if !text.trim().is_empty() => {}
                        Some(_) => empty.push(*index),
                        None => {
                            return Err(StructuredToolError::coded(
                                "WORKER_SCHEMA_INVALID",
                                format!("Invalid submit_translation_source_reading arguments: /segment_{index}_target must be a non-empty string"),
                            ))
                        }
                    }
                }
                if !empty.is_empty() {
                    return Err(StructuredToolError::coded(
                        "TRANSLATION_TARGET_EMPTY",
                        json!({ "code": "TRANSLATION_TARGET_EMPTY", "segmentIndices": empty }).to_string(),
                    ));
                }
                Ok(args)
            })
        });
        let outcome = run_structured_tool(
            router_chat_turn_fn(Arc::clone(router), REVIEW_AGENT),
            model,
            &messages,
            StructuredDefaults { temperature: REVIEW_TEMPERATURE, max_tokens: 8192 },
            WorkerToolSpec {
                name: "submit_translation_source_reading".into(),
                description: "Submit an independent translation of each requested original segment.".into(),
                parameters_schema: schema,
                max_turns: None,
            },
            Some(validate),
            Vec::new(),
            StructuredRunOptions { temperature: None, max_tokens: Some(8192) },
        )
        .await
        .map_err(|error| error.message)?;
        for index in indices {
            let reading = outcome.result.get(format!("segment_{index}_target")).and_then(Value::as_str).unwrap_or_default();
            readings.insert(index, reading.to_string());
        }
    }
    Ok(readings)
}

/// 审校源表（TS `sources` Map 插入序：original-i/target-i 交错 → titles → glossary）。
fn review_sources(input: &ReviewChapterInput<'_>) -> (BTreeMap<String, String>, Vec<(String, String)>) {
    let mut ordered: Vec<(String, String)> = Vec::new();
    for segment in input.segments {
        ordered.push((format!("original-{}", segment.index), segment.source.clone()));
        ordered.push((format!("target-{}", segment.index), segment.target.clone().unwrap_or_default()));
    }
    if let Some(title) = input.source_chapter_title.filter(|title| !title.is_empty()) {
        ordered.push(("original-title".into(), title.to_string()));
    }
    ordered.push(("target-title".into(), input.chapter_title.to_string()));
    if !input.glossary.is_empty() {
        ordered.push(("glossary".into(), serde_json::to_string_pretty(input.glossary).unwrap_or_default()));
    }
    let map = ordered.iter().cloned().collect();
    (map, ordered)
}

/// TS `TranslationReviewAgent.review` 全流程。
pub async fn review_translation_chapter(
    router: &Arc<AgentRouter>,
    input: &ReviewChapterInput<'_>,
) -> Result<ReviewChapterOutput, String> {
    let endpoint = router.resolve(REVIEW_AGENT);
    let readings = independent_readings(router, &endpoint.model, input).await?;
    let (sources, ordered) = review_sources(input);

    let segment_blocks = input
        .segments
        .iter()
        .map(|segment| {
            [
                format!("## Segment {}", segment.index),
                format!("### Source: original-{}\n{}", segment.index, number_source_lines(&segment.source, 1)),
                format!(
                    "### Source: target-{}\n{}",
                    segment.index,
                    number_source_lines(segment.target.as_deref().unwrap_or(""), 1)
                ),
                format!(
                    "### Independent reading (comparison aid)\n{}",
                    readings.get(&segment.index).map(String::as_str).unwrap_or("")
                ),
            ]
            .join("\n\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let extra_sources = ordered
        .iter()
        .filter(|(id, _)| id == "glossary" || id.ends_with("-title"))
        .map(|(id, text)| format!("## Source: {id}\n{}", number_source_lines(text, 1)))
        .collect::<Vec<_>>()
        .join("\n\n");
    let final_tool = "submit_translation_review";
    let messages = with_skills(vec![
        message(LLMRole::System, SOURCED_REVIEW_SYSTEM),
        message(
            LLMRole::User,
            json!({
                "sourceLanguage": input.source_language,
                "targetLanguage": input.target_language,
                "chapterTitle": input.chapter_title,
            })
            .to_string(),
        ),
        message(LLMRole::User, segment_blocks),
        message(LLMRole::User, extra_sources),
        message(LLMRole::System, AUTHOR_GOALS_SYSTEM),
        message(LLMRole::System, ARITHMETIC_SYSTEM),
        message(LLMRole::System, record_instruction(final_tool)),
    ]);

    let recorded: Arc<Mutex<BTreeMap<String, ReviewObservationSubmission>>> = Arc::new(Mutex::new(BTreeMap::new()));
    let sources_arc = Arc::new(sources.clone());
    let recorded_for_support = Arc::clone(&recorded);
    let supporting = vec![
        SupportingToolSpec {
            name: "record_review_observation".into(),
            description: "Record an assessed finding after checking its reasoning and any arithmetic. This tool validates source addresses and returns their excerpts; it does not verify whether the claim is true. Resubmitting the same code replaces that finding.".into(),
            // TS categoryRequired → ArtifactReviewIndexToolSchema（category 必填）。
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string", "minLength": 1 },
                    "summary": { "type": "string" },
                    "category": { "enum": ["execution", "quality", "scope"] },
                    "assessment": { "enum": ["issue", "resolved", "unavailable", "observation"] },
                    "scope": { "type": "string" },
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
                "required": ["code", "summary", "category", "sourceRefs"],
                "additionalProperties": false,
            }),
            execute: Arc::new(move |arguments: Value| {
                let recorded = Arc::clone(&recorded_for_support);
                let sources = Arc::clone(&sources_arc);
                Box::pin(async move {
                    let submission: ReviewObservationSubmission = serde_json::from_value(arguments)
                        .map_err(|error| StructuredToolError::coded("WORKER_SCHEMA_INVALID", error.to_string()))?;
                    if submission.category.is_none() {
                        return Err(StructuredToolError::coded("WORKER_SCHEMA_INVALID", "category is required"));
                    }
                    let resolved = resolve_and_check(std::slice::from_ref(&submission), &sources)?;
                    let receipt = json!({
                        "code": submission.code,
                        "status": "source_resolved",
                        "sourceRefs": resolved[0].source_refs,
                        "instruction": "These are the actual selected excerpts. Check that they support this finding before keeping its code in the final review.",
                    });
                    recorded.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(submission.code.clone(), submission);
                    Ok(receipt)
                })
            }),
        },
        crate::production::short_fiction_tools::review_calculation_tool(),
        crate::production::short_fiction_tools::story_interval_tool(),
    ];
    let recorded_for_validate = Arc::clone(&recorded);
    let validate: ValidateFn = Arc::new(move |value: Value| {
        let recorded = Arc::clone(&recorded_for_validate);
        Box::pin(async move {
            let codes: Vec<String> = value
                .get("observationCodes")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let recorded = recorded.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let unknown: Vec<String> = codes.iter().filter(|code| !recorded.contains_key(*code)).cloned().collect();
            if !unknown.is_empty() || codes.iter().collect::<std::collections::BTreeSet<_>>().len() != codes.len() {
                return Err(StructuredToolError::coded(
                    "REVIEW_OBSERVATIONS_UNRECORDED",
                    json!({
                        "code": "REVIEW_OBSERVATIONS_UNRECORDED",
                        "unknownCodes": unknown,
                        "acceptedCodes": recorded.keys().cloned().collect::<Vec<_>>(),
                        "instruction": "Submit and correct each selected finding through record_review_observation, then finish with its accepted code exactly once. Remove withdrawn findings from the final list.",
                    })
                    .to_string(),
                ));
            }
            Ok(value)
        })
    });
    let max_tokens = std::cmp::min(4096 * 2, endpoint.max_tokens);
    let outcome = run_structured_tool(
        router_chat_turn_fn(Arc::clone(router), REVIEW_AGENT),
        &endpoint.model,
        &messages,
        StructuredDefaults { temperature: REVIEW_TEMPERATURE, max_tokens },
        WorkerToolSpec {
            name: final_tool.into(),
            description: "Finish the translation review with its recorded findings.".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "summary": { "type": "string" },
                    "observationCodes": { "type": "array", "items": { "type": "string", "minLength": 1 } },
                },
                "required": ["summary", "observationCodes"],
                "additionalProperties": false,
            }),
            max_turns: Some(16),
        },
        Some(validate),
        supporting,
        StructuredRunOptions { temperature: None, max_tokens: Some(max_tokens) },
    )
    .await
    .map_err(|error| error.message)?;
    let codes: Vec<String> = outcome
        .result
        .get("observationCodes")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let submissions: Vec<ReviewObservationSubmission> = {
        let recorded = recorded.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        codes.iter().filter_map(|code| recorded.get(code).cloned()).collect()
    };
    let observations = resolve_and_check(&submissions, &sources).map_err(|error| error.message)?;
    Ok(ReviewChapterOutput {
        summary: outcome.result.get("summary").and_then(Value::as_str).unwrap_or_default().to_string(),
        observations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::observation::ObservationSourceRef;

    fn observation(code: &str, assessment: ObservationAssessment, ids: &[&str]) -> Observation {
        Observation {
            code: code.into(),
            summary: "s".into(),
            evidence: Vec::new(),
            category: None,
            assessment: Some(assessment),
            scope: None,
            target_hash: None,
            target: None,
            source_refs: Some(ids.iter().map(|id| ObservationSourceRef { source_id: (*id).into(), quote: "q".into() }).collect()),
        }
    }

    /// 969 号：成对引证——issue 缺 original-*/target-* 任一即错；observation 级不要求。
    #[test]
    fn issue_findings_require_paired_sources() {
        assert!(check_pairs(&[observation("a", ObservationAssessment::Issue, &["original-1", "target-1"])]).is_ok());
        assert!(check_pairs(&[observation("b", ObservationAssessment::Observation, &["target-1"])]).is_ok());
        let error = check_pairs(&[
            observation("c", ObservationAssessment::Issue, &["target-1"]),
            observation("d", ObservationAssessment::Issue, &["original-2"]),
        ])
        .unwrap_err();
        assert_eq!(error.code.as_deref(), Some("TRANSLATION_REVIEW_PAIR_REQUIRED"));
        let details: Value = serde_json::from_str(&error.message).unwrap();
        assert_eq!(details["observationCodes"], json!(["c", "d"]));
        assert_eq!(details["instruction"], PAIR_INSTRUCTION);
    }

    /// 969 号：源表插入序（original/target 交错 → titles → glossary）与缺省面。
    #[test]
    fn review_sources_follow_ts_insertion_order() {
        let segments = vec![
            TranslationSegment { index: 1, source: "a".into(), target: Some("甲".into()), notes: None },
            TranslationSegment { index: 2, source: "b".into(), target: None, notes: None },
        ];
        let glossary = vec![TranslationGlossaryTerm { source: "Mana".into(), target: "魔力".into(), note: None }];
        let input = ReviewChapterInput {
            source_language: "en",
            target_language: "zh",
            chapter_title: "第一章",
            source_chapter_title: Some("Chapter 1"),
            segments: &segments,
            glossary: &glossary,
        };
        let (map, ordered) = review_sources(&input);
        let ids: Vec<&str> = ordered.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["original-1", "target-1", "original-2", "target-2", "original-title", "target-title", "glossary"]);
        assert_eq!(map["target-2"], "", "未译段 target 为空串");
        let no_title = ReviewChapterInput { source_chapter_title: None, glossary: &[], ..input };
        let (_, ordered) = review_sources(&no_title);
        assert!(!ordered.iter().any(|(id, _)| id == "original-title" || id == "glossary"));
    }
}
