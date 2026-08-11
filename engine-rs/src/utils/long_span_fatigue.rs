//! 长篇疲劳检测 —— 纯文本核心。
//!
//! 移植自 `packages/core/src/utils/long-span-fatigue.ts`（545 行）的纯函数子集：
//! - [`dice_coefficient`] / [`build_bigrams`]：bigram Sørensen–Dice 相似度
//! - [`extract_boundary_sentence`]：提取章节首/尾句
//! - [`normalize_sentence`] / [`summarize_sentence`]：句式归一化与摘要
//!
//! ## 待移植（IO 编排，需 std::fs + cadence 集成）
//! analyzeLongSpanFatigue / buildEnglishVarianceBrief（读 chapter_summaries.md +
//! chapters/*.md，调 analyze_chapter_cadence，组装 issue 列表）。

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

const ENGLISH_STOP_WORDS_LSF: &[&str] = &[
    "the", "and", "but", "with", "from", "into", "that", "this", "there",
    "again", "while", "after", "before", "were", "was", "had", "has", "have", "kept",
];

/// 中文字符与标点（移植 TS CHINESE_PUNCTUATION）。
fn chinese_punct_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[，。！？；：“”‘’（）《》、\s\-—…·]").unwrap())
}
/// 英文非字母数字（移植 TS ENGLISH_PUNCTUATION /[^a-z0-9]+/gi）。
fn english_punct_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)[^a-z0-9]+").unwrap())
}

/// Dice 相似度（bigram）。相同→1；长度<2→0；否则 2*overlap/(总 bigram 数)。
pub fn dice_coefficient(left: &str, right: &str) -> f64 {
    if left == right {
        return 1.0;
    }
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    if left_chars.len() < 2 || right_chars.len() < 2 {
        return 0.0;
    }
    let left_bigrams = build_bigrams(left);
    let right_bigrams = build_bigrams(right);
    let mut overlap = 0u32;
    for (bg, count) in &left_bigrams {
        overlap += (*count).min(*right_bigrams.get(bg).unwrap_or(&0));
    }
    let left_count: u32 = left_bigrams.values().sum();
    let right_count: u32 = right_bigrams.values().sum();
    (2.0 * overlap as f64) / (left_count + right_count) as f64
}

/// 构建 bigram 计数（按 UTF-16 码元为单位的 2-gram；归一化后输出全 BMP，chars 与 UTF-16 一致）。
pub fn build_bigrams(value: &str) -> HashMap<String, u32> {
    let chars: Vec<char> = value.chars().collect();
    let mut result: HashMap<String, u32> = HashMap::new();
    for w in chars.windows(2) {
        let bg: String = w.iter().collect();
        *result.entry(bg).or_insert(0) += 1;
    }
    result
}

/// 提取章节首句或尾句：扁平非空非标题行，按句末标点分句（保留标点，对齐 TS lookbehind），取首/尾。
pub fn extract_boundary_sentence(content: &str, boundary: Boundary) -> Option<String> {
    let flattened: String = content
        .split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    let sentences = split_sentences(&flattened);
    if sentences.is_empty() {
        return None;
    }
    match boundary {
        Boundary::Opening => Some(sentences[0].clone()),
        Boundary::Ending => Some(sentences.last().unwrap().clone()),
    }
}

/// 按句末标点（。！？!?\.）分句，保留标点在前句，句末空白剥离（对齐 TS `/(?<=[。！？!?\.])\s*/`）。
fn split_sentences(s: &str) -> Vec<String> {
    let enders = ['。', '！', '？', '!', '?', '.'];
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        cur.push(c);
        if enders.contains(&c) {
            let trimmed = cur.trim().to_string();
            if !trimmed.is_empty() {
                out.push(trimmed);
            }
            cur.clear();
        }
    }
    let trimmed = cur.trim().to_string();
    if !trimmed.is_empty() {
        out.push(trimmed);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    Opening,
    Ending,
}

/// 句式归一化：英文 lower + 去非字母数字；中文去标点 + lower。
pub fn normalize_sentence(sentence: &str, language: crate::utils::language::WritingLanguage) -> String {
    match language {
        crate::utils::language::WritingLanguage::En => english_punct_re()
            .replace_all(&sentence.to_lowercase(), "")
            .trim()
            .to_string(),
        crate::utils::language::WritingLanguage::Zh => chinese_punct_re()
            .replace_all(sentence, "")
            .to_lowercase(),
    }
}

/// 句式摘要：英文前 6 词；中文前 12 字符（去标点后）。
pub fn summarize_sentence(sentence: &str, language: crate::utils::language::WritingLanguage) -> String {
    match language {
        crate::utils::language::WritingLanguage::En => {
            let lower = sentence.to_lowercase();
            let cleaned = english_punct_re().replace_all(&lower, " ");
            let words: Vec<&str> = cleaned.split_whitespace().take(6).collect();
            let joined = words.join(" ");
            if !joined.is_empty() { joined } else { sentence.chars().take(32).collect() }
        }
        crate::utils::language::WritingLanguage::Zh => {
            let collapsed: String = chinese_punct_re().replace_all(sentence, "").into_owned();
            collapsed.chars().take(12).collect()
        }
    }
}

/// 英文停用词集合（LSF 局部版，移植自 long-span-fatigue.ts）。
pub fn is_english_stop_word_lsf(token: &str) -> bool {
    ENGLISH_STOP_WORDS_LSF.contains(&token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dice_identical() {
        assert_eq!(dice_coefficient("hello", "hello"), 1.0);
    }

    #[test]
    fn dice_short_returns_zero() {
        assert_eq!(dice_coefficient("a", "b"), 0.0);
        assert_eq!(dice_coefficient("ab", "a"), 0.0);
    }

    #[test]
    fn dice_partial_overlap() {
        // "abcd" bigrams: ab bc cd；"abce" bigrams: ab bc ce；overlap=2，total=6 → 4/6
        let d = dice_coefficient("abcd", "abce");
        assert!((d - 0.6667).abs() < 0.001, "实际: {d}");
    }

    #[test]
    fn dice_no_overlap() {
        assert_eq!(dice_coefficient("abcd", "wxyz"), 0.0);
    }

    #[test]
    fn extract_boundary_opening_ending() {
        let content = "# 标题\n第一句开头。第二句。最后结尾。";
        assert_eq!(extract_boundary_sentence(content, Boundary::Opening).unwrap(), "第一句开头。");
        assert_eq!(extract_boundary_sentence(content, Boundary::Ending).unwrap(), "最后结尾。");
    }

    #[test]
    fn extract_boundary_skips_headers() {
        let content = "# H1\n## H2\n真正的第一句。后续。";
        assert_eq!(extract_boundary_sentence(content, Boundary::Opening).unwrap(), "真正的第一句。");
    }

    #[test]
    fn normalize_zh_strips_punct() {
        let n = normalize_sentence("他，走了过来。", crate::utils::language::WritingLanguage::Zh);
        assert_eq!(n, "他走了过来");
    }

    #[test]
    fn normalize_en_lower_and_strip() {
        let n = normalize_sentence("He, walked! Over?", crate::utils::language::WritingLanguage::En);
        assert_eq!(n, "hewalkedover");
    }

    #[test]
    fn summarize_en_first_words() {
        let s = summarize_sentence("The quick brown fox jumps over", crate::utils::language::WritingLanguage::En);
        assert_eq!(s, "the quick brown fox jumps over");
        // 截断到 6 词
        let s2 = summarize_sentence("one two three four five six seven eight", crate::utils::language::WritingLanguage::En);
        assert_eq!(s2, "one two three four five six");
    }

    #[test]
    fn summarize_zh_first_chars() {
        let s = summarize_sentence("他静静地走向那扇古老的门", crate::utils::language::WritingLanguage::Zh);
        assert_eq!(s.chars().count(), 12);
    }
}
