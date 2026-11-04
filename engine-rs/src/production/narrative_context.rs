//! 叙事控制渲染（891 号 M3c 终）：`packages/core/src/utils/narrative-control.ts`
//! + `models/input-governance.ts` 的 ContextPackage 契约。
//!
//! reviser/writer 的 governed context 渲染面。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// TS `ContextSourceSchema`（strict）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSource {
    pub source: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    pub protection: ContextProtection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextProtection {
    Protected,
    Compressible,
}

/// TS `ContextPackageSchema`（strict）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPackage {
    pub chapter: u32,
    #[serde(rename = "selectedContext")]
    pub selected_context: Vec<ContextSource>,
}

/// TS `renderNarrativeSelectedContext`：逐条 `### Context N` + JSON(source,
/// protection) + reason/detail，空 excerpt 行剔除，块间 `\n\n`。
pub fn render_narrative_selected_context(
    entries: &[ContextSource],
    language: WritingLanguage,
) -> String {
    let en = language == crate::utils::language::WritingLanguage::En;
    let heading_label = if en { "Context" } else { "上下文" };
    let reason_label = if en { "reason" } else { "原因" };
    let detail_label = if en { "detail" } else { "细节" };
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let mut lines = vec![
                format!("### {heading_label} {}", index + 1),
                serde_json::to_string(&json_flat(entry)).unwrap_or_default(),
                format!("- {reason_label}: {}", entry.reason),
            ];
            if let Some(excerpt) = &entry.excerpt {
                lines.push(format!("- {detail_label}: {excerpt}"));
            }
            lines.into_iter().filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// TS `JSON.stringify({source, protection})`——仅两键（顺序无契约语义）。
fn json_flat(entry: &ContextSource) -> Value {
    serde_json::json!({
        "source": entry.source,
        "protection": entry.protection,
    })
}

use crate::utils::language::WritingLanguage;
