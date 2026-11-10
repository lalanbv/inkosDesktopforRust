//! LLM 翻译模型（llm-model.ts）：submit_translation 结构化工具提交（968 号 P2）
//! + 审校 JSON 面（P3 前形态）。
//!
//! 968 号：`translate_segments` 从单轮 JSON 文本升级为 TS 2.0 `runWorkerAgentTool`
//! 同款结构化工具环（[`crate::production::structured_agent::run_structured_tool`]）
//! ——`tool_choice:"required"` + `submit_translation`；validate 闭包=TS schema 面
//! （TypeBox 等价）+ `validateTranslatedSegments` 语义面（空/未知/重复/缺失即错，
//! 消息逐字），失败回喂模型重试（环预算 3 轮）——此前 Rust 单轮 JSON 静默丢弃坏段，
//! 部分漏译仍「成功」。章节标题翻译（chapterTitle 入端口与落盘）。审校仍为单轮 JSON
//! （P3 换 TranslationReviewAgent 独立源读+成对引证）。
//!
//! 技能注入：structured 环走 `router_chat_turn_fn`（直呼 streaming_client，旁路
//! AgentRouter::chat 的 OPERATION_SKILLS 出口）——构造 system 时显式读取当前激活集
//! 拼接（TS prepareWorkerMessages 同位；960 号 scope_operation_skills 作用域内）。
//! 备案：TS prepareWorkerMessages 的 authorRequest 前置/上下文编译分支翻译面不移植。

use std::sync::Arc;

use serde_json::{json, Value};

use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::production::structured_agent::{
    router_chat_turn_fn, run_structured_tool, StructuredDefaults, StructuredRunOptions,
    StructuredToolError, ValidateFn, WorkerToolSpec,
};
use crate::translation::types::*;

/// `createLLMTranslationModel`：经 AgentRouter 的 translator 端口
/// （温度 0.2 / 0.1；maxTokens 可覆盖）。968 号：持有 Arc（structured 环的
/// router_chat_turn_fn 需所有权）。
pub struct LlmTranslationModel {
    pub router: Arc<AgentRouter>,
    pub max_tokens: Option<u32>,
}

/// TS 环内 system 逐字（968 号对齐 2.0；旧「Return JSON only」system 退役）。
pub const TRANSLATION_SYSTEM_PROMPT: &str = "Translate the chapter title and all segments with the activated translation Skill.\nSubmit the complete translation through the translation result tool.";

/// TS `TranslationResultToolSchema`（TypeBox → JSON Schema 同形）。
fn translation_result_tool_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "chapterTitle": { "type": "string" },
            "segments": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "index": { "type": "integer", "minimum": 0 },
                        "target": { "type": "string", "minLength": 1 },
                        "notes": { "type": "string" },
                    },
                    "required": ["index", "target"],
                },
            },
            "glossary": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "minLength": 1 },
                        "target": { "type": "string", "minLength": 1 },
                        "note": { "type": "string" },
                    },
                    "required": ["source", "target"],
                },
            },
        },
        "required": ["segments"],
    })
}

fn schema_error(detail: String) -> StructuredToolError {
    StructuredToolError::coded("WORKER_SCHEMA_INVALID", format!("Invalid submit_translation arguments: {detail}"))
}

/// TypeBox schema 面（TS runWorkerAgentTool toolArgumentIssues 等价——明细形态备案
/// 不逐字，码面 WORKER_SCHEMA_INVALID 同）。
fn check_submission_schema(args: &Value) -> Result<(), StructuredToolError> {
    let object = args.as_object().ok_or_else(|| schema_error("expected an object".into()))?;
    if let Some(title) = object.get("chapterTitle") {
        if !title.is_string() {
            return Err(schema_error("/chapterTitle must be a string".into()));
        }
    }
    let segments = object
        .get("segments")
        .and_then(Value::as_array)
        .ok_or_else(|| schema_error("/segments must be an array".into()))?;
    for (position, item) in segments.iter().enumerate() {
        let record = item.as_object().ok_or_else(|| schema_error(format!("/segments/{position} must be an object")))?;
        if record.get("index").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).is_none() {
            return Err(schema_error(format!("/segments/{position}/index must be a non-negative integer")));
        }
        if record.get("target").and_then(Value::as_str).is_none_or(str::is_empty) {
            return Err(schema_error(format!("/segments/{position}/target must be a non-empty string")));
        }
        if record.get("notes").is_some_and(|notes| !notes.is_string()) {
            return Err(schema_error(format!("/segments/{position}/notes must be a string")));
        }
    }
    if let Some(glossary) = object.get("glossary") {
        let terms = glossary.as_array().ok_or_else(|| schema_error("/glossary must be an array".into()))?;
        for (position, term) in terms.iter().enumerate() {
            let record = term.as_object().ok_or_else(|| schema_error(format!("/glossary/{position} must be an object")))?;
            for field in ["source", "target"] {
                if record.get(field).and_then(Value::as_str).is_none_or(str::is_empty) {
                    return Err(schema_error(format!("/glossary/{position}/{field} must be a non-empty string")));
                }
            }
            if record.get("note").is_some_and(|note| !note.is_string()) {
                return Err(schema_error(format!("/glossary/{position}/note must be a string")));
            }
        }
    }
    Ok(())
}

/// TS `validateTranslatedSegments` 逐字语义（schema 面已过）：空 target（trim 后）/
/// 未知段号/重复段号/缺失段号一律错误；按源段序返回归一化段（target/notes trim）。
fn validate_translated_segments(
    items: &[Value],
    source_segments: &[TranslationSegment],
) -> Result<Vec<TranslatedSegmentItem>, StructuredToolError> {
    let source_index: std::collections::HashSet<u32> =
        source_segments.iter().map(|segment| segment.index).collect();
    let mut by_index: std::collections::HashMap<u32, TranslatedSegmentItem> =
        std::collections::HashMap::new();
    for item in items {
        let index = item.get("index").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
        let target = item.get("target").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if target.is_empty() {
            return Err(StructuredToolError::coded(
                "TRANSLATION_TARGET_EMPTY",
                format!("Translation returned an empty target for segment {index}."),
            ));
        }
        if !source_index.contains(&index) {
            return Err(StructuredToolError::plain(format!("Translation returned unknown segment index {index}.")));
        }
        if by_index.contains_key(&index) {
            return Err(StructuredToolError::plain(format!("Translation returned duplicate segment index {index}.")));
        }
        let notes = item
            .get("notes")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|notes| !notes.is_empty())
            .map(String::from);
        by_index.insert(index, TranslatedSegmentItem { index, target: target.to_string(), notes });
    }
    let missing: Vec<String> = source_segments
        .iter()
        .filter(|segment| !by_index.contains_key(&segment.index))
        .map(|segment| segment.index.to_string())
        .collect();
    if !missing.is_empty() {
        return Err(StructuredToolError::plain(format!(
            "Translation omitted segment index(es): {}.",
            missing.join(", ")
        )));
    }
    Ok(source_segments
        .iter()
        .filter_map(|segment| by_index.remove(&segment.index))
        .collect())
}

/// submit_translation 全量校验 → 归一化结果（validate 闭包与单测共用）。
pub(crate) fn validate_translation_submission(
    args: &Value,
    source_segments: &[TranslationSegment],
) -> Result<TranslateSegmentsOutput, StructuredToolError> {
    check_submission_schema(args)?;
    let items = args.get("segments").and_then(Value::as_array).cloned().unwrap_or_default();
    let segments = validate_translated_segments(&items, source_segments)?;
    Ok(TranslateSegmentsOutput {
        segments,
        glossary: parse_glossary(args.get("glossary")),
        chapter_title: args
            .get("chapterTitle")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(String::from),
    })
}

fn output_to_value(output: &TranslateSegmentsOutput) -> Value {
    json!({
        "chapterTitle": output.chapter_title,
        "segments": output.segments.iter().map(|segment| json!({
            "index": segment.index,
            "target": segment.target,
            "notes": segment.notes,
        })).collect::<Vec<_>>(),
        "glossary": output.glossary,
    })
}

fn output_from_value(value: &Value) -> TranslateSegmentsOutput {
    TranslateSegmentsOutput {
        segments: value
            .get("segments")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(TranslatedSegmentItem {
                            index: u32::try_from(item.get("index")?.as_u64()?).ok()?,
                            target: item.get("target")?.as_str()?.to_string(),
                            notes: item.get("notes").and_then(Value::as_str).map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        glossary: parse_glossary(value.get("glossary")),
        chapter_title: value.get("chapterTitle").and_then(Value::as_str).map(String::from),
    }
}

#[async_trait::async_trait]
impl TranslationModelPort for LlmTranslationModel {
    async fn translate_segments(
        &self,
        input: TranslateSegmentsInput<'_>,
    ) -> Result<TranslateSegmentsOutput, String> {
        let mut messages = vec![
            LLMMessage {
                role: LLMRole::System,
                content: TRANSLATION_SYSTEM_PROMPT.to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
            LLMMessage {
                role: LLMRole::User,
                content: serde_json::to_string_pretty(&json!({
                    "sourceLanguage": input.source_language,
                    "targetLanguage": input.target_language,
                    "chapterTitle": input.chapter_title,
                    "glossary": input.glossary,
                    "segments": input.segments.iter().map(|segment| {
                        json!({ "index": segment.index, "source": segment.source })
                    }).collect::<Vec<_>>(),
                }))
                .unwrap_or_default(),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        if let Some(skills) = crate::skills::production_bindings::current_operation_skills() {
            crate::agents::append_activated_skill_guidance(&mut messages, &skills);
        }
        let source_owned: Arc<Vec<TranslationSegment>> = Arc::new(input.segments.to_vec());
        let validate: ValidateFn = Arc::new(move |args: Value| {
            let source = Arc::clone(&source_owned);
            Box::pin(async move { validate_translation_submission(&args, &source).map(|output| output_to_value(&output)) })
        });
        let max_tokens = self.max_tokens.unwrap_or(8192);
        let endpoint = self.router.resolve("translator");
        let outcome = run_structured_tool(
            router_chat_turn_fn(Arc::clone(&self.router), "translator"),
            &endpoint.model,
            &messages,
            StructuredDefaults { temperature: 0.2, max_tokens },
            WorkerToolSpec {
                name: "submit_translation".to_string(),
                description: "Submit translated segments and glossary updates.".to_string(),
                parameters_schema: translation_result_tool_schema(),
                max_turns: None,
            },
            Some(validate),
            Vec::new(),
            StructuredRunOptions { temperature: None, max_tokens: Some(max_tokens) },
        )
        .await
        .map_err(|error| error.message)?;
        Ok(output_from_value(&outcome.result))
    }

    async fn review_chapter(
        &self,
        input: ReviewChapterInput<'_>,
    ) -> Result<ReviewChapterOutput, String> {
        // 969 号：单轮 JSON 审校退役——LLM 审校走 review.rs 审校 2.0 双段环（独立源读 +
        // 有源审校，auditor 端点）。
        crate::translation::review::review_translation_chapter(&self.router, &input).await
    }
}

/// 测试暴露面。
pub mod tests_support {
    use super::*;

    pub struct NoopModel;
    #[async_trait::async_trait]
    impl TranslationModelPort for NoopModel {
        async fn translate_segments(
            &self,
            _input: TranslateSegmentsInput<'_>,
        ) -> Result<TranslateSegmentsOutput, String> {
            Ok(TranslateSegmentsOutput::default())
        }
    }

    pub fn noop() -> NoopModel {
        NoopModel
    }

    pub fn parse_json_object_pub(raw: &str) -> Result<serde_json::Value, String> {
        parse_json_object(raw)
    }
}

/// `parseJsonObject`：整串 → fence 剥离 → 首个 { 到最后 } 子串（审校 JSON 面）。
fn parse_json_object(raw: &str) -> Result<serde_json::Value, String> {
    let trimmed = strip_fence(raw.trim());
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&trimmed) {
        if value.is_object() {
            return Ok(value);
        }
    }
    let start = trimmed.find('{');
    let end = trimmed.rfind('}');
    if let (Some(start), Some(end)) = (start, end) {
        if end > start {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&trimmed[start..=end]) {
                if value.is_object() {
                    return Ok(value);
                }
            }
        }
    }
    Err("Translation model did not return a JSON object.".to_string())
}

fn strip_fence(raw: &str) -> String {
    let re = regex::Regex::new(r"(?is)^```(?:json)?\s*(.*?)\s*```$").unwrap();
    re.captures(raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| raw.to_string())
}

fn parse_glossary(value: Option<&serde_json::Value>) -> Vec<TranslationGlossaryTerm> {
    let Some(items) = value.and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let record = item.as_object()?;
            let source = record.get("source")?.as_str()?.trim().to_string();
            let target = record.get("target")?.as_str()?.trim().to_string();
            if source.is_empty() || target.is_empty() {
                return None;
            }
            Some(TranslationGlossaryTerm {
                source,
                target,
                note: record
                    .get("note")
                    .and_then(|n| n.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Vec<TranslationSegment> {
        vec![
            TranslationSegment { index: 1, source: "a".into(), target: None, notes: None },
            TranslationSegment { index: 2, source: "b".into(), target: None, notes: None },
        ]
    }

    /// 968 号：TS validateTranslatedSegments 逐字五形态 + 归一化（按源段序/trim/标题）。
    #[test]
    fn submission_semantic_faces_match_ts() {
        let ok = validate_translation_submission(
            &json!({ "chapterTitle": "  第一章  ", "segments": [ { "index": 2, "target": "乙", "notes": " 注 " }, { "index": 1, "target": " 甲 " } ], "glossary": [ { "source": " Mana ", "target": "魔力" } ] }),
            &source(),
        )
        .unwrap();
        assert_eq!(ok.chapter_title.as_deref(), Some("第一章"));
        assert_eq!(ok.segments.iter().map(|s| s.index).collect::<Vec<_>>(), [1, 2], "按源段序");
        assert_eq!(ok.segments[0].target, "甲");
        assert_eq!(ok.segments[1].notes.as_deref(), Some("注"));
        assert_eq!(ok.glossary[0].source, "Mana");
        let err = |args: Value| validate_translation_submission(&args, &source()).unwrap_err();
        let unknown = err(json!({ "segments": [ { "index": 9, "target": "x" } ] }));
        assert_eq!(unknown.message, "Translation returned unknown segment index 9.");
        let duplicate = err(json!({ "segments": [ { "index": 1, "target": "a" }, { "index": 1, "target": "b" } ] }));
        assert_eq!(duplicate.message, "Translation returned duplicate segment index 1.");
        let missing = err(json!({ "segments": [ { "index": 2, "target": "b" } ] }));
        assert_eq!(missing.message, "Translation omitted segment index(es): 1.");
        let empty = err(json!({ "segments": [ { "index": 1, "target": "  " }, { "index": 2, "target": "b" } ] }));
        assert_eq!(empty.message, "Translation returned an empty target for segment 1.");
        assert_eq!(empty.code.as_deref(), Some("TRANSLATION_TARGET_EMPTY"));
    }

    /// 968 号：schema 面（TypeBox 等价）——码面 WORKER_SCHEMA_INVALID，先于语义面。
    #[test]
    fn submission_schema_faces() {
        for args in [
            json!([]),
            json!({}),
            json!({ "segments": [ 1 ] }),
            json!({ "segments": [ { "index": -1, "target": "a" } ] }),
            json!({ "segments": [ { "index": 1, "target": "" } ] }),
            json!({ "segments": [ { "index": 1, "target": "a", "notes": 3 } ] }),
            json!({ "chapterTitle": 5, "segments": [] }),
            json!({ "segments": [], "glossary": [ { "source": "", "target": "x" } ] }),
        ] {
            let error = validate_translation_submission(&args, &source()).unwrap_err();
            assert_eq!(error.code.as_deref(), Some("WORKER_SCHEMA_INVALID"), "{args}");
        }
    }
}
