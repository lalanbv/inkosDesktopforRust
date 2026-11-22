//! R52/1656 号：编辑器实体命中集扫描纯函数（TS 镜像 =
//! packages/core/src/utils/entity-highlight.ts，golden
//! entity-highlight-vectors.json 锁定；码点下标口径）。纯前端功能
//! （overlay 组件消费），Rust 侧仅 golden 纪律镜像——零引擎接线。

/// 命中（TS `EntityHighlightHit` 镜像）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityHighlightHit {
    pub entity: String,
    pub card_index: usize,
    pub start: usize,
    pub end: usize,
}

/// 卡入参（TS `EntityCardLike` 镜像）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityCardLike {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// 命中集扫描：cards 的 name+aliases 全部出现位置（大小写敏感+exclude
/// 条目级+重叠最长优先+minLength 缺省 2；去重叠后 start 升序）。TS 镜像
/// = `scanEntityHighlights`。
pub fn scan_entity_hits(
    text: &str,
    cards: &[EntityCardLike],
    min_length: usize,
    exclude: &[String],
) -> Vec<EntityHighlightHit> {
    let min_length = if min_length == 0 { 2 } else { min_length };
    let codepoints: Vec<char> = text.chars().collect();
    let mut seen = std::collections::HashSet::new();
    let mut terms: Vec<(Vec<char>, usize)> = Vec::new();
    for (card_index, card) in cards.iter().enumerate() {
        let mut names = vec![card.name.clone()];
        names.extend(card.aliases.iter().cloned());
        for name in names {
            if seen.contains(&name) {
                continue;
            }
            if name.chars().count() < min_length {
                continue;
            }
            if exclude.iter().any(|phrase| phrase == &name) {
                continue;
            }
            seen.insert(name.clone());
            terms.push((name.chars().collect(), card_index));
        }
    }
    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    for (term, card_index) in &terms {
        let first = match term.first() {
            Some(first) => *first,
            None => continue,
        };
        let mut i = 0usize;
        while i + term.len() <= codepoints.len() {
            if codepoints[i] != first {
                i += 1;
                continue;
            }
            if codepoints[i..i + term.len()] == term[..] {
                spans.push((i, i + term.len(), *card_index));
            }
            i += 1;
        }
    }
    // 贪心去重叠：start 升序、长度降序、卡序稳定。
    spans.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then((b.1 - b.0).cmp(&(a.1 - a.0)))
            .then(a.2.cmp(&b.2))
    });
    let mut hits = Vec::new();
    let mut cursor = 0usize;
    for (start, end, card_index) in spans {
        if start >= cursor {
            hits.push(EntityHighlightHit {
                entity: codepoints[start..end].iter().collect(),
                card_index,
                start,
                end,
            });
            cursor = end;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_alias_wins_and_case_sensitive() {
        let cards = vec![
            EntityCardLike {
                name: "林动".into(),
                aliases: vec!["林动大哥".into()],
            },
            EntityCardLike {
                name: "Lin Dong".into(),
                aliases: vec![],
            },
        ];
        let hits = scan_entity_hits("林动大哥来了，lin dong 不是 Lin Dong。", &cards, 2, &[]);
        let entities: Vec<&str> = hits.iter().map(|hit| hit.entity.as_str()).collect();
        // 大小写敏感：小写 lin dong 不命中；别名最长优先。
        assert_eq!(entities, vec!["林动大哥", "Lin Dong"]);
        assert_eq!(hits[0].start, 0);
        assert_eq!(hits[0].end, 4);
    }

    #[test]
    fn exclude_and_min_length() {
        let cards = vec![
            EntityCardLike {
                name: "林动".into(),
                aliases: vec![],
            },
            EntityCardLike {
                name: "云".into(),
                aliases: vec![],
            },
        ];
        // 排除「林动」；单字「云」< minLength 2 过滤。
        let hits = scan_entity_hits("林动与云", &cards, 2, &["林动".to_string()]);
        assert!(hits.is_empty());
    }
}
