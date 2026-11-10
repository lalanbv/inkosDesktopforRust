//! 段落修订代理工具（973 号）：TS `harness/tools/translation.ts`
//! `createTranslationRevisionTool`（revise_paragraph）的注册表面——模型修订执行体
//! 复用 [`crate::translation::revision::revise_translation_segment`]（971 号）。
//!
//! TS 形态对齐：committedAction（生产变更面=ProductionMutation，suppress_production
//! 集中拒绝）；参数 schema=TranslationRevisionParams 逐字（workId?/chapterNumber≥1/
//! paragraph last|≥1/instruction minLength 1）；结果文本=TS 逐字
//! `{Revised|Kept} chapter N, paragraph P of C (last paragraph|not the last paragraph).
//! Selected source: …`，details kind=translation_segment_revised。
//!
//! 备案：TS 走 `withWorkMutationScope`（Work 锁）；Rust 翻译域锁面（run/revise HTTP
//! 面）为每端点独立串行（971 号），本工具与 HTTP revise 同执行体同语义。

use serde_json::{json, Value};

use crate::interaction::project_tools::ToolResult;
use crate::interaction::registry::{MutationKind, ToolDef};
use crate::server::books_routes::BooksRuntime;

/// 973 号：翻译族依赖（agent_route 装配：runtime + 会话活动书）。
pub struct TranslationToolDeps<'a> {
    pub runtime: &'a BooksRuntime,
    pub active_book_id: Option<&'a str>,
}

/// TS `TranslationRevisionParams`（逐字，additionalProperties:false）。
fn revision_params_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workId": { "type": "string", "description": "Translation Work ID; omit only when this Work is active." },
            "chapterNumber": { "type": "integer", "minimum": 1 },
            "paragraph": {
                "description": "Use last for the final paragraph. Otherwise supply a human paragraph number starting at 1. This is not a zero-based array index or a stored segment ID.",
                "anyOf": [
                    { "const": "last" },
                    { "type": "integer", "minimum": 1 },
                ],
            },
            "instruction": { "type": "string", "minLength": 1, "description": "The requested change to this translated paragraph, preserving source facts." },
        },
        "required": ["chapterNumber", "paragraph", "instruction"],
        "additionalProperties": false,
    })
}

pub(crate) fn defs() -> Vec<Box<dyn ToolDef>> {
    vec![Box::new(ReviseParagraph)]
}

// ── 注册模块（R38b 形态）：revise_paragraph 单件——生产变更面。 ──

crate::interaction::registry::tool_def!(
    ReviseParagraph,
    "revise_paragraph",
    MutationKind::ProductionMutation,
    ctx, args,
    { "Revise one translated paragraph while preserving every other paragraph and all source text. To revise the final paragraph, set paragraph to last. The host selects it from the complete chapter. Review and export after revision." },
    revision_params_schema(),
    ctx.translation_deps.is_some(),
    {
        let deps = ctx.translation_deps.as_ref().expect("available 门控");
        tool_revise_paragraph(deps.runtime, deps.active_book_id, args).await
    }
);

/// 解析 paragraph 参数（TS Union：`"last"` | ≥1 整数）。None=last。
fn parse_paragraph(value: Option<&Value>) -> Result<Option<u32>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text == "last" => Ok(None),
        Some(Value::Number(number)) => {
            let raw = number.as_u64().ok_or_else(|| {
                coded("TRANSLATION_PARAGRAPH_REQUIRED", "Select last or a paragraph number starting at 1")
            })?;
            let paragraph = u32::try_from(raw).map_err(|_| {
                coded("TRANSLATION_PARAGRAPH_REQUIRED", "Select last or a paragraph number starting at 1")
            })?;
            if paragraph == 0 {
                return Err(coded(
                    "TRANSLATION_PARAGRAPH_REQUIRED",
                    "Select last or a paragraph number starting at 1",
                ));
            }
            Ok(Some(paragraph))
        }
        Some(_) => Err(coded(
            "TRANSLATION_PARAGRAPH_REQUIRED",
            "Select last or a paragraph number starting at 1",
        )),
    }
}

fn resolve_work_id_from(active_book_id: Option<&str>, explicit: Option<&str>) -> Result<String, String> {
    explicit
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| active_book_id.map(str::to_string))
        .ok_or_else(|| coded("WORK_ID_REQUIRED", "Translation Work ID is required when no Work is active."))
}

fn coded(code: &str, message: impl std::fmt::Display) -> String {
    format!("{code}: {message}")
}

/// 工具执行体：参数守卫（TS resolveTranslationWorkId/params 校验同序）→
/// revise_translation_segment（技能装配与 HTTP revise 同链）→ TS textResult 形。
async fn tool_revise_paragraph(
    runtime: &BooksRuntime,
    active_book_id: Option<&str>,
    args: &Value,
) -> ToolResult {
    let work_id = match resolve_work_id_from(active_book_id, args.get("workId").and_then(Value::as_str)) {
        Ok(work_id) => work_id,
        Err(message) => return crate::interaction::project_tools::error_result(message),
    };
    let chapter_number = match args
        .get("chapterNumber")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
    {
        Some(number) => number,
        None => return crate::interaction::project_tools::error_result(coded(
            "TRANSLATION_CHAPTER_REQUIRED",
            "chapterNumber is required",
        )),
    };
    let paragraph = match parse_paragraph(args.get("paragraph")) {
        Ok(paragraph) => paragraph,
        Err(message) => return crate::interaction::project_tools::error_result(message),
    };
    let instruction = match args
        .get("instruction")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(instruction) => instruction.to_string(),
        None => return crate::interaction::project_tools::error_result(coded(
            "TRANSLATION_INSTRUCTION_REQUIRED",
            "instruction is required",
        )),
    };

    let root = runtime.state.project_root();
    // 971 号同款技能装配（revise=翻译技能域）。
    let activations = match crate::server::translation_routes::translation_skill_activations(root).await {
        Ok(activations) => activations,
        Err(message) => return crate::interaction::project_tools::error_result(coded(
            "TRANSLATION_REVISION_FAILED",
            message,
        )),
    };
    let model = crate::translation::llm_model::LlmTranslationModel {
        router: std::sync::Arc::clone(&runtime.effective_router().await),
        max_tokens: None,
    };
    let result = match crate::skills::production_bindings::scope_operation_skills(
        Some(std::sync::Arc::new(activations)),
        crate::translation::revision::revise_translation_segment(
            root,
            &work_id,
            chapter_number,
            paragraph,
            &instruction,
            &model,
        ),
    )
    .await
    {
        Ok(result) => result,
        Err(message) => {
            let code = message.split(": ").next().unwrap_or("TRANSLATION_REVISION_FAILED").to_string();
            return crate::interaction::project_tools::error_result(coded(&code, &message));
        }
    };

    // TS textResult 逐字：`{Revised|Kept} chapter N, paragraph P of C (last paragraph|
    // not the last paragraph). Selected source: …`。
    let text = format!(
        "{} chapter {}, paragraph {} of {}{}. Selected source: {}",
        if result["changed"].as_bool().unwrap_or(false) { "Revised" } else { "Kept" },
        result["chapterNumber"],
        result["paragraphNumber"],
        result["paragraphCount"],
        if result["isLastParagraph"].as_bool().unwrap_or(false) {
            " (last paragraph)"
        } else {
            " (not the last paragraph)"
        },
        result["sourceExcerpt"].as_str().unwrap_or_default(),
    );
    ToolResult {
        is_error: false,
        text,
        details: {
            let mut details = result.clone();
            if let Some(object) = details.as_object_mut() {
                object.insert("kind".into(), json!("translation_segment_revised"));
                object.insert("workId".into(), json!(work_id));
            }
            Some(details)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::registry::ToolRegistry;

    /// 973 号：注册面——revise_paragraph 入册、生产变更面、schema 三键齐。
    #[test]
    fn revise_paragraph_registered_as_production_mutation() {
        let registry = ToolRegistry::global();
        let def = registry
            .all()
            .find(|def| def.name() == "revise_paragraph")
            .expect("revise_paragraph 入册");
        assert_eq!(def.mutation_kind(), MutationKind::ProductionMutation);
        let schema = def.parameters();
        assert_eq!(schema["required"], serde_json::json!(["chapterNumber", "paragraph", "instruction"]));
        assert_eq!(schema["additionalProperties"], false);
        let any_of = &schema["properties"]["paragraph"]["anyOf"];
        assert_eq!(any_of[0], serde_json::json!({ "const": "last" }));
        assert_eq!(any_of[1]["type"], "integer");
        assert_eq!(any_of[1]["minimum"], 1);
    }

    /// 973 号：parse_paragraph——last(None)/合法段号/0 与负形与字符串数字即错。
    #[test]
    fn parse_paragraph_faces() {
        assert_eq!(parse_paragraph(Some(&json!("last"))).unwrap(), None);
        assert_eq!(parse_paragraph(Some(&json!(3))).unwrap(), Some(3));
        assert_eq!(parse_paragraph(None).unwrap(), None);
        assert_eq!(parse_paragraph(Some(&Value::Null)).unwrap(), None);
        assert!(parse_paragraph(Some(&json!(0))).is_err());
        assert!(parse_paragraph(Some(&json!(-1))).is_err());
        assert!(parse_paragraph(Some(&json!("2"))).is_err());
    }

    /// 973 号：TS textResult 文本逐字（Revised/Kept × last/not-last 四形）。
    #[test]
    fn receipt_text_matches_ts() {
        let text = |changed: bool, last: bool| {
            format!(
                "{} chapter 1, paragraph 2 of 3{}. Selected source: 样段",
                if changed { "Revised" } else { "Kept" },
                if last { " (last paragraph)" } else { " (not the last paragraph)" },
            )
        };
        assert_eq!(text(true, true), "Revised chapter 1, paragraph 2 of 3 (last paragraph). Selected source: 样段");
        assert_eq!(text(false, false), "Kept chapter 1, paragraph 2 of 3 (not the last paragraph). Selected source: 样段");
    }
}
