//! 行区间/选区编辑合同（890 号 M3c 下）：`packages/core/src/utils/text-range-edits.ts`
//! 忠实移植。不可变源绑定 + 替换字段 apply——host 侧字节守卫面。
//!
//! 语义锚：selection 的 `singleLine` 行内片段回喂禁换行（ARTIFACT_EDIT_SHAPE_INVALID）；
//! overlap/越界 → ARTIFACT_EDIT_RANGE_INVALID。

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::utils::source_text::split_source_lines;

#[derive(Debug, Clone, PartialEq)]
pub struct TextEditRange {
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextEditSelection {
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub single_line: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoundSelection {
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub single_line: bool,
    /// 选区源文本（替换校验/凭证面）。
    pub content: String,
    pub protected_prefix: String,
    pub protected_suffix: String,
    pub start_offset: usize,
    pub end_offset: usize,
}

/// TS `textRangeEditContract`：行区间 → 不可变源绑定（`range_{index}_content`
/// 替换字段）。非法区间（重叠/越界/逆序）→ ARTIFACT_EDIT_RANGE_INVALID。
#[derive(Debug, Clone)]
pub struct RangeEditContract {
    pub ranges: Vec<BoundRange>,
    /// 字段名序（`range_{index}_content`）——LLM 提交 schema 构造用。
    pub field_names: Vec<String>,
    pub content: String,
    pub sorted: Vec<TextEditRange>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoundRange {
    pub index: usize,
    pub start_line: u32,
    pub end_line: u32,
    /// 选区源文本（`lines[start-1..end].join('')`）。
    pub content: String,
}

impl RangeEditContract {
    /// TS `apply(replacements)`：保留区间外全部字节。
    pub fn apply(&self, replacements: &[String]) -> Result<String, String> {
        let lines = split_source_lines(&self.content);
        let mut output: Vec<String> = Vec::new();
        let mut cursor = 0usize;
        for (index, range) in self.sorted.iter().enumerate() {
            let replacement = replacements
                .get(index)
                .ok_or_else(|| format!("missing replacement range_{index}_content"))?;
            let _ = replacement;
            output.push(lines[cursor..range.start_line as usize - 1].join(""));
            output.push(replacements[index].clone());
            cursor = range.end_line as usize;
        }
        output.push(lines[cursor..].join(""));
        Ok(output.join(""))
    }

    /// LLM 提交 schema 的 parameters 面（TypeBox 对齐：逐字段 description）。
    pub fn parameters_schema(&self) -> Value {
        let mut properties = Map::new();
        for range in &self.ranges {
            properties.insert(
                format!("range_{}_content", range.index),
                json!({
                    "type": "string",
                    "description": format!(
                        "Replacement for source lines {}-{}, including the original trailing newline when present.",
                        range.start_line, range.end_line
                    ),
                }),
            );
        }
        json!({
            "type": "object",
            "properties": properties,
            "required": self.ranges.iter().map(|range| json!(format!("range_{}_content", range.index))).collect::<Vec<_>>(),
            "additionalProperties": false,
        })
    }
}

/// TS `textRangeEditContract`。
pub fn text_range_edit_contract(
    content: &str,
    ranges: &[TextEditRange],
) -> Result<RangeEditContract, String> {
    let lines = split_source_lines(content);
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|range| range.start_line);
    let valid = !sorted.is_empty()
        && sorted.iter().enumerate().all(|(index, range)| {
            range.start_line >= 1
                && range.end_line >= range.start_line
                && (range.end_line as usize) <= lines.len()
                && (index == 0 || range.start_line > sorted[index - 1].end_line)
        });
    if !valid {
        return Err(json!({
            "code": "ARTIFACT_EDIT_RANGE_INVALID",
            "recovery": {
                "action": "workspace__read",
                "reason": format!(
                    "Use the numbered source and exact revision from read; do not extend a range past totalLines. Editable ranges must be non-overlapping source lines between 1 and {}",
                    lines.len()
                ),
            },
        })
        .to_string());
    }
    let bound_ranges: Vec<BoundRange> = sorted
        .iter()
        .enumerate()
        .map(|(index, range)| BoundRange {
            index,
            start_line: range.start_line,
            end_line: range.end_line,
            content: lines[range.start_line as usize - 1..range.end_line as usize].join(""),
        })
        .collect();
    let field_names = bound_ranges
        .iter()
        .map(|range| format!("range_{}_content", range.index))
        .collect();
    Ok(RangeEditContract {
        ranges: bound_ranges,
        field_names,
        content: content.to_string(),
        sorted,
    })
}

/// TS `textSelectionEditContract`：唯一目标文本绑定（重复出现 →
/// ARTIFACT_EDIT_TARGET_AMBIGUOUS；缺失 → ARTIFACT_EDIT_TARGET_NOT_FOUND）。
pub struct SelectionEditContract {
    pub content: String,
    pub target_text: String,
    pub start_offset: usize,
}

impl SelectionEditContract {
    pub fn apply(&self, replacement: &str) -> String {
        format!(
            "{}{replacement}{}",
            &self.content[..self.start_offset],
            &self.content[self.start_offset + self.target_text.len()..]
        )
    }

    pub fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "range_0_content": {
                    "type": "string",
                    "description": "Replacement for the exact selected source text. Preserve all text outside this selection.",
                },
            },
            "required": ["range_0_content"],
            "additionalProperties": false,
        })
    }
}

/// TS `textSelectionEditContract`。
pub fn text_selection_edit_contract(
    content: &str,
    target_text: &str,
) -> Result<SelectionEditContract, String> {
    if target_text.is_empty() {
        return Err(json!({ "code": "ARTIFACT_EDIT_TARGET_NOT_FOUND" }).to_string());
    }
    let Some(start) = content.find(target_text) else {
        return Err(json!({ "code": "ARTIFACT_EDIT_TARGET_NOT_FOUND" }).to_string());
    };
    if content[start + target_text.len()..].contains(target_text) {
        return Err(json!({ "code": "ARTIFACT_EDIT_TARGET_AMBIGUOUS" }).to_string());
    }
    Ok(SelectionEditContract {
        content: content.to_string(),
        target_text: target_text.to_string(),
        start_offset: start,
    })
}

/// scoped 选区合同（TS `textScopedSelectionEditContract` → selectionContract）：
/// 选区转 offset 区间（区域内 text 二次定位）→ 排序 → 非重叠校验 → apply。
#[derive(Debug, Clone)]
pub struct ScopedSelectionContract {
    pub content: String,
    pub spans: Vec<BoundSelection>,
}

impl ScopedSelectionContract {
    /// TS `apply`：singleLine 片段禁换行（ARTIFACT_EDIT_SHAPE_INVALID）。
    pub fn apply(&self, replacements: &[String]) -> Result<String, String> {
        let mut output: Vec<String> = Vec::new();
        let mut cursor = 0usize;
        for (index, span) in self.spans.iter().enumerate() {
            let replacement = replacements
                .get(index)
                .ok_or_else(|| format!("missing replacement selection_{index}_text"))?;
            let mut body = replacement.clone();
            if span.content.ends_with('\n') {
                if let Some(stripped) =
                    body.strip_suffix('\r').or_else(|| body.strip_suffix('\n'))
                {
                    let _ = stripped;
                    body = replacement.trim_end_matches('\r').trim_end_matches('\n').to_string();
                } else {
                    body = replacement
                        .trim_end_matches("\r\n")
                        .trim_end_matches('\r')
                        .trim_end_matches('\n')
                        .to_string();
                }
            }
            if span.single_line && body.contains(['\r', '\n']) {
                return Err(json!({
                    "code": "ARTIFACT_EDIT_SHAPE_INVALID",
                    "selectionIndex": index,
                })
                .to_string());
            }
            output.push(self.content[cursor..span.start_offset].to_string());
            output.push(replacement.clone());
            cursor = span.end_offset;
        }
        output.push(self.content[cursor..].to_string());
        Ok(output.join(""))
    }

    pub fn parameters_schema(&self) -> Value {
        let mut properties = Map::new();
        for (index, span) in self.spans.iter().enumerate() {
            properties.insert(
                format!("selection_{index}_text"),
                json!({
                    "type": "string",
                    "description": "Replacement characters for content only. The protectedPrefix and protectedSuffix are already retained; do not reproduce or rewrite them.",
                }),
            );
            let _ = span;
        }
        json!({
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
        })
    }
}

/// TS `textScopedSelectionEditContract`。
pub fn text_scoped_selection_edit_contract(
    content: &str,
    selections: &[TextEditSelection],
) -> Result<ScopedSelectionContract, String> {
    let lines = split_source_lines(content);
    let mut spans: Vec<BoundSelection> = Vec::new();
    for selection in selections {
        // 区间合法性（textRangeEditContract 单选区面）。
        if selection.start_line == 0
            || selection.end_line < selection.start_line
            || (selection.end_line as usize) > lines.len()
        {
            return Err(json!({ "code": "ARTIFACT_EDIT_RANGE_INVALID" }).to_string());
        }
        let region = lines[selection.start_line as usize - 1..selection.end_line as usize].join("");
        let Some(local_start) = region.find(&selection.text) else {
            return Err(json!({ "code": "ARTIFACT_EDIT_TARGET_NOT_FOUND" }).to_string());
        };
        let local_end = local_start + selection.text.len();
        let offset: usize = lines[..selection.start_line as usize - 1].join("").len();
        spans.push(BoundSelection {
            start_line: selection.start_line,
            end_line: selection.end_line,
            text: selection.text.clone(),
            single_line: selection.single_line,
            content: selection.text.clone(),
            protected_prefix: region[..local_start].to_string(),
            protected_suffix: region[local_end..].to_string(),
            start_offset: offset + local_start,
            end_offset: offset + local_end,
        });
    }
    spans.sort_by_key(|span| span.start_offset);
    // 非重叠 + 字节界校验（selectionContract 头部守卫）。
    let valid = spans.iter().enumerate().all(|(index, span)| {
        span.start_offset <= span.end_offset
            && span.end_offset <= content.len()
            && (index == 0 || span.start_offset >= spans[index - 1].end_offset)
    });
    if !valid {
        return Err(json!({ "code": "ARTIFACT_EDIT_RANGE_INVALID" }).to_string());
    }
    Ok(ScopedSelectionContract { content: content.to_string(), spans })
}

/// TS `textOffsetSelectionEditContract`：既有 offset 区间（恢复态/窄化面）
/// 直接转 scoped 合同——无需文本搜索。
pub fn text_offset_selection_edit_contract(
    content: &str,
    ranges: &[(usize, usize, bool)],
) -> Result<ScopedSelectionContract, String> {
    let lines = split_source_lines(content);
    let mut offsets: Vec<usize> = vec![0];
    for line in &lines {
        let last = *offsets.last().unwrap();
        offsets.push(last + line.len());
    }
    let line_at = |offset: usize| -> usize {
        let mut index = 0usize;
        while index + 1 < lines.len() && offsets[index + 1] <= offset {
            index += 1;
        }
        index + 1
    };
    let mut spans: Vec<BoundSelection> = Vec::new();
    for (start_offset, end_offset, single_line) in ranges {
        let start_line = line_at(*start_offset);
        let end_line = line_at(std::cmp::max(*start_offset, end_offset.saturating_sub(1)));
        let text = content[*start_offset..*end_offset].to_string();
        spans.push(BoundSelection {
            start_line: start_line as u32,
            end_line: end_line as u32,
            text: text.clone(),
            single_line: *single_line,
            content: text.clone(),
            protected_prefix: content[offsets[start_line - 1]..*start_offset].to_string(),
            protected_suffix: content[*end_offset..offsets[end_line]].to_string(),
            start_offset: *start_offset,
            end_offset: *end_offset,
        });
    }
    Ok(ScopedSelectionContract { content: content.to_string(), spans })
}
