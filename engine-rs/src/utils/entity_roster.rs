//! 实体名册 + 新专名确认卡（G7b/342 号，Phase B 批次二末项）。
//!
//! TS 真源：`packages/core/src/utils/entity-roster.ts`；共享向量：
//! `packages/core/src/__tests__/golden/entity-roster-vectors.json`
//! （差分测试 `tests/golden_entity_roster_diff.rs`）。
//!
//! 三选判定顺序：包含关系（昵称后缀）→ alias；编辑距离 ≤1 → typo；其余 new-entity。
//! 精确命中（name/aliases，大小写不敏感）→ 已登记不出卡。

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterEntity {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub kind: String,
    #[serde(default)]
    pub registered_at: i64,
}

const ENTITY_HEADING: &str = "## entity-";

/// 解析实体名册（手改容错：kind 非法归 other，坏节/无名跳过）。
pub fn parse_entity_roster(markdown: &str) -> Vec<RosterEntity> {
    const KINDS: [&str; 5] = ["person", "place", "faction", "item", "other"];
    let mut entries: Vec<RosterEntity> = Vec::new();
    let mut current_id: Option<String> = None;
    let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut has_current = false;

    macro_rules! flush {
        () => {
            if has_current {
                let name = fields.get("name").map(|s| s.trim().to_string()).unwrap_or_default();
                if !name.is_empty() {
                    let aliases = split_list(fields.get("aliases").map(String::as_str).unwrap_or(""));
                    let kind_raw = fields.get("kind").map(|s| s.trim().to_string()).unwrap_or_else(|| "other".into());
                    let kind = if KINDS.contains(&kind_raw.as_str()) { kind_raw } else { "other".to_string() };
                    let registered_at = fields
                        .get("registeredAt")
                        .map(String::as_str)
                        .unwrap_or("")
                        .trim()
                        .parse::<i64>()
                        .unwrap_or(0);
                    entries.push(RosterEntity { id: current_id.clone().unwrap_or_default(), name, aliases, kind, registered_at });
                }
            }
            has_current = false;
        };
    }

    for raw_line in markdown.lines() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("## ") {
            let name = rest.trim();
            if name.starts_with(ENTITY_HEADING.trim_start_matches("## ")) && name.len() >= 5 {
                flush!();
                current_id = Some(name.to_string());
                has_current = true;
                fields.clear();
                continue;
            }
        }
        if !has_current {
            continue;
        }
        if let Some(rest) = line.strip_prefix("- ") {
            if let Some(colon) = rest.find(':') {
                fields.insert(rest[..colon].trim().to_string(), rest[colon + 1..].trim_start().to_string());
            }
        }
    }
    flush!();
    entries
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split([',', '，', '、', ';', '；'])
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// 渲染名册（防呆方言：每条一节、平铺 key: value 行）。
pub fn render_entity_roster(entries: &[RosterEntity]) -> String {
    let sections: Vec<String> = entries
        .iter()
        .map(|entry| {
            format!(
                "## {}\n- name: {}\n- aliases: {}\n- kind: {}\n- registeredAt: {}",
                entry.id,
                entry.name,
                entry.aliases.join(", "),
                entry.kind,
                entry.registered_at,
            )
        })
        .collect();
    let mut parts = vec!["# 实体名册（Entity Roster）".to_string()];
    parts.extend(sections);
    parts.join("\n\n")
}

/// 从章摘要 characters 列提取人名候选（去重保序、剔除停用词）。
pub fn extract_character_candidates(
    summaries: &[(i64, String)],
) -> Vec<String> {
    const STOPWORDS: [&str; 9] = ["无", "没有", "暂无", "none", "n/a", "unknown", "未知", "other", "others"];
    let mut ordered: Vec<(i64, String)> = summaries.to_vec();
    ordered.sort_by_key(|(chapter, _)| *chapter);
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for (_, characters) in ordered {
        for raw in characters.split([',', '，', '、', ';', '；', '/']) {
            let name = raw.trim();
            if name.is_empty() || STOPWORDS.iter().any(|s| name.eq_ignore_ascii_case(s)) {
                continue;
            }
            let key = name.to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            out.push(name.to_string());
        }
    }
    out
}

/// 经典 Levenshtein 距离（双端一致的纯 DP 实现）。
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut current_row = vec![i];
        for j in 1..=b.len() {
            let substitution = previous[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            current_row.push((previous[j] + 1).min(current_row[j - 1] + 1).min(substitution));
        }
        previous = current_row;
    }
    previous[b.len()]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterCandidateCard {
    pub candidate: String,
    pub action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_name: Option<String>,
    pub confidence: &'static str,
}

fn action_rank(action: &str) -> u8 {
    match action {
        "typo" => 0,
        "alias" => 1,
        _ => 2,
    }
}

fn confidence_rank(confidence: &str) -> u8 {
    match confidence {
        "high" => 0,
        "medium" => 1,
        _ => 2,
    }
}

/// 新专名比对（settle 时调用）：判定顺序 包含关系→alias / 编辑距离≤1→typo / 其余 new。
pub fn resolve_roster_candidates(
    candidates: &[String],
    roster: &[RosterEntity],
) -> Vec<RosterCandidateCard> {
    let mut cards: Vec<RosterCandidateCard> = Vec::new();
    for candidate in candidates {
        let key = candidate.to_lowercase();
        let matched = roster.iter().any(|entity| {
            entity.name.to_lowercase() == key
                || entity.aliases.iter().any(|alias| alias.to_lowercase() == key)
        });
        if matched {
            continue;
        }

        let mut alias_target: Option<String> = None;
        let mut typo_target: Option<String> = None;
        for entity in roster {
            let name_chars = entity.name.chars().count();
            if name_chars >= 2
                && (candidate.contains(&entity.name) || entity.name.contains(candidate.as_str()))
            {
                alias_target = Some(entity.name.clone());
                break;
            }
            if name_chars >= 2 && edit_distance(candidate, &entity.name) <= 1 {
                typo_target = Some(entity.name.clone());
                break;
            }
            for alias in &entity.aliases {
                if alias.chars().count() >= 2 && edit_distance(candidate, alias) <= 1 {
                    alias_target = Some(entity.name.clone());
                    break;
                }
            }
            if alias_target.is_some() {
                break;
            }
        }
        if let Some(target) = alias_target {
            cards.push(RosterCandidateCard {
                candidate: candidate.clone(),
                action: "alias",
                target_name: Some(target),
                confidence: "medium",
            });
            continue;
        }
        if let Some(target) = typo_target {
            cards.push(RosterCandidateCard {
                candidate: candidate.clone(),
                action: "typo",
                target_name: Some(target),
                confidence: "high",
            });
            continue;
        }
        cards.push(RosterCandidateCard {
            candidate: candidate.clone(),
            action: "new-entity",
            target_name: None,
            confidence: "low",
        });
    }
    cards.sort_by(|a, b| {
        action_rank(a.action)
            .cmp(&action_rank(b.action))
            .then(confidence_rank(a.confidence).cmp(&confidence_rank(b.confidence)))
            .then(a.candidate.cmp(&b.candidate))
    });
    cards
}

/// 确认卡人话渲染（聊天确认卡/审计注入共用）；空卡返回 None。
pub fn render_roster_confirmation_card(
    cards: &[RosterCandidateCard],
    language: &str,
) -> Option<String> {
    if cards.is_empty() {
        return None;
    }
    let is_en = language == "en";
    let mut lines = vec![
        if is_en {
            "Unregistered proper nouns found in this chapter (new entity / alias / typo):".to_string()
        } else {
            "本章发现未登记专名（新实体 / 别名 / 笔误）：".to_string()
        },
    ];
    for card in cards {
        let line = if card.action == "typo" {
            if is_en {
                format!("- \"{}\" → possible typo of \"{}\"?", card.candidate, card.target_name.clone().unwrap_or_default())
            } else {
                format!("- \"{}\" → 疑似 \"{}\" 的笔误？", card.candidate, card.target_name.clone().unwrap_or_default())
            }
        } else if card.action == "alias" {
            if is_en {
                format!("- \"{}\" → possible alias of \"{}\"?", card.candidate, card.target_name.clone().unwrap_or_default())
            } else {
                format!("- \"{}\" → 可能是 \"{}\" 的别名？", card.candidate, card.target_name.clone().unwrap_or_default())
            }
        } else if is_en {
            format!("- \"{}\" → new entity?", card.candidate)
        } else {
            format!("- \"{}\" → 新实体？", card.candidate)
        };
        lines.push(line);
    }
    Some(lines.join("\n"))
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityRosterContract {
    pub actions: Vec<&'static str>,
    pub kinds: Vec<&'static str>,
    pub truth_file: &'static str,
    #[serde(rename = "typoMaxDistance")]
    pub typo_max_distance: usize,
    #[serde(rename = "containmentMinChars")]
    pub containment_min_chars: usize,
}

pub fn entity_roster_contract() -> EntityRosterContract {
    EntityRosterContract {
        actions: vec!["new-entity", "alias", "typo"],
        kinds: vec!["person", "place", "faction", "item", "other"],
        truth_file: "story/entity_roster.md",
        typo_max_distance: 1,
        containment_min_chars: 2,
    }
}
