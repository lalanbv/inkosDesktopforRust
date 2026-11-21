//! 全局搜索纯函数核（R47，1640 号）。
//!
//! 移植镜像 `packages/core/src/retrieval/global-search.ts`：查询分词复用
//! [`crate::utils::local_search::tokenize_search_text`]（分词单源，golden
//! search-tokenizer-vectors 已锁双端），命中定位/高亮裁剪/加权字段扫描为
//! 本模块纯函数（golden search-highlight-vectors 锁双端）。
//!
//! 偏移口径备案：匹配在 `to_lowercase()` 投影串上定位，再按字符下标映射回
//! 原串取窗。极小语种（如 U+0130 小写化变长）可能错位——best-effort 高亮，
//! 不影响命中本身；双端实现逐字对称，golden 向量锁定常规 CJK/Latin 面。

use crate::utils::local_search::tokenize_search_text;

/// 高亮片段前后窗（字符数）。对齐 TS `SEARCH_CONTEXT_BEFORE/AFTER`。
pub const SEARCH_CONTEXT_BEFORE: usize = 48;
pub const SEARCH_CONTEXT_AFTER: usize = 96;

/// 省略号标记（片段被裁剪时加在首/尾）。对齐 TS `SNIPPET_ELLIPSIS`。
pub const SNIPPET_ELLIPSIS: &str = "…";

/// 高亮裁剪结果：片段文本 + 命中在片段内的字符下标与长度。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSnippet {
    pub text: String,
    pub match_start: usize,
    pub match_length: usize,
}

/// 查询匹配 token 集：分词单源 + 去重保序 + 原始/紧缩短语候选。
///
/// 候选 = 分词 token + 原始短语（trim+lowercase）+ 紧缩短语（去内部空白）。
/// 分词单源只产 bigram（「碎镜之主」→ 碎镜/镜之/之主），整短语高亮依赖
/// 紧缩候选。排序 = 长度降序、同长按首现序（双端同一比较器，golden 锁定）
/// ——最长候选优先命中。
pub fn query_tokens(query: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut tokens: Vec<(usize, String)> = tokenize_search_text(query)
        .into_iter()
        .enumerate()
        .filter(|(_, token)| seen.insert(token.clone()))
        .collect();
    let raw = query.trim().to_lowercase();
    let condensed = raw.replace([' ', '\t', '\n', '\r'], "");
    for candidate in [raw.as_str(), condensed.as_str()] {
        if !candidate.is_empty() && seen.insert(candidate.to_string()) {
            tokens.push((usize::MAX, candidate.to_string()));
        }
    }
    tokens.sort_by(|a, b| {
        b.1.chars()
            .count()
            .cmp(&a.1.chars().count())
            .then(a.0.cmp(&b.0))
    });
    tokens.into_iter().map(|(_, token)| token).collect()
}

/// 在 `content` 中定位任一查询 token 的最早出现（同位取最长候选）。
/// 返回原串上的 (字符下标, 字符长度)；无命中 → `None`。
pub fn find_query_match(content: &str, tokens: &[String]) -> Option<(usize, usize)> {
    if tokens.is_empty() || content.is_empty() {
        return None;
    }
    let haystack = content.to_lowercase();
    let mut best: Option<(usize, usize)> = None; // (字符下标, 字符长度)
    for token in tokens {
        let Some(byte_pos) = haystack.find(token.as_str()) else {
            continue;
        };
        // 投影串字符下标 = 前缀字符计数（投影与原串逐字符等长面内成立）。
        let char_pos = haystack[..byte_pos].chars().count();
        let char_len = token.chars().count();
        let better = match best {
            None => true,
            Some((current_pos, current_len)) => {
                char_pos < current_pos || (char_pos == current_pos && char_len > current_len)
            }
        };
        if better {
            best = Some((char_pos, char_len));
        }
    }
    best
}

/// 高亮片段：最早命中居中裁剪（前 [`SEARCH_CONTEXT_BEFORE`] 后
/// [`SEARCH_CONTEXT_AFTER`] 字符），被裁侧加省略号；`match_start/length`
/// 为片段内字符坐标（含省略号前缀偏移）。无命中 → `None`。
pub fn build_highlight_snippet(content: &str, query: &str) -> Option<HighlightSnippet> {
    let tokens = query_tokens(query);
    find_query_match(content, &tokens).map(|(char_pos, char_len)| {
        let total: usize = content.chars().count();
        let start = char_pos.saturating_sub(SEARCH_CONTEXT_BEFORE);
        let end = (char_pos + char_len + SEARCH_CONTEXT_AFTER).min(total);
        let prefix = if start > 0 { SNIPPET_ELLIPSIS } else { "" };
        let suffix = if end < total { SNIPPET_ELLIPSIS } else { "" };
        let body: String = content.chars().skip(start).take(end - start).collect();
        let text = format!("{prefix}{body}{suffix}");
        HighlightSnippet {
            // 片段内命中下标 = 省略号前缀宽 + 窗口内偏移。
            match_start: prefix.chars().count() + (char_pos - start),
            match_length: char_len,
            text,
        }
    })
}

/// 加权字段扫描结果：命中字段下标 + 权重 + 片段内坐标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedFieldMatch {
    pub field_index: usize,
    pub weight: i64,
    pub char_pos: usize,
    pub char_length: usize,
}

/// 加权字段扫描（名册/账本轴）：按给定顺序取首个命中字段。
pub fn find_weighted_field_match(
    fields: &[(&str, i64)],
    query: &str,
) -> Option<WeightedFieldMatch> {
    let tokens = query_tokens(query);
    if tokens.is_empty() {
        return None;
    }
    for (index, (text, weight)) in fields.iter().enumerate() {
        if let Some((char_pos, char_len)) = find_query_match(text, &tokens) {
            return Some(WeightedFieldMatch {
                field_index: index,
                weight: *weight,
                char_pos,
                char_length: char_len,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_tokens_longest_first_with_raw_phrase() {
        // 原始短语候选 + 长度降序（同长保首现序）。
        let tokens = query_tokens("碎镜之主");
        assert_eq!(tokens.first().map(String::as_str), Some("碎镜之主"));
        assert!(tokens.contains(&"碎镜".to_string()));
    }

    #[test]
    fn query_tokens_dedup_and_ascii_single_char_drop() {
        // ASCII 单字符被分词器丢弃（分词单源行为）；重复 token 去重；
        // 相邻单字 bigram 由分词单源补位；原始+紧缩短语候选按长度入列。
        let tokens = query_tokens("林 林 a");
        assert_eq!(
            tokens,
            vec![
                "林 林 a".to_string(),
                "林林a".to_string(),
                "林林".to_string(),
                "林".to_string()
            ]
        );
    }

    #[test]
    fn find_query_match_prefers_earliest_then_longest() {
        let tokens = query_tokens("镜 河");
        let (pos, len) = find_query_match("江面有镜，镜中有河。镜河同框", &tokens).unwrap();
        assert_eq!(pos, 3); // 最早的「镜」
        assert_eq!(len, 1);
        // 长候选优先于其子串碎片：空白分隔查询的紧缩短语「碎镜之主」
        // （4 字）先于分词 bigram「碎镜」（2 字）命中同位。
        let (pos, len) =
            find_query_match("传说中的碎镜之主沉睡千年", &query_tokens("碎镜 之主")).unwrap();
        assert_eq!((pos, len), (4, 4));
    }

    #[test]
    fn highlight_snippet_window_and_ellipsis() {
        let content = "a".repeat(200) + "碎镜之主" + &"b".repeat(200);
        let snippet = build_highlight_snippet(&content, "碎镜之主").unwrap();
        assert!(snippet.text.starts_with(SNIPPET_ELLIPSIS));
        assert!(snippet.text.ends_with(SNIPPET_ELLIPSIS));
        // 片段内命中下标 = 省略号(1) + 前窗(48)；窗宽 = 48+4+96。
        assert_eq!(snippet.match_start, 1 + 48);
        assert_eq!(snippet.match_length, 4);
        assert_eq!(snippet.text.chars().count(), 1 + 48 + 4 + 96 + 1);
    }

    #[test]
    fn highlight_snippet_no_clip_at_boundaries() {
        let snippet = build_highlight_snippet("短文含碎镜。", "碎镜").unwrap();
        assert_eq!(snippet.text, "短文含碎镜。");
        assert_eq!(snippet.match_start, 3);
        assert_eq!(snippet.match_length, 2);
    }

    #[test]
    fn highlight_snippet_none_on_miss() {
        assert!(build_highlight_snippet("毫无相关内容", "碎镜之主").is_none());
        assert!(build_highlight_snippet("任意内容", "").is_none());
    }

    #[test]
    fn weighted_fields_first_hit_wins() {
        let fields = [("主角别名区", 3i64), ("备注提到碎镜", 1i64)];
        let matched = find_weighted_field_match(&fields, "碎镜").expect("第二字段应命中");
        assert_eq!(matched.field_index, 1);
        assert_eq!(
            (matched.weight, matched.char_pos, matched.char_length),
            (1, 4, 2)
        );
        assert!(find_weighted_field_match(&fields, "不存在").is_none());
    }
}
