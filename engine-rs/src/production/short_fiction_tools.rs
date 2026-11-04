//! 短篇管线 supporting 工具与修订契约（892 号 M3d）：
//! - `agents/story-interval.ts`（29 行）+ `agents/review-calculation.ts`（33 行）
//!   ——submitSourcedReview/planRevision 的 supportingTools。
//! - `agents/short-fiction-tool.ts` 的 `shortRevisionPlanSubmissionSchema`。
//! - `agents/short-revision-scope.ts` 的 `shortAuthorScopeRequest`（scope 选择
//!   代理消费面——893 revise 入口接线）。
//!
//! 全部为本地纯算术/纯校验工具：结果只核验算术，不建立故事事实（TS 注释逐字）。

use serde_json::{json, Value};
use std::sync::Arc;

use super::structured_agent::{StructuredToolError, SupportingToolSpec};

fn reject_unknown(value: &Value, fields: &[&str]) -> Result<(), StructuredToolError> {
    let map = value.as_object().ok_or_else(|| {
        StructuredToolError::coded("WORKER_SCHEMA_INVALID", "expected an object".to_string())
    })?;
    for key in map.keys() {
        if !fields.contains(&key.as_str()) {
            return Err(StructuredToolError::coded(
                "WORKER_SCHEMA_INVALID",
                format!("unknown field: {key}"),
            ));
        }
    }
    Ok(())
}

// ── story-interval.ts ────────────────────────────────────────────────

/// `calculate_story_interval`：同一既定周时钟上的本地日历算术；结果核验的是
/// 算术而非所选日期的事实（TS description 逐字）。
pub fn story_interval_tool() -> SupportingToolSpec {
    let parameters_schema = json!({
        "type": "object",
        "properties": {
            "start": {
                "type": "object",
                "properties": {
                    "clock": { "type": "string", "minLength": 1, "description": "The explicitly established common origin of these week numbers. Project weeks and employment weeks are different clocks unless their origins have been reconciled from the text." },
                    "week": { "type": "integer", "minimum": 1, "maximum": 10000 },
                    "weekday": { "type": "integer", "minimum": 1, "maximum": 7, "description": "Monday=1 through Sunday=7." },
                    "minuteOfDay": { "type": "integer", "minimum": 0, "maximum": 1439, "description": "Hour*60+minute, only when both event times are known. Omit for day precision." },
                },
                "required": ["clock", "week", "weekday"],
                "additionalProperties": false,
            },
            "end": {
                "type": "object",
                "properties": {
                    "clock": { "type": "string", "minLength": 1, "description": "The explicitly established common origin of these week numbers. Project weeks and employment weeks are different clocks unless their origins have been reconciled from the text." },
                    "week": { "type": "integer", "minimum": 1, "maximum": 10000 },
                    "weekday": { "type": "integer", "minimum": 1, "maximum": 7, "description": "Monday=1 through Sunday=7." },
                    "minuteOfDay": { "type": "integer", "minimum": 0, "maximum": 1439, "description": "Hour*60+minute, only when both event times are known. Omit for day precision." },
                },
                "required": ["clock", "week", "weekday"],
                "additionalProperties": false,
            },
        },
        "required": ["start", "end"],
        "additionalProperties": false,
    });
    SupportingToolSpec {
        name: "calculate_story_interval".into(),
        description: "Compute elapsed time between two events on the same established week clock, including weekday and optional time of day. Read the source anchors first. Missing or incompatible anchors cannot be resolved by subtracting week numbers. The result verifies arithmetic, not the truth of the selected dates.".into(),
        parameters_schema,
        execute: Arc::new(|arguments: Value| {
            Box::pin(async move {
                reject_unknown(&arguments, &["start", "end"])?;
                let start = arguments
                    .get("start")
                    .cloned()
                    .ok_or_else(|| StructuredToolError::coded("WORKER_SCHEMA_INVALID", "start is required".to_string()))?;
                let end = arguments
                    .get("end")
                    .cloned()
                    .ok_or_else(|| StructuredToolError::coded("WORKER_SCHEMA_INVALID", "end is required".to_string()))?;
                for (label, point) in [(&"start", &start), (&"end", &end)] {
                    reject_unknown(point, &["clock", "week", "weekday", "minuteOfDay"])?;
                    let _ = label;
                    let clock = point.get("clock").and_then(Value::as_str).unwrap_or_default();
                    if clock.trim().is_empty() {
                        return Err(StructuredToolError::coded("WORKER_SCHEMA_INVALID", "clock is required".to_string()));
                    }
                    let week = point.get("week").and_then(Value::as_i64).unwrap_or(0);
                    let weekday = point.get("weekday").and_then(Value::as_i64).unwrap_or(0);
                    if !(1..=10000).contains(&week) || !(1..=7).contains(&weekday) {
                        return Err(StructuredToolError::coded("WORKER_SCHEMA_INVALID", "week/weekday out of range".to_string()));
                    }
                    if let Some(minute) = point.get("minuteOfDay") {
                        let minute = minute.as_i64().unwrap_or(-1);
                        if !(0..=1439).contains(&minute) {
                            return Err(StructuredToolError::coded("WORKER_SCHEMA_INVALID", "minuteOfDay out of range".to_string()));
                        }
                    }
                }
                let start_clock = start.get("clock").and_then(Value::as_str).unwrap_or_default().trim().to_string();
                let end_clock = end.get("clock").and_then(Value::as_str).unwrap_or_default().trim().to_string();
                if start_clock.is_empty() || start_clock != end_clock {
                    return Err(StructuredToolError::coded(
                        "STORY_INTERVAL_CLOCK_MISMATCH",
                        "Relate the two clock origins using source evidence before comparing the dates.".to_string(),
                    ));
                }
                let start_minute = start.get("minuteOfDay").is_some();
                let end_minute = end.get("minuteOfDay").is_some();
                if start_minute != end_minute {
                    return Err(StructuredToolError::coded(
                        "STORY_INTERVAL_PRECISION_MISMATCH",
                        "Use the same precision for both endpoints; an unknown time is not midnight.".to_string(),
                    ));
                }
                let coordinate = |point: &Value| -> i64 {
                    let week = point.get("week").and_then(Value::as_i64).unwrap_or(1);
                    let weekday = point.get("weekday").and_then(Value::as_i64).unwrap_or(1);
                    let minute_of_day =
                        point.get("minuteOfDay").and_then(Value::as_i64).unwrap_or(0);
                    ((week - 1) * 7 + weekday - 1) * 1440 + minute_of_day
                };
                let minutes = coordinate(&end) - coordinate(&start);
                let absolute = minutes.abs();
                let magnitude = |total: i64, divisor: i64| total / divisor;
                let receipt = json!({
                    "kind": "story_interval",
                    "start": start,
                    "end": end,
                    "precision": if start_minute { "minutes" } else { "days" },
                    "direction": minutes.signum(),
                    "elapsedMinutes": minutes,
                    "elapsedDays": minutes as f64 / 1440.0,
                    "magnitude": {
                        "weeks": magnitude(absolute, 10080),
                        "days": magnitude(absolute % 10080, 1440),
                        "hours": magnitude(absolute % 1440, 60),
                        "minutes": absolute % 60,
                    },
                    "exactWholeWeeks": minutes % 10080 == 0,
                });
                Ok(receipt)
            })
        }),
    }
}

// ── review-calculation.ts ────────────────────────────────────────────

/// `calculate_review_values`：先算总/差/积/商再下算术结论。证据本地计算；对
/// 其来源的解释仍由审稿人负责，不求值任何代码或表达式（TS 注释逐字）。
pub fn review_calculation_tool() -> SupportingToolSpec {
    let parameters_schema = json!({
        "type": "object",
        "properties": {
            "operation": { "enum": ["add", "subtract", "multiply", "divide"] },
            "values": {
                "type": "array",
                "items": { "type": "number" },
                "minItems": 2,
                "maxItems": 64,
                "description": "Values in the same units, applied left to right. Negative values can represent deductions.",
            },
        },
        "required": ["operation", "values"],
        "additionalProperties": false,
    });
    SupportingToolSpec {
        name: "calculate_review_values".into(),
        description: "Calculate a total, difference, product or ratio before making an arithmetic finding. Select values from the source and keep units and time periods consistent. This verifies arithmetic only, not the choice of operands or their interpretation.".into(),
        parameters_schema,
        execute: Arc::new(|arguments: Value| {
            Box::pin(async move {
                reject_unknown(&arguments, &["operation", "values"])?;
                let operation = arguments
                    .get("operation")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let values = arguments
                    .get("values")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let invalid = || {
                    StructuredToolError::coded(
                        "REVIEW_CALCULATION_UNDEFINED",
                        "Supply finite values and a defined finite arithmetic result.".to_string(),
                    )
                };
                if values.len() < 2
                    || values.len() > 64
                    || values.iter().any(|value| {
                        value.as_f64().map(|number| !number.is_finite()).unwrap_or(true)
                    })
                {
                    return Err(invalid());
                }
                let mut result = values[0].as_f64().unwrap_or_default();
                for value in values.iter().skip(1) {
                    let value = value.as_f64().unwrap_or_default();
                    match operation.as_str() {
                        "add" => result += value,
                        "subtract" => result -= value,
                        "multiply" => result *= value,
                        "divide" => {
                            if value == 0.0 {
                                return Err(invalid());
                            }
                            result /= value;
                        }
                        _ => return Err(invalid()),
                    }
                }
                if !result.is_finite() {
                    return Err(invalid());
                }
                let receipt = json!({
                    "kind": "review_calculation",
                    "operation": operation,
                    "values": values,
                    "result": result,
                });
                Ok(receipt)
            })
        }),
    }
}

// ── short-fiction-tool.ts ────────────────────────────────────────────

/// TS `shortRevisionPlanSubmissionSchema`：扁平章槽提交面（长正文一律平铺字符串，
/// 不嵌 JSON 字符串）——`chapter_N_sourceNumber`/`chapter_N_instruction` 双槽。
pub fn short_revision_plan_submission_schema(
    chapter_count: u32,
    source_chapter_count: u32,
    allow_story_title: bool,
) -> Value {
    let source_count = source_chapter_count.max(1);
    let mut properties = serde_json::Map::new();
    if allow_story_title {
        properties.insert(
            "storyTitle".into(),
            json!({
                "type": "string",
                "minLength": 1,
                "description": "Revised book title grounded in the current manuscript, only when explicitly authorized by the author. Omit to preserve it.",
            }),
        );
    }
    properties.insert(
        "revisionBrief".into(),
        json!({
            "type": "string",
            "minLength": 1,
            "description": "Resolve shared facts and event order once for the selected revision. State the chosen corrections that all affected chapters must follow, rather than repeating unresolved alternatives. Preserve author constraints and facts outside the requested change.",
        }),
    );
    properties.insert(
        "outlineMarkdown".into(),
        json!({
            "type": "string",
            "minLength": 1,
            "description": "Update the complete outline when it contradicts the chosen, authorized corrections. Omit when it remains valid, including local prose edits. Preserve unrelated events and structure.",
        }),
    );
    properties.insert(
        "openingHook".into(),
        json!({ "type": "string", "minLength": 1 }),
    );
    for index in 1..=chapter_count {
        properties.insert(
            format!("chapter_{index}_sourceNumber"),
            json!({
                "type": "integer",
                "minimum": 0,
                "maximum": source_count,
                "description": format!("Original chapter to place in final slot {index}. Omit to retain this slot. Use 0 with a writing instruction for a new scene. A source mapping alone copies its prose unchanged."),
            }),
        );
        properties.insert(
            format!("chapter_{index}_instruction"),
            json!({
                "type": "string",
                "minLength": 1,
                "description": format!("Revise only final chapter {index} according to this instruction. Omit when its prose is unchanged."),
            }),
        );
    }
    json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    })
}

// ── short-revision-scope.ts ──────────────────────────────────────────

/// TS `ShortAuthorScopeSchema`（additionalProperties:false → deny_unknown_fields）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

/// TS `ShortAuthorScopeSchema` 的 JSON Schema 形态（请求 tools 数组元素）。
pub fn short_author_scope_schema() -> Value {
    json!({
    "type": "object",
    "properties": {
        "wholeManuscript": { "type": "boolean", "description": "True only when the author permits rewriting or restructuring the whole manuscript." },
        "storyTitle": { "type": "boolean", "description": "True only when the author explicitly permits changing the book title. Prose revision, review and packaging alone do not grant this permission." },
        "chapterNumbers": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "description": "Existing chapters whose prose the author permits editing. A selected chapter can still contain protected text." },
        "chapterTitleNumbers": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "description": "Only chapters whose titles the author explicitly permits changing. Omit when titles are protected." },
        "opening": { "type": "boolean", "description": "Permission to change the independent opening scene." },
        "outline": { "type": "boolean", "description": "Permission to change the saved story plan." },
    },
    "required": ["wholeManuscript", "chapterNumbers", "opening", "outline"],
    "additionalProperties": false,
    })
}

/// TS `shortAuthorScopeRequest`：(messages, tool)——scope 选择代理（893 revise
/// 入口接线）消费。validate：所选章号必须存在于成稿。
pub fn short_author_scope_request(
    draft: &super::short_fiction::ShortFictionBatchDraft,
    author_request: &str,
) -> (Vec<crate::llm::provider::LLMMessage>, super::structured_agent::WorkerToolSpec, ShortAuthorScopeValidate) {
    use crate::llm::provider::{LLMMessage, LLMRole};
    let manuscript = serde_json::to_value(draft).unwrap_or(Value::Null);
    let messages = vec![
        LLMMessage {
            role: LLMRole::System,
            content: "Identify which existing manuscript parts the author permits changing. Select chapter numbers from the supplied manuscript. Reviewing the whole story or refreshing sales packaging does not authorize rewriting it. A desired emotional or commercial effect does not expand an explicit selected chapter range. An opening-only or outline-only edit has no selected chapters. Select wholeManuscript only for an affirmative whole-manuscript rewrite or restructuring request; otherwise preserve unselected parts. Do not write a revision or diagnose quality.".into(),
            tool_calls: None,
            tool_call_id: None,
        },
        LLMMessage {
            role: LLMRole::System,
            content: "Editing specified words, dates, facts or passages across every chapter still has wholeManuscript=false: list every affected chapter and preserve its other prose. Whole-story coverage does not imply unrestricted rewriting. Permission to correct prose does not by itself include changing the saved outline or book title. A book-title-only request selects storyTitle and no chapters, chapter titles, opening or outline.".into(),
            tool_calls: None,
            tool_call_id: None,
        },
        LLMMessage {
            role: LLMRole::User,
            content: json!({ "authorRequest": author_request, "manuscript": manuscript }).to_string(),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let tool = super::structured_agent::WorkerToolSpec {
        name: "submit_short_author_scope".into(),
        description: "Select only the manuscript parts the author permits changing.".into(),
        parameters_schema: short_author_scope_schema(),
        max_turns: None,
    };
    (
        messages,
        tool,
        ShortAuthorScopeValidate {
            chapter_numbers: draft.chapters.iter().map(|chapter| chapter.number).collect(),
        },
    )
}

/// scope validate 的宿主侧输入（章号存在性表）。
pub struct ShortAuthorScopeValidate {
    pub chapter_numbers: Vec<u32>,
}

impl ShortAuthorScopeValidate {
    /// TS validate：越界章号 → SHORT_REVISION_OUT_OF_SCOPE。
    pub fn validate(&self, scope: &ShortAuthorScope) -> Result<(), StructuredToolError> {
        let mut selected = scope.chapter_numbers.clone();
        selected.extend(scope.chapter_title_numbers.clone().unwrap_or_default());
        if selected.iter().any(|number| !self.chapter_numbers.contains(number)) {
            return Err(StructuredToolError::coded(
                "SHORT_REVISION_OUT_OF_SCOPE",
                "Select existing chapter numbers.".to_string(),
            ));
        }
        Ok(())
    }
}

// ── 测试 ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tokio() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    fn story_interval_receipt_golden() {
        let tool = story_interval_tool();
        let receipt = tokio()
            .block_on((tool.execute)(json!({
                "start": { "clock": "project-week", "week": 2, "weekday": 1 },
                "end": { "clock": "project-week", "week": 2, "weekday": 3 },
            })))
            .unwrap();
        assert_eq!(receipt.get("kind").and_then(Value::as_str), Some("story_interval"));
        assert_eq!(receipt.get("elapsedMinutes").and_then(Value::as_i64), Some(2 * 1440));
        assert_eq!(receipt.get("precision").and_then(Value::as_str), Some("days"));
        assert_eq!(receipt.get("exactWholeWeeks").and_then(Value::as_bool), Some(false));
        let magnitude = receipt.get("magnitude").unwrap();
        assert_eq!(magnitude.get("days").and_then(Value::as_i64), Some(2));
    }

    #[test]
    fn story_interval_errors() {
        let tool = story_interval_tool();
        let error = tokio()
            .block_on((tool.execute)(json!({
                "start": { "clock": "a", "week": 2, "weekday": 1 },
                "end": { "clock": "b", "week": 2, "weekday": 3 },
            })))
            .unwrap_err();
        assert_eq!(error.code.as_deref(), Some("STORY_INTERVAL_CLOCK_MISMATCH"));
        let error = tokio()
            .block_on((tool.execute)(json!({
                "start": { "clock": "a", "week": 2, "weekday": 1, "minuteOfDay": 60 },
                "end": { "clock": "a", "week": 2, "weekday": 3 },
            })))
            .unwrap_err();
        assert_eq!(error.code.as_deref(), Some("STORY_INTERVAL_PRECISION_MISMATCH"));
    }

    #[test]
    fn review_calculation_receipt_and_error() {
        let tool = review_calculation_tool();
        let receipt = tokio()
            .block_on((tool.execute)(json!({ "operation": "add", "values": [3, 4, 5] })))
            .unwrap();
        assert_eq!(receipt.get("kind").and_then(Value::as_str), Some("review_calculation"));
        assert_eq!(receipt.get("result").and_then(Value::as_f64), Some(12.0));
        let error = tokio()
            .block_on((tool.execute)(json!({ "operation": "divide", "values": [1, 0] })))
            .unwrap_err();
        assert_eq!(error.code.as_deref(), Some("REVIEW_CALCULATION_UNDEFINED"));
        let error = tokio()
            .block_on((tool.execute)(json!({ "operation": "add", "values": [1] })))
            .unwrap_err();
        assert_eq!(error.code.as_deref(), Some("REVIEW_CALCULATION_UNDEFINED"));
    }

    #[test]
    fn plan_schema_flat_slots() {
        let schema = short_revision_plan_submission_schema(2, 3, true);
        let properties = schema.get("properties").unwrap();
        assert!(properties.get("storyTitle").is_some());
        assert!(properties.get("chapter_1_sourceNumber").is_some());
        assert!(properties.get("chapter_2_instruction").is_some());
        assert!(properties.get("chapter_3_instruction").is_none());
        let without_title = short_revision_plan_submission_schema(1, 1, false);
        assert!(without_title.get("properties").unwrap().get("storyTitle").is_none());
    }

    #[test]
    fn author_scope_validate() {
        let draft = super::super::short_fiction::ShortFictionBatchDraft {
            story_title: "题".into(),
            opening_hook: None,
            raw_content: String::new(),
            chapters: vec![
                super::super::short_fiction::ShortFictionChapter {
                    number: 1,
                    title: "一".into(),
                    content: "甲".into(),
                    char_count: 1,
                },
                super::super::short_fiction::ShortFictionChapter {
                    number: 2,
                    title: "二".into(),
                    content: "乙".into(),
                    char_count: 1,
                },
            ],
        };
        let (messages, tool, validate) = short_author_scope_request(&draft, "改第一章");
        assert_eq!(messages.len(), 3);
        assert_eq!(tool.name, "submit_short_author_scope");
        assert!(messages.iter().all(|message| !message.content.contains('\u{FFFD}')));
        validate
            .validate(&ShortAuthorScope {
                whole_manuscript: false,
                story_title: None,
                chapter_numbers: vec![1],
                chapter_title_numbers: None,
                opening: false,
                outline: false,
            })
            .unwrap();
        let error = validate
            .validate(&ShortAuthorScope {
                whole_manuscript: false,
                story_title: None,
                chapter_numbers: vec![9],
                chapter_title_numbers: None,
                opening: false,
                outline: false,
            })
            .unwrap_err();
        assert_eq!(error.code.as_deref(), Some("SHORT_REVISION_OUT_OF_SCOPE"));
    }
}
