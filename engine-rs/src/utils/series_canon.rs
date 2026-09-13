//! R20 系列正典共享（389 号契约批，四轮 P0）。
//!
//! TS 真源：`packages/core/src/utils/series-canon.ts`；golden 唯一事实源：
//! `packages/core/src/__tests__/golden/series-canon-vectors.json`（差分测试
//! `tests/golden_series_canon_diff.rs` 读同一文件）。
//!
//! 跨书正典层（Sudowrite Series Folders 对标）：`.inkos/series/{seriesId}.json`
//! 同系列多书消费；book 同名条目覆盖 series 条目（覆盖即省略）。
//! 截断/计数与 TS 逐字一致（码元 = chars；禁 locale 感知排序）。

use crate::utils::entity_codex::{CodexMatch, EntityCodexCard};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SERIES_CANON_VERSION: i64 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesCanonFile {
    pub version: i64,
    pub series_id: String,
    pub entries: Vec<EntityCodexCard>,
}

/// seriesId 规范：snake_case slug（禁空格/连字符/路径段，≤64 码元）。
pub fn is_valid_series_id(series_id: &str) -> bool {
    let len = series_id.chars().count();
    (1..=64).contains(&len)
        && series_id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

const SERIES_LIMITS_NAME: usize = 80;
const SERIES_LIMITS_ALIASES: usize = 10;
const SERIES_LIMITS_ALIAS: usize = 60;
const CODEX_LIMITS_SUMMARY: usize = 300;
const CODEX_LIMITS_FACTS: usize = 10;
const CODEX_LIMITS_FACT: usize = 200;
const CODEX_LIMITS_RELATIONSHIPS: usize = 10;
const CODEX_LIMITS_RELATIONSHIP_NOTE: usize = 200;

fn clamp_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() > max_chars {
        value.chars().take(max_chars).collect()
    } else {
        value.to_string()
    }
}

fn clamp_string_list(raw: Option<&Vec<Value>>, max_items: usize, max_chars: usize) -> Vec<String> {
    raw.map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str())
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .take(max_items)
            .map(|item| clamp_text(&item, max_chars))
            .collect()
    })
    .unwrap_or_default()
}

fn is_valid_kind(kind: &str) -> bool {
    matches!(kind, "person" | "place" | "faction" | "item" | "other")
}

/// 条目校验（非法跳过/字段截断）；kind 非法兜底 other（对齐名册解析）。
fn sanitize_entry(raw: &Value) -> Option<EntityCodexCard> {
    let record = raw.as_object()?;
    let name = record
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let name = clamp_text(&name, SERIES_LIMITS_NAME);
    let aliases = clamp_string_list(
        record.get("aliases").and_then(Value::as_array),
        SERIES_LIMITS_ALIASES,
        SERIES_LIMITS_ALIAS,
    );
    let kind_raw = record
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let kind = if is_valid_kind(kind_raw) {
        kind_raw.to_string()
    } else {
        "other".to_string()
    };
    let summary = clamp_text(
        record.get("summary").and_then(Value::as_str).unwrap_or(""),
        CODEX_LIMITS_SUMMARY,
    );
    let facts = clamp_string_list(
        record.get("facts").and_then(Value::as_array),
        CODEX_LIMITS_FACTS,
        CODEX_LIMITS_FACT,
    );
    let relationships: Vec<crate::utils::entity_codex::CodexRelationship> = record
        .get("relationships")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|relation| {
                    let rel = relation.as_object()?;
                    let target = rel.get("target").and_then(Value::as_str)?.trim().to_string();
                    if target.is_empty() {
                        return None;
                    }
                    let note = clamp_text(
                        rel.get("note").and_then(Value::as_str).unwrap_or(""),
                        CODEX_LIMITS_RELATIONSHIP_NOTE,
                    );
                    Some(crate::utils::entity_codex::CodexRelationship { target, note })
                })
                .take(CODEX_LIMITS_RELATIONSHIPS)
                .collect()
        })
        .unwrap_or_default();
    let first_chapter = record
        .get("firstChapter")
        .and_then(Value::as_i64)
        .filter(|chapter| *chapter >= 0);
    Some(EntityCodexCard {
        name,
        aliases,
        kind,
        summary,
        facts,
        relationships,
        first_chapter,
    })
}

/// 解析系列正典文件（version/seriesId 整包校验；坏条目跳过；name 码元序）。
pub fn parse_series_canon_file(raw: &Value) -> Option<SeriesCanonFile> {
    let record = raw.as_object()?;
    if record.get("version").and_then(Value::as_i64) != Some(SERIES_CANON_VERSION) {
        return None;
    }
    let series_id = record.get("seriesId").and_then(Value::as_str)?;
    if !is_valid_series_id(series_id) {
        return None;
    }
    let entries_raw = record.get("entries").and_then(Value::as_array)?;
    let mut entries: Vec<EntityCodexCard> = entries_raw
        .iter()
        .filter_map(sanitize_entry)
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Some(SeriesCanonFile {
        version: SERIES_CANON_VERSION,
        series_id: series_id.to_string(),
        entries,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexLayerMerge {
    /// 书内卡片（原样返回，顺序不变）。
    pub book: Vec<EntityCodexCard>,
    /// 幸存 series 条目（同名被 book 覆盖剔除后，name 码元序）。
    pub series: Vec<EntityCodexCard>,
}

/// 双层合并：book 同名条目覆盖 series 条目（覆盖即省略，非交织）。
pub fn merge_codex_layers(
    book_cards: &[EntityCodexCard],
    series_entries: &[EntityCodexCard],
) -> CodexLayerMerge {
    let mut series: Vec<EntityCodexCard> = series_entries
        .iter()
        .filter(|entry| !book_cards.iter().any(|card| card.name == entry.name))
        .cloned()
        .collect();
    series.sort_by(|a, b| a.name.cmp(&b.name));
    CodexLayerMerge {
        book: book_cards.to_vec(),
        series,
    }
}

const SERIES_EXCERPT_MAX: usize = 400;

/// 注入渲染：幸存条目命中 → 「## 系列实体卡」块（无命中返回 None）。
pub fn render_series_codex_block(
    matches: &[CodexMatch],
    language: crate::utils::language::WritingLanguage,
) -> Option<String> {
    if matches.is_empty() {
        return None;
    }
    let is_en = language == crate::utils::language::WritingLanguage::En;
    let sections: Vec<String> = matches
        .iter()
        .map(|codex_match| {
            let card = &codex_match.card;
            let mut lines = Vec::new();
            if card.aliases.is_empty() {
                lines.push(format!("### {}", card.name));
            } else {
                lines.push(format!("### {}（{}）", card.name, card.aliases.join("、")));
            }
            if !card.summary.is_empty() {
                if card.summary.chars().count() > SERIES_EXCERPT_MAX {
                    let clipped: String = card.summary.chars().take(SERIES_EXCERPT_MAX).collect();
                    lines.push(format!("{clipped}…"));
                } else {
                    lines.push(card.summary.clone());
                }
            }
            if !card.facts.is_empty() {
                lines.push(if is_en { "Canon facts:".to_string() } else { "正典事实：".to_string() });
                for fact in &card.facts {
                    lines.push(format!("- {fact}"));
                }
            }
            if !card.relationships.is_empty() {
                lines.push(if is_en { "Relationships:".to_string() } else { "关系：".to_string() });
                for relation in &card.relationships {
                    lines.push(format!("- {}: {}", relation.target, relation.note));
                }
            }
            lines.join("\n")
        })
        .collect();
    let header = if is_en {
        "## Series entity cards (cross-book canon — follow canon facts)"
    } else {
        "## 系列实体卡（跨书正典——遵循正典事实）"
    };
    let mut parts = vec![header.to_string()];
    parts.extend(sections);
    Some(parts.join("\n"))
}
