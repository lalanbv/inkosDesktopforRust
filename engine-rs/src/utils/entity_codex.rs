//! R10 实体卡 Codex 化（375 号契约批，三轮 P0）。
//!
//! TS 真源：`packages/core/src/utils/entity-codex.ts`；golden 唯一事实源：
//! `packages/core/src/__tests__/golden/entity-codex-vectors.json`（差分测试
//! `tests/golden_entity_codex_diff.rs` 读同一文件）。
//!
//! 实体卡 = 名册条目的视图扩展（同源派生，非第二真相源）：身份字段
//! （name/aliases/kind）以名册为准，卡片仅承载 summary/facts/relationships
//! 作者扩展。命中计数/排序与 TS 逐字一致（禁 locale 感知排序）。

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRosterEntity {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub registered_at: i64,
}

fn default_kind() -> String {
    "other".to_string()
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRelationship {
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityCodexCard {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub relationships: Vec<CodexRelationship>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_chapter: Option<i64>,
}

/// 名册 → 卡片视图（同源派生）：summary/facts 置空，按 name 码元序排序。
pub fn derive_codex_cards(roster: &[CodexRosterEntity]) -> Vec<EntityCodexCard> {
    let mut cards: Vec<EntityCodexCard> = roster
        .iter()
        .map(|entity| EntityCodexCard {
            name: entity.name.clone(),
            aliases: entity.aliases.clone(),
            kind: entity.kind.clone(),
            summary: String::new(),
            facts: Vec::new(),
            relationships: Vec::new(),
            first_chapter: None,
        })
        .collect();
    cards.sort_by(|a, b| a.name.cmp(&b.name));
    cards
}

const CODEX_LIMITS_SUMMARY: usize = 300;
const CODEX_LIMITS_FACTS: usize = 10;
const CODEX_LIMITS_FACT: usize = 200;
const CODEX_LIMITS_RELATIONSHIPS: usize = 10;
const CODEX_LIMITS_RELATIONSHIP_NOTE: usize = 200;

fn clamp_list(items: &[String], max_items: usize, max_chars: usize) -> Vec<String> {
    items
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .take(max_items)
        .map(|item| {
            if item.chars().count() > max_chars {
                item.chars().take(max_chars).collect()
            } else {
                item
            }
        })
        .collect()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRevision {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub facts: Option<Vec<String>>,
    #[serde(default)]
    pub relationships: Option<Vec<CodexRelationship>>,
}

/// 卡片修订：以名册同源卡为底座合并作者扩展（summary/facts/relationships）。
pub fn revise_codex_card(base: &EntityCodexCard, revision: &CodexRevision) -> EntityCodexCard {
    let summary = revision
        .summary
        .clone()
        .unwrap_or_else(|| base.summary.clone());
    let summary = if summary.chars().count() > CODEX_LIMITS_SUMMARY {
        summary.chars().take(CODEX_LIMITS_SUMMARY).collect()
    } else {
        summary
    };
    EntityCodexCard {
        name: base.name.clone(),
        aliases: base.aliases.clone(),
        kind: base.kind.clone(),
        summary,
        facts: clamp_list(
            revision.facts.as_ref().unwrap_or(&base.facts),
            CODEX_LIMITS_FACTS,
            CODEX_LIMITS_FACT,
        ),
        relationships: {
            let relationships = revision
                .relationships
                .as_ref()
                .unwrap_or(&base.relationships);
            relationships
                .iter()
                .take(CODEX_LIMITS_RELATIONSHIPS)
                .map(|relation| CodexRelationship {
                    target: relation.target.clone(),
                    note: {
                        let note = if relation.note.chars().count() > CODEX_LIMITS_RELATIONSHIP_NOTE {
                            relation.note.chars().take(CODEX_LIMITS_RELATIONSHIP_NOTE).collect()
                        } else {
                            relation.note.clone()
                        };
                        note
                    },
                })
                .collect()
        },
        first_chapter: base.first_chapter,
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexMatch {
    pub card: EntityCodexCard,
    /// 命中次数（name 与 alias 合并计）。
    pub hits: i64,
}

/// 场景命中：text 含 name 或任一 alias 的卡片（0 命中不返回）。
pub fn match_codex_cards(text: &str, cards: &[EntityCodexCard]) -> Vec<CodexMatch> {
    let mut matches: Vec<CodexMatch> = Vec::new();
    for card in cards {
        let mut hits: i64 = 0;
        for term in std::iter::once(&card.name).chain(card.aliases.iter()) {
            if term.is_empty() {
                continue;
            }
            let term_chars = term.chars().count().max(1);
            let mut from = 0usize;
            let char_len = text.chars().count();
            while from <= char_len {
                let hay: String = text.chars().skip(from).collect();
                match hay.find(term.as_str()) {
                    Some(relative) => {
                        hits += 1;
                        from += hay[..relative].chars().count() + term_chars;
                    }
                    None => break,
                }
            }
        }
        if hits > 0 {
            matches.push(CodexMatch { card: card.clone(), hits });
        }
    }
    matches.sort_by(|a, b| {
        b.hits
            .cmp(&a.hits)
            .then_with(|| a.card.name.cmp(&b.card.name))
    });
    matches
}

const CODEX_EXCERPT_MAX: usize = 400;

/// 注入渲染：命中卡片 → 「## 实体卡」块（无命中返回 None）。
pub fn render_codex_block(
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
                if card.summary.chars().count() > CODEX_EXCERPT_MAX {
                    let clipped: String =
                        card.summary.chars().take(CODEX_EXCERPT_MAX).collect();
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
        "## Entity cards (scene cast — follow canon facts)"
    } else {
        "## 实体卡（本场出场——遵循正典事实）"
    };
    let mut parts = vec![header.to_string()];
    parts.extend(sections);
    Some(parts.join("\n"))
}
