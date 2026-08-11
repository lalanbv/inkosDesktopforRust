//! 翻译文本处理（规范化、章节切分、分段、HTML 剥离/解码）。
//!
//! 移植自 `packages/core/src/translation/text.ts`。

use crate::utils::split_chapters;
use regex::Regex;
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct TranslationTextChapter {
    pub title: String,
    pub content: String,
}

fn trailing_ws_line_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[ \t]+\n").unwrap())
}
fn excess_newlines_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n{4,}").unwrap())
}
fn heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^#{1,3}\s+(.+?)\s*$").unwrap())
}
fn script_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?is)<script.*?</script>").unwrap())
}
fn style_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?is)<style.*?</style>").unwrap())
}
fn br_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)<br\s*/?>").unwrap())
}
fn close_tag_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)</(?:p|div|h[1-6]|li|section|article)>").unwrap())
}
fn any_tag_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"<[^>]+>").unwrap())
}
fn numeric_entity_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"&#(\d+);").unwrap())
}
fn hex_entity_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)&#x([0-9a-f]+);").unwrap())
}

/// HTML 实体解码（named + numeric + hex）。
pub fn decode_html(value: &str) -> String {
    let s = Regex::new(r"(?i)&nbsp;").unwrap().replace_all(value, " ").into_owned();
    let s = Regex::new(r"(?i)&amp;").unwrap().replace_all(&s, "&").into_owned();
    let s = Regex::new(r"(?i)&lt;").unwrap().replace_all(&s, "<").into_owned();
    let s = Regex::new(r"(?i)&gt;").unwrap().replace_all(&s, ">").into_owned();
    let s = Regex::new(r"(?i)&quot;").unwrap().replace_all(&s, "\"").into_owned();
    let s = Regex::new(r"(?i)&#39;").unwrap().replace_all(&s, "'").into_owned();
    // numeric &#code;
    let s = numeric_entity_re()
        .replace_all(&s, |caps: &regex::Captures| {
            caps[1].parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned();
    // hex &#xcode;
    hex_entity_re()
        .replace_all(&s, |caps: &regex::Captures| {
            u32::from_str_radix(&caps[1], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned()
}

/// 规范化翻译文本：HTML 解码 + CRLF→LF + 行尾空白 + 折叠 4+ 换行 + trim。
pub fn normalize_translation_text(value: &str) -> String {
    let s = decode_html(value);
    let s = s.replace("\r\n", "\n");
    let s = trailing_ws_line_re().replace_all(&s, "\n").into_owned();
    let s = excess_newlines_re().replace_all(&s, "\n\n\n").into_owned();
    s.trim().to_string()
}

/// 按章节标题切分（先 splitChapters，失败转 markdown 标题，再失败整体作 Chapter 1）。
pub fn split_translation_chapters(text: &str) -> Vec<TranslationTextChapter> {
    let normalized = normalize_translation_text(text);
    let detected = split_chapters(&normalized, None);
    if !detected.is_empty() {
        return detected.into_iter().map(|c| TranslationTextChapter { title: c.title, content: c.content }).collect();
    }
    let md = split_markdown_headings(&normalized);
    if !md.is_empty() {
        return md;
    }
    vec![TranslationTextChapter { title: "Chapter 1".into(), content: normalized }]
}

/// 按段落 + maxChars 分段（返回片段列表，对齐 TS segmentTranslationText）。
pub fn segment_translation_text_vec(text: &str, max_chars: usize) -> Vec<String> {
    let normalized = normalize_translation_text(text);
    if normalized.is_empty() {
        return Vec::new();
    }
    let re = Regex::new(r"\n{2,}").unwrap();
    let paragraphs: Vec<String> = re
        .split(&normalized)
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    let units: Vec<String> = if !paragraphs.is_empty() { paragraphs } else { vec![normalized] };
    units
        .into_iter()
        .flat_map(|p| {
            let len = p.chars().count();
            if len > max_chars {
                split_long_paragraph(&p, max_chars)
            } else {
                vec![p]
            }
        })
        .collect()
}

fn split_markdown_headings(text: &str) -> Vec<TranslationTextChapter> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut headings: Vec<(String, usize)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(caps) = heading_re().captures(line) {
            headings.push((caps[1].trim().to_string(), i));
        }
    }
    if headings.is_empty() {
        return Vec::new();
    }
    headings
        .iter()
        .enumerate()
        .map(|(idx, (title, line))| {
            let next = headings.get(idx + 1).map(|(_, l)| *l).unwrap_or(lines.len());
            let content: String = lines[(*line + 1).min(next)..next].join("\n").trim().to_string();
            TranslationTextChapter { title: title.clone(), content }
        })
        .filter(|c| !c.content.trim().is_empty())
        .collect()
}

fn split_long_paragraph(paragraph: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = paragraph.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + max_chars).min(chars.len());
        let chunk: String = chars[start..end].iter().collect();
        let trimmed = chunk.trim().to_string();
        if !trimmed.is_empty() {
            chunks.push(trimmed);
        }
        start += max_chars;
    }
    chunks
}

/// 剥 HTML 标签 + 解码实体（script/style→空格，br/块级闭合→换行）。
pub fn strip_html(html: &str) -> String {
    let s = script_re().replace_all(html, " ").into_owned();
    let s = style_re().replace_all(&s, " ").into_owned();
    let s = br_re().replace_all(&s, "\n").into_owned();
    let s = close_tag_re().replace_all(&s, "\n").into_owned();
    let s = any_tag_re().replace_all(&s, " ").into_owned();
    decode_html(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_named_and_numeric_entities() {
        assert_eq!(decode_html("a&nbsp;b &amp; c &lt;tag&gt; &quot;q&quot; &#39;s"), "a b & c <tag> \"q\" 's");
        assert_eq!(decode_html("&#65;&#66;&#67;"), "ABC"); // numeric
        assert_eq!(decode_html("&#x4e2d;&#x6587;"), "中文"); // hex
    }

    #[test]
    fn normalize_collapses_whitespace() {
        let s = normalize_translation_text("a\r\nb   \n\n\n\n\nc");
        assert_eq!(s, "a\nb\n\n\nc");
    }

    #[test]
    fn split_by_zhang_chapters() {
        let text = "第一章 觉醒\n内容一\n第二章 出发\n内容二";
        let chapters = split_translation_chapters(text);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "觉醒");
    }

    #[test]
    fn split_fallback_markdown_headings() {
        let text = "# Intro\n前言\n## Chapter A\n内容A\n## Chapter B\n内容B";
        let chapters = split_translation_chapters(text);
        assert!(chapters.len() >= 2);
        assert!(chapters.iter().any(|c| c.title == "Chapter A"));
    }

    #[test]
    fn split_fallback_single_chapter() {
        let text = "纯文本无标题";
        let chapters = split_translation_chapters(text);
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "Chapter 1");
    }

    #[test]
    fn segment_by_paragraphs() {
        let text = "段落一。\n\n段落二。\n\n段落三。";
        let segs = segment_translation_text_vec(text, 1200);
        assert_eq!(segs.len(), 3);
    }

    #[test]
    fn segment_splits_long_paragraph() {
        let long = "字".repeat(50);
        let text = long.clone();
        let segs = segment_translation_text_vec(&text, 20);
        assert!(segs.len() >= 3); // 50 字 / 20 = 3 段
    }

    #[test]
    fn strip_html_tags_and_entities() {
        let html = "<p>正文</p>\n<script>alert(1)</script>\n<br/>&amp;";
        let out = strip_html(html);
        assert!(out.contains("正文"));
        assert!(!out.contains("<p>"));
        assert!(!out.contains("script"));
        assert!(out.contains("&")); // &amp; 解码
    }
}
