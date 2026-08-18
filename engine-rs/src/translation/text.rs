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

/// 段内分段（140 号：TS splitParagraph 升级——句边界打包，超长句按词边界
/// 递归；不再硬切 maxChars）。句/词边界用 UAX #29（TS Intl.Segmenter 的
/// locale 无关对应物——locale 细化规则差异备案）。
use unicode_segmentation::UnicodeSegmentation;

fn split_long_paragraph(paragraph: &str, max_chars: usize) -> Vec<String> {
    let sentences: Vec<String> = paragraph
        .unicode_sentences()
        .map(str::to_string)
        .filter(|sentence| !sentence.trim().is_empty())
        .collect();
    let units = if sentences.is_empty() {
        vec![paragraph.to_string()]
    } else {
        sentences
    };
    pack_boundary_units(&units, max_chars)
}

/// TS `packBoundaryUnits` 逐字：贪心打包到 ≤maxChars；超长 unit 先落当前
/// 缓冲再按词界递归（单词不可再分则整段直出）；尾 trim 过滤空。
fn pack_boundary_units(units: &[String], max_chars: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for unit in units {
        if unit.chars().count() > max_chars {
            if !current.is_empty() {
                chunks.push(current);
                current = String::new();
            }
            let words: Vec<String> = unit
                .split_word_bounds()
                .filter(|word| !word.is_empty())
                .map(str::to_string)
                .collect();
            if words.len() <= 1 {
                chunks.push(unit.clone());
            } else {
                chunks.extend(pack_boundary_units(&words, max_chars));
            }
            continue;
        }
        let candidate = if current.is_empty() {
            unit.clone()
        } else {
            format!("{current}{unit}")
        };
        if candidate.chars().count() <= max_chars {
            current = candidate;
        } else {
            if !current.is_empty() {
                chunks.push(current);
            }
            current = unit.clone();
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
        .into_iter()
        .map(|chunk| chunk.trim().to_string())
        .filter(|chunk| !chunk.is_empty())
        .collect()
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

    // ── 140 号：句界打包（TS splitParagraph 升级对应面） ────────────────

    #[test]
    fn long_paragraph_packs_on_sentence_boundaries() {
        let paragraph = "第一句完整表达一个意思。第二句继续推进情节！第三句收束？";
        let chunks = split_long_paragraph(paragraph, 12);
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 12, "片段超限：{chunk:?}");
            assert!(
                chunk.ends_with('。') || chunk.ends_with('！') || chunk.ends_with('？'),
                "句界打包不应句中截断：{chunk:?}"
            );
        }
        assert_eq!(chunks.concat(), paragraph);
    }

    #[test]
    fn oversize_sentence_recurses_on_word_boundaries() {
        let sentence = "word1 word2 word3 word4 word5 word6 word7";
        let chunks = split_long_paragraph(sentence, 12);
        assert!(chunks.len() >= 2, "{chunks:?}");
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 12, "词界递归后仍超限：{chunk:?}");
        }
        // 连续中文：UAX 词界逐字（TS Intl.Segmenter word 粒度同）→ 递归拆包。
        let unspaced = "这是一个没有任何边界且超过限制长度的连续中文句子没有任何分隔符";
        let chunks = split_long_paragraph(unspaced, 10);
        assert!(chunks.len() >= 2, "{chunks:?}");
        assert!(chunks.iter().all(|c| c.chars().count() <= 10), "{chunks:?}");
        // 真正不可分：单个超长 token（无任何边界）直出。
        let token = "supercalifragilisticexpialidocious";
        let chunks = split_long_paragraph(token, 10);
        assert_eq!(chunks, vec![token.to_string()], "单词不可再分直出");
    }

    #[test]
    fn seg_vec_uses_sentence_packing() {
        let text = "第一句。第二句！第三句？";
        let chunks = segment_translation_text_vec(text, 8);
        assert!(!chunks.is_empty());
        assert!(
            chunks.iter().all(|c| c.ends_with('。') || c.ends_with('！') || c.ends_with('？')),
            "{chunks:?}"
        );
    }
}
