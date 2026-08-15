//! LLM 翻译模型（llm-model.ts）：提示词逐字 + JSON 容错解析（fence/子串提取）。

use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::translation::types::*;

/// `createLLMTranslationModel`：经 AgentRouter 的 translator 端口
/// （温度 0.2 / 0.1；maxTokens 可覆盖）。
pub struct LlmTranslationModel<'a> {
    pub router: &'a AgentRouter,
    pub max_tokens: Option<u32>,
}

#[async_trait::async_trait]
impl TranslationModelPort for LlmTranslationModel<'_> {
    async fn translate_segments(
        &self,
        input: TranslateSegmentsInput<'_>,
    ) -> Result<TranslateSegmentsOutput, String> {
        let system = [
            "You are InkOS Translation Agent.",
            "Translate faithfully between the requested languages.",
            "Preserve paragraph order, scene meaning, names, tone, and terminology.",
            "Do not summarize. Do not add commentary outside JSON.",
            "Return JSON only: {\"segments\":[{\"index\":1,\"target\":\"...\",\"notes\":\"optional\"}],\"glossary\":[{\"source\":\"...\",\"target\":\"...\",\"note\":\"optional\"}]}",
        ]
        .join("\n");
        let user = serde_json::to_string_pretty(&serde_json::json!({
            "sourceLanguage": input.source_language,
            "targetLanguage": input.target_language,
            "chapterTitle": input.chapter_title,
            "glossary": input.glossary,
            "segments": input.segments.iter().map(|segment| {
                serde_json::json!({ "index": segment.index, "source": segment.source })
            }).collect::<Vec<_>>(),
        }))
        .unwrap_or_default();
        let outcome = self
            .router
            .chat(
                "translator",
                vec![
                    LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
                    LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
                ],
                0.2,
                Some(self.max_tokens.unwrap_or(8192)),
            )
            .await
            .map_err(|e| e.to_string())?;
        let parsed = parse_json_object(&outcome.content)?;
        Ok(TranslateSegmentsOutput {
            segments: parse_translated_segments(parsed.get("segments"), input.segments)?,
            glossary: parse_glossary(parsed.get("glossary")),
        })
    }

    async fn review_chapter(
        &self,
        input: ReviewChapterInput<'_>,
    ) -> Result<ReviewChapterOutput, String> {
        let system = [
            "You are InkOS Translation Review Agent.",
            "Check fidelity, omissions, terminology, pronouns, names, and target-language readability.",
            "Return JSON only: {\"passed\":true,\"summary\":\"...\",\"issues\":[\"...\"]}.",
        ]
        .join("\n");
        let user = serde_json::to_string_pretty(&serde_json::json!({
            "sourceLanguage": input.source_language,
            "targetLanguage": input.target_language,
            "chapterTitle": input.chapter_title,
            "glossary": input.glossary,
            "segments": input.segments.iter().map(|segment| {
                serde_json::json!({ "index": segment.index, "source": segment.source, "target": segment.target.clone().unwrap_or_default() })
            }).collect::<Vec<_>>(),
        }))
        .unwrap_or_default();
        let outcome = self
            .router
            .chat(
                "translator",
                vec![
                    LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
                    LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
                ],
                0.1,
                Some(4096),
            )
            .await
            .map_err(|e| e.to_string())?;
        let parsed = parse_json_object(&outcome.content)?;
        Ok(ReviewChapterOutput {
            passed: parsed.get("passed") == Some(&serde_json::json!(true)),
            summary: parsed
                .get("summary")
                .and_then(|s| s.as_str())
                .map(String::from)
                .unwrap_or_else(|| "Translation review completed.".to_string()),
            issues: parsed
                .get("issues")
                .and_then(|i| i.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|issue| issue.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
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

/// `parseJsonObject`：整串 → fence 剥离 → 首个 { 到最后 } 子串。
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

/// `parseTranslatedSegments`：index 必须命中源段；target 非空。
fn parse_translated_segments(
    value: Option<&serde_json::Value>,
    source_segments: &[TranslationSegment],
) -> Result<Vec<TranslatedSegmentItem>, String> {
    let Some(items) = value.and_then(|v| v.as_array()) else {
        return Err("Translation model did not return a segments array.".to_string());
    };
    let mut out = Vec::new();
    for item in items {
        let Some(record) = item.as_object() else { continue };
        let index = record.get("index").and_then(|v| v.as_u64());
        let target = record
            .get("target")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty());
        let (Some(index), Some(target)) = (index, target) else {
            continue;
        };
        let index = index as u32;
        if !source_segments.iter().any(|segment| segment.index == index) {
            continue;
        }
        out.push(TranslatedSegmentItem {
            index,
            target: target.to_string(),
            notes: record
                .get("notes")
                .and_then(|n| n.as_str())
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(String::from),
        });
    }
    if out.is_empty() {
        return Err("Translation model returned no usable translated segments.".to_string());
    }
    Ok(out)
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
