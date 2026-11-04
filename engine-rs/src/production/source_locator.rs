//! 源定位代理（890 号 M3c 下）：`packages/core/src/agents/source-locator.ts`
//! `SourceLocatorAgent.select`——位置解析先于正文阅读（防越权扩编）。
//!
//! 环语义：submitStructured 等价（888 基座）+ LocationSchema 校验
//!（EDIT_LOCATION_INVALID / EDIT_TARGET_AUTHORITY_MISSING 两码）。

use std::sync::Arc;

use serde_json::{json, Value};

use super::author_edit_scope::{AuthorTextScope, AuthorTextScopeSelection};
use super::structured_agent::{
    run_structured_tool, StructuredAgentError, StructuredDefaults, StructuredRunOptions,
    WorkerToolSpec,
};
use crate::llm::provider::LLMMessage;
use crate::utils::language::WritingLanguage;
use crate::production::author_edit_scope::source_units;

#[derive(Debug, Clone, PartialEq)]
pub struct SourceIdentity<'a> {
    pub kind: &'a str, // "chapter" | "artifact"
    pub chapter_number: Option<u32>,
    pub title: Option<&'a str>,
    pub path: Option<&'a str>,
}

#[derive(Clone, Copy)]
pub struct LocatorAgents<'a> {
    pub chat: &'a super::structured_agent::ChatTurnFn,
    pub model: &'a str,
    pub defaults: &'a StructuredDefaults,
}

/// TS `SourceLocatorAgent.select`：LocationSchema 提交（含两段 system 逐字）
/// → validate（kind/boundary/count/targets 一致面 + target.quote 必须在
/// authorRequest 原文）→ AuthorTextScope（paragraphs 有界计数/wholeDocument
/// reference 展开）。
#[allow(clippy::too_many_lines)]
pub async fn select(
    agents: &LocatorAgents<'_>,
    content: &str,
    author_request: &str,
    identity: Option<&SourceIdentity<'_>>,
    _language: WritingLanguage,
) -> Result<AuthorTextScope, StructuredAgentError> {
    // 过滤纯标题/分隔线段（TS 同款正则面）。
    let paragraphs: Vec<_> = source_units(content)
        .paragraphs
        .into_iter()
        .filter(|paragraph| {
            !paragraph.lines.iter().all(|line| {
                let text = line.text.trim_start();
                let heading = text.starts_with('#')
                    && text
                        .trim_start_matches('#')
                        .starts_with(' ');
                let rule = {
                    let trimmed_end = text.trim_end();
                    let stars = trimmed_end.chars().filter(|ch| *ch == '*').count();
                    let dashes = trimmed_end.chars().filter(|ch| *ch == '-').count();
                    let underscores = trimmed_end.chars().filter(|ch| *ch == '_').count();
                    (stars >= 3 && trimmed_end.chars().all(|ch| ch == '*' || ch == ' '))
                        || (dashes >= 3 && trimmed_end.chars().all(|ch| ch == '-' || ch == ' '))
                        || (underscores >= 3
                            && trimmed_end.chars().all(|ch| ch == '_' || ch == ' '))
                };
                heading || rule
            })
        })
        .collect();
    let paragraph_count = paragraphs.len();
    let identity_json = identity.map(|identity| {
        json!({
            "kind": identity.kind,
            "chapterNumber": identity.chapter_number,
            "title": identity.title,
            "path": identity.path,
        })
    });
    let messages = vec![
        llm_system("Identify only the location the author permits editing. Do not plan replacement prose. The supplied identity describes the complete document available for this operation; other documents named in the request are not inside it. First check that the author permits changing this document. Use none when it is protected or the requested edit belongs exclusively to another document. Use paragraphs only for explicit first/last paragraph counts within this document, and document only for unrestricted rewriting of this document. Otherwise use reference. For none/document/reference set boundary=none and count=0; none/document/paragraphs have targets=[]."),
        llm_system("Reference scope must enumerate each independent requested target, with an exact quote from the author request. Split a request for several corrections into separate targets instead of one summary. Include a fact requested across chapters unless the author explicitly limits it to another document; the next step will determine whether it is present here. Editing selected facts throughout a chapter is reference scope, not document scope. A chapter number is not a paragraph count; an independent opening and a chapter title are separate from chapter prose. Preserve all restrictions on identity, location and surrounding text."),
        llm_user(
            &serde_json::to_string(&json!({
                "authorRequest": author_request,
                "documentIdentity": identity_json,
                "paragraphCount": paragraph_count,
            }))
            .unwrap_or_default(),
        ),
    ];
    let author_request_owned = author_request.to_string();
    let paragraph_count_for_validate = paragraph_count;
    let validate: super::structured_agent::ValidateFn = Arc::new(move |value: Value| {
        let author_request = author_request_owned.clone();
        let paragraph_count = paragraph_count_for_validate;
        Box::pin(async move {
            let kind = value.get("kind").and_then(Value::as_str).unwrap_or_default();
            let boundary = value.get("boundary").and_then(Value::as_str).unwrap_or_default();
            let count = value.get("count").and_then(Value::as_u64).unwrap_or(0);
            let targets = value.get("targets").and_then(Value::as_array).cloned().unwrap_or_default();
            let valid = if kind == "paragraphs" {
                boundary != "none"
                    && count >= 1
                    && count as usize <= paragraph_count
                    && targets.is_empty()
            } else {
                boundary == "none"
                    && count == 0
                    && if kind == "reference" {
                        !targets.is_empty()
                    } else {
                        targets.is_empty()
                    }
            };
            for target in &targets {
                let quote = target.get("quote").and_then(Value::as_str).unwrap_or_default();
                if !author_request.contains(quote) {
                    return Err(super::structured_agent::StructuredToolError::coded(
                        "EDIT_TARGET_AUTHORITY_MISSING",
                        "Each target must quote the original author request exactly.",
                    ));
                }
            }
            if !valid {
                return Err(super::structured_agent::StructuredToolError::coded(
                    "EDIT_LOCATION_INVALID",
                    "Use one valid location: a bounded first/last paragraph count, the complete document, or a qualified source reference.",
                ));
            }
            Ok(value)
        })
    });
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "kind": { "enum": ["none", "document", "paragraphs", "reference"] },
            "boundary": { "enum": ["first", "last", "none"] },
            "count": { "type": "integer", "minimum": 0 },
            "targets": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "quote": { "type": "string", "minLength": 1, "description": "Exact author wording authorizing this target." },
                        "target": { "type": "string", "minLength": 1, "description": "One independent requested target, with identity and scope qualifiers, not replacement prose." },
                    },
                    "required": ["quote", "target"],
                    "additionalProperties": false,
                },
                "description": "For reference scope, list each distinct target separately. For document/paragraphs use an empty array.",
            },
        },
        "required": ["kind", "boundary", "count", "targets"],
        "additionalProperties": false,
    });
    let outcome = run_structured_tool(
        Arc::clone(agents.chat),
        agents.model,
        &messages,
        agents.defaults.clone(),
        WorkerToolSpec {
            name: "submit_edit_location".into(),
            description: "Resolve the author location without reading or rewriting the prose.".into(),
            parameters_schema: schema,
            max_turns: None,
        },
        Some(validate),
        Vec::new(),
        StructuredRunOptions { temperature: None, max_tokens: None },
    )
    .await?;
    let result = &outcome.result;
    let kind = result.get("kind").and_then(Value::as_str).unwrap_or_default();
    let boundary = result.get("boundary").and_then(Value::as_str).unwrap_or_default();
    let count = result.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
    let scope = match kind {
            "none" => AuthorTextScope {
                whole_document: false,
                selections: Vec::new(),
                reason: format!("location=none boundary={boundary}"),
                focus: None,
            },
            "document" => AuthorTextScope {
                whole_document: true,
                selections: Vec::new(),
                reason: format!("location=document boundary={boundary}"),
                focus: None,
            },
            "paragraphs" => {
                // TS：first/last 有界计数 → 逐段展开选择（p1..pN 或 p(N-count+1)..pN）。
                let selections: Vec<AuthorTextScopeSelection> = paragraphs
                    .iter()
                    .take(if boundary == "first" { count } else { 0 })
                    .map(|paragraph| AuthorTextScopeSelection {
                        unit_id: paragraph.id.clone(),
                        text: String::new(),
                    })
                    .chain(
                        paragraphs
                            .iter()
                            .rev()
                            .take(if boundary == "last" { count } else { 0 })
                            .map(|paragraph| AuthorTextScopeSelection {
                                unit_id: paragraph.id.clone(),
                                text: String::new(),
                            }),
                    )
                    .collect();
                AuthorTextScope {
                    whole_document: false,
                    selections,
                    reason: format!("location=paragraphs boundary={boundary} count={count}"),
                    focus: None,
                }
            }
            // reference：targets 逐个成为 selections 占位（unitId 空——由
            // author-permission 的 scope 面按文本定位；TS 同为占位结构）。
            _ => {
                let selections: Vec<AuthorTextScopeSelection> = result
                    .get("targets")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .map(|target| AuthorTextScopeSelection {
                                unit_id: String::new(),
                                text: target
                                    .get("target")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                AuthorTextScope {
                    whole_document: false,
                    selections,
                    reason: "location=reference".into(),
                    focus: None,
                }
            }
        }
    ;
    Ok(scope)
}

fn llm_system(content: &str) -> LLMMessage {
    LLMMessage { role: crate::llm::provider::LLMRole::System, content: content.into(), tool_calls: None, tool_call_id: None }
}

fn llm_user(content: &str) -> LLMMessage {
    LLMMessage { role: crate::llm::provider::LLMRole::User, content: content.into(), tool_calls: None, tool_call_id: None }
}
