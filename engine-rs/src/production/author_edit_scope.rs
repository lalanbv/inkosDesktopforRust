//! 作者授权编辑范围（890 号 M3c 下）：`packages/core/src/agents/author-edit-scope.ts`
//! 移植——sourceUnits 段落/行编址 + AuthorTextScope 合同 + merge/permission 面。
//!
//! 语义锚：selection.text 空 = 选整单元；inline 片段 = 行 ID 或行内部分文本
//!（singleLine）；wholeDocument 与 selections 互斥。

use serde::Serialize;
use serde_json::{json, Value};

use crate::utils::source_text::{number_source_lines, split_source_lines};


#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceUnitLine {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceUnitParagraph {
    pub id: String,
    pub lines: Vec<SourceUnitLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnitAddress {
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
}

/// TS `sourceUnits`：段落（p1/p2…）与行（p1.l1…）双编址；段落 text=行文本
/// 连接（无分隔），byId 含段落与行两级地址（1 基行号）。
pub struct SourceUnits {
    pub paragraphs: Vec<SourceUnitParagraph>,
    pub by_id: BTreeMapById,
}

pub type BTreeMapById = std::collections::BTreeMap<String, UnitAddress>;

pub fn source_units(content: &str) -> SourceUnits {
    let mut paragraphs: Vec<SourceUnitParagraph> = Vec::new();
    let mut by_id: BTreeMapById = std::collections::BTreeMap::new();
    let mut current: Option<usize> = None;
    for (index, text) in split_source_lines(content).into_iter().enumerate() {
        if text.trim().is_empty() {
            current = None;
            continue;
        }
        if current.is_none() {
            paragraphs.push(SourceUnitParagraph {
                id: format!("p{}", paragraphs.len() + 1),
                lines: Vec::new(),
            });
            current = Some(paragraphs.len() - 1);
        }
        let paragraph = &paragraphs[current.unwrap()];
        let line = SourceUnitLine {
            id: format!("{}.l{}", paragraph.id, paragraph.lines.len() + 1),
            text: text.clone(),
        };
        by_id.insert(
            line.id.clone(),
            UnitAddress {
                start_line: index + 1,
                end_line: index + 1,
                text: text.clone(),
            },
        );
        let paragraph_id = paragraph.id.clone();
        let previous_text = by_id.get(&paragraph_id).map(|unit| unit.text.clone()).unwrap_or_default();
        let previous_start = by_id.get(&paragraph_id).map(|unit| unit.start_line).unwrap_or(index + 1);
        by_id.insert(
            paragraph_id,
            UnitAddress {
                start_line: previous_start,
                end_line: index + 1,
                text: format!("{previous_text}{text}"),
            },
        );
        paragraphs[current.unwrap()].lines.push(line);
    }
    SourceUnits { paragraphs, by_id }
}

/// TS `AuthorTextScope`（TypeBox additionalProperties:false——serde 面对齐）。
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AuthorTextScope {
    #[serde(rename = "wholeDocument")]
    pub whole_document: bool,
    pub selections: Vec<AuthorTextScopeSelection>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<Vec<AuthorTextScopeFocus>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorTextScopeSelection {
    #[serde(rename = "unitId")]
    pub unit_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorTextScopeFocus {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// 编辑合同（TS `AuthorTextEditContract`：行区间合同或 scoped 选区合同 +
/// focus + originalSelectionTexts）。
#[derive(Debug)]
pub enum AuthorTextEditContract {
    Ranges(super::text_range_edits::RangeEditContract),
    Scoped(super::text_range_edits::ScopedSelectionContract),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AuthorEditPermission {
    pub basis: String,
    #[serde(rename = "selectedOriginalText")]
    pub selected_original_text: Vec<String>,
    #[serde(rename = "surroundingText")]
    pub surrounding_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<Vec<AuthorTextScopeFocus>>,
}

/// TS `authorEditPermission`：仅 offset 形态（scoped/恢复态）产出权限报告。
pub fn author_edit_permission(
    contract: Option<&AuthorTextEditContract>,
) -> Option<AuthorEditPermission> {
    let (ranges, focus) = match contract? {
        AuthorTextEditContract::Scoped(scoped) => {
            (scoped.spans.clone(), scoped_focus_clone(scoped))
        }
        // TS：`'startOffset' in scope.ranges[0]` 不成立（行区间形态）→ undefined。
        AuthorTextEditContract::Ranges(_) => return None,
    };
    let selected: Vec<String> = ranges
        .iter()
        .map(|span| {
            // originalSelectionTexts 优先；scoped 合同无该字段时=span.text。
            span.text.clone()
        })
        .collect();
    Some(AuthorEditPermission {
        basis: "original_author_request".into(),
        selected_original_text: selected,
        surrounding_text: "protected".into(),
        focus: focus.filter(|focus| !focus.is_empty()),
    })
}

fn scoped_focus_clone(scoped: &super::text_range_edits::ScopedSelectionContract) -> Option<Vec<AuthorTextScopeFocus>> {
    // scoped 合同自身不携带 focus（focus 由 AuthorTextEditContract 外层保存）。
    let _ = scoped;
    None
}

/// TS `authorTextScopeContract`：wholeDocument=整篇行区间；selections 逐个
/// 按 unitId 地址化（inline=行 ID 或行内部分文本）→ scoped 合同。
pub fn author_text_scope_contract(
    content: &str,
    result: &AuthorTextScope,
) -> Result<AuthorTextEditContract, ReviewScopeError> {
    if result.whole_document {
        if !result.selections.is_empty() {
            return Err(ReviewScopeError::plain(
                "Whole-document scope must not also select partial text",
            ));
        }
        let total_lines = split_source_lines(content).len() as u32;
        let ranges = super::text_range_edits::text_range_edit_contract(
            content,
            &[super::text_range_edits::TextEditRange {
                start_line: 1,
                end_line: total_lines,
            }],
        )
        .map_err(ReviewScopeError::plain)?;
        return Ok(AuthorTextEditContract::Ranges(ranges));
    }
    let units = source_units(content);
    let mut selections: Vec<super::text_range_edits::TextEditSelection> = Vec::new();
    for selection in &result.selections {
        let Some(unit) = units.by_id.get(&selection.unit_id) else {
            return Err(ReviewScopeError::coded(
                "ARTIFACT_EDIT_TARGET_NOT_FOUND",
                "Choose an existing paragraph or line ID from the supplied document.",
            ));
        };
        let inline_fragment = !selection.text.is_empty()
            && selection.text != unit.text
            && unit.start_line == unit.end_line;
        selections.push(super::text_range_edits::TextEditSelection {
            start_line: unit.start_line as u32,
            end_line: unit.end_line as u32,
            text: if selection.text.is_empty() { unit.text.clone() } else { selection.text.clone() },
            single_line: selection.unit_id.contains(".l") || inline_fragment,
        });
    }
    let scoped = super::text_range_edits::text_scoped_selection_edit_contract(content, &selections)
        .map_err(ReviewScopeError::plain)?;
    Ok(AuthorTextEditContract::Scoped(scoped))
}

/// TS `mergeAuthorTextScopes`：独立请求目标可重叠，并集不加源字节——区间
/// 排序合并后映射回最近似单元（singleLine 优先、其次最窄）。
pub fn merge_author_text_scopes(
    content: &str,
    scopes: &[AuthorTextScope],
) -> Result<AuthorTextScope, ReviewScopeError> {
    let mut spans: Vec<(usize, usize, bool)> = Vec::new();
    for scope in scopes {
        if scope.whole_document {
            return Err(ReviewScopeError::plain(
                "Reference targets require source selections",
            ));
        }
        if scope.selections.is_empty() {
            continue;
        }
        match author_text_scope_contract(content, scope)? {
            AuthorTextEditContract::Scoped(scoped) => {
                spans.extend(
                    scoped
                        .spans
                        .iter()
                        .map(|span| (span.start_offset, span.end_offset, span.single_line)),
                );
            }
            AuthorTextEditContract::Ranges(_) => {
                return Err(ReviewScopeError::plain(
                    "Reference targets require source selections",
                ));
            }
        }
    }
    spans.sort_by_key(|(start, _, _)| *start);
    let mut merged: Vec<(usize, usize, bool)> = Vec::new();
    for span in spans {
        match merged.last_mut() {
            Some(previous) if span.0 < previous.1 => {
                previous.1 = std::cmp::max(previous.1, span.1);
                previous.2 = previous.2 && span.2;
            }
            _ => merged.push(span),
        }
    }
    // 行偏移索引（TS offsets push 语义）。
    let lines = split_source_lines(content);
    let mut offsets: Vec<usize> = vec![0];
    for line in &lines {
        let last = *offsets.last().unwrap();
        offsets.push(last + line.len());
    }
    let units: Vec<(String, usize, usize, bool)> = source_units(content)
        .by_id
        .into_iter()
        .map(|(id, unit)| {
            let line = id.contains(".l");
            (
                id,
                offsets[unit.start_line - 1],
                offsets[unit.end_line],
                line,
            )
        })
        .collect();
    let selections: Vec<AuthorTextScopeSelection> = merged
        .iter()
        .map(|(start_offset, end_offset, single_line)| {
            let mut candidates: Vec<&(String, usize, usize, bool)> = units
                .iter()
                .filter(|(_, unit_start, unit_end, _)| {
                    *unit_start <= *start_offset && *unit_end >= *end_offset
                })
                .collect();
            candidates.sort_by(|a, b| {
                let a_match = a.3 == *single_line;
                let b_match = b.3 == *single_line;
                b_match.cmp(&a_match).then_with(|| {
                    (a.2 - a.1).cmp(&(b.2 - b.1))
                })
            });
            let unit = candidates.first().ok_or_else(|| {
                ReviewScopeError::plain("Merged span has no containing source unit")
            })?;
            Ok(AuthorTextScopeSelection {
                unit_id: unit.0.clone(),
                text: content[*start_offset..*end_offset].to_string(),
            })
        })
        .collect::<Result<Vec<_>, ReviewScopeError>>()?;
    Ok(AuthorTextScope {
        whole_document: false,
        selections,
        reason: scopes
            .iter()
            .map(|scope| scope.reason.clone())
            .collect::<Vec<_>>()
            .join("\n"),
        focus: None,
    })
}

/// TS `authorTextScopeRequest` 的消息构造面（SourceLocatorAgent.select 消费）。
pub fn author_text_scope_request_messages(
    content: &str,
    author_request: &str,
    document_identity: Option<&Value>,
) -> Vec<Value> {
    let paragraphs = source_units(content).paragraphs;
    json!({
        "authorRequest": author_request,
        "documentIdentity": document_identity,
        "document": paragraphs,
    });
    vec![
        json!({ "role": "system", "content": "Locate the smallest exact source units authorized by the author; do not rewrite them. Treat the requested identity and position as hard boundaries, even when another character or passage better fits the requested emotional effect. Never substitute a different target based on your interpretation of the plot. documentIdentity identifies the complete supplied document, even when its prose does not repeat a chapter heading. Other documents named in the request are not inside it. Select every occurrence of the qualified target in this document, including unambiguous references back to the same item; do not stop at its first mention. The numbered paragraphs contain individual lines, each with a supplied ID. Select paragraph IDs for entire paragraphs, line IDs for entire lines, and an exact substring within a selected line only for inline edits. An empty selection text means the complete selected unit. Preserve labels, stage directions and surrounding text when only dialogue is requested. The desired creative effect does not authorize selecting extra units. Use wholeDocument=true with no selections only when the author permits rewriting everything." }),
        json!({ "role": "system", "content": "Read the entire original authorRequest to identify each distinct permitted target in this document. Do not reduce a request with several corrections to the last or most salient one. When only dates, names, quantities or other words may change, copy their exact substrings and leave the rest of each sentence protected. Do not select an entire line merely because it contains an editable expression. For these reference targets, never use an empty text shortcut: a whole paragraph is selectable only if authorized as a whole, and its text must then be copied exactly. Include all qualified occurrences; title or independent-opening instructions do not select chapter prose." }),
        json!({ "role": "user", "content": serde_json::to_string(&json!({
            "authorRequest": author_request,
            "documentIdentity": document_identity,
            "document": paragraphs,
        })).unwrap_or_default() }),
    ]
}

/// 审稿源错误复用（码面与 models::observation 的 ReviewSourceError 同族但
/// 域不同——编辑范围错误独立定义）。
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ReviewScopeError {
    pub code: Option<String>,
    pub message: String,
}

impl ReviewScopeError {
    pub fn plain(message: impl Into<String>) -> Self {
        Self { code: None, message: message.into() }
    }

    pub fn coded(code: &str, message: impl Into<String>) -> Self {
        Self { code: Some(code.into()), message: message.into() }
    }
}

/// 供调试展示的编号源面（numberReviewSource 包装）。
pub fn display_numbered_source(content: &str) -> String {
    number_source_lines(content, 1)
}

// ── 测试（金标准 + 变异）─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "第一段第一行\n第一段第二行\n\n第二段整段。\n";

    #[test]
    fn source_units_addressing_golden() {
        // 段落/行双编址：p1 含两行、p2 一行；段落 text=行文本连接（无分隔）。
        let units = source_units(SOURCE);
        assert_eq!(units.paragraphs.len(), 2);
        assert_eq!(units.paragraphs[0].id, "p1");
        assert_eq!(units.paragraphs[0].lines.len(), 2);
        assert_eq!(units.paragraphs[0].lines[0].id, "p1.l1");
        assert_eq!(units.paragraphs[1].id, "p2");
        let paragraph_unit = units.by_id.get("p1").unwrap();
        assert_eq!(paragraph_unit.start_line, 1);
        assert_eq!(paragraph_unit.end_line, 2);
        assert_eq!(paragraph_unit.text, "第一段第一行\n第一段第二行\n", "段落 text=保留终止符的行连接（TS 同）");
        let line_unit = units.by_id.get("p2.l1").unwrap();
        assert_eq!(line_unit.start_line, 4);
        assert_eq!(line_unit.end_line, 4);
    }

    #[test]
    fn scope_contract_whole_document_and_line_selection() {
        let scope = AuthorTextScope {
            whole_document: true,
            selections: Vec::new(),
            reason: "整稿授权".into(),
            focus: None,
        };
        let AuthorTextEditContract::Ranges(contract) = author_text_scope_contract(SOURCE, &scope)
            .unwrap()
        else {
            panic!("wholeDocument → 行区间合同");
        };
        let output = contract
            .apply(&["第一段第一行\n第一段第二行\n\n第二段整段。\n".to_string()])
            .unwrap();
        assert_eq!(output, SOURCE);
        // 行选区：p2.l1 → scoped 合同（singleLine=true——行 ID 含 .l）。
        let scope = AuthorTextScope {
            whole_document: false,
            selections: vec![AuthorTextScopeSelection { unit_id: "p2.l1".into(), text: String::new() }],
            reason: "只改第二段".into(),
            focus: None,
        };
        let AuthorTextEditContract::Scoped(scoped) = author_text_scope_contract(SOURCE, &scope)
            .unwrap()
        else {
            panic!("行选区 → scoped 合同");
        };
        assert!(scoped.spans[0].single_line);
        // TS 语义：单元 text 含行终止符 → 替换亦吞掉行尾 \n（replacement 无
        // \n 则结果无尾换行）。
        let output = scoped.apply(&["第二段整段（改）。".to_string()]).unwrap();
        assert_eq!(output, "第一段第一行\n第一段第二行\n\n第二段整段（改）。");
    }

    #[test]
    fn scope_contract_rejects_ghost_unit_and_whole_plus_selections() {
        // 变异①：幽灵 unitId → ARTIFACT_EDIT_TARGET_NOT_FOUND。
        let scope = AuthorTextScope {
            whole_document: false,
            selections: vec![AuthorTextScopeSelection { unit_id: "p9.l9".into(), text: String::new() }],
            reason: "越界".into(),
            focus: None,
        };
        let error = author_text_scope_contract(SOURCE, &scope).unwrap_err();
        assert_eq!(error.code.as_deref(), Some("ARTIFACT_EDIT_TARGET_NOT_FOUND"));
        // 变异②：wholeDocument + selections 并存。
        let scope = AuthorTextScope {
            whole_document: true,
            selections: vec![AuthorTextScopeSelection { unit_id: "p1.l1".into(), text: String::new() }],
            reason: "矛盾授权".into(),
            focus: None,
        };
        let error = author_text_scope_contract(SOURCE, &scope).unwrap_err();
        assert_eq!(error.message, "Whole-document scope must not also select partial text");
    }

    #[test]
    fn single_line_selection_rejects_multiline_replacement() {
        // 变异③：行内片段回喂多行 → ARTIFACT_EDIT_SHAPE_INVALID。
        let scope = AuthorTextScope {
            whole_document: false,
            selections: vec![AuthorTextScopeSelection {
                unit_id: "p1.l1".into(),
                text: "第一段第一行".into(),
            }],
            reason: "行内编辑".into(),
            focus: None,
        };
        let AuthorTextEditContract::Scoped(scoped) = author_text_scope_contract(SOURCE, &scope)
            .unwrap()
        else {
            panic!("行内片段 → scoped");
        };
        assert!(scoped.spans[0].single_line);
        let error = scoped.apply(&["替换成\n两行。".to_string()]).unwrap_err();
        assert!(error.contains("ARTIFACT_EDIT_SHAPE_INVALID"), "{error}");
    }

    #[test]
    fn merge_scopes_unions_overlapping_selections() {
        // 两个 scope 选同一行不同片段（含/不含终止符）→ 并集映射回单元。
        let scope_a = AuthorTextScope {
            whole_document: false,
            selections: vec![AuthorTextScopeSelection {
                unit_id: "p1.l1".into(),
                text: "第一段第一行".into(),
            }],
            reason: "改开头".into(),
            focus: None,
        };
        let scope_b = AuthorTextScope {
            whole_document: false,
            selections: vec![AuthorTextScopeSelection {
                unit_id: "p1.l1".into(),
                text: "第一段第一行\n".into(),
            }],
            reason: "含终止符的完整行".into(),
            focus: None,
        };
        let merged = merge_author_text_scopes(SOURCE, &[scope_a, scope_b]).unwrap();
        assert!(!merged.whole_document);
        assert!(!merged.selections.is_empty());
    }
}
