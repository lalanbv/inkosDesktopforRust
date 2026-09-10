//! 章节写作输出解析器（writer-parser）。
//!
//! 移植自 `packages/core/src/agents/writer-parser.ts`（178 行）。把 LLM 的 `=== TAG ===`
//! 分隔输出解析为结构化章节数据。共享于 WriterAgent（写新章）与 ChapterAnalyzerAgent（分析既有章）。
//!
//! 复用 `crate::agents::settler_parser::extract_tag`（lookahead 手动扫描等价实现）。

use regex::Regex;
use std::sync::OnceLock;

use crate::agents::settler_parser::extract_tag;
use crate::models::genre_profile::GenreProfile;
use crate::models::length_governance::LengthCountingMode;
use crate::utils::length_metrics::count_chapter_length;

/// 创意输出（标题 + 正文 + 字数 + 写作自检）。对齐 TS `CreativeOutput`。
#[derive(Debug, Clone, PartialEq)]
pub struct CreativeOutput {
    pub title: String,
    pub content: String,
    pub word_count: u32,
    pub pre_write_check: String,
}

/// 解析创意输出（=== TAG === 优先，失败走 fallback）。对齐 TS `parseCreativeOutput`。
pub fn parse_creative_output(
    chapter_number: u32,
    content: &str,
    counting_mode: LengthCountingMode,
) -> CreativeOutput {
    let mut chapter_content = extract_tag(content, "CHAPTER_CONTENT");
    if chapter_content.is_empty() {
        chapter_content = fallback_extract_content(content, counting_mode);
    }

    let mut title = extract_tag(content, "CHAPTER_TITLE");
    if title.is_empty() {
        title = fallback_extract_title(content, chapter_number, counting_mode);
    }

    CreativeOutput {
        title,
        content: chapter_content.clone(),
        word_count: count_chapter_length(&chapter_content, counting_mode),
        pre_write_check: extract_tag(content, "PRE_WRITE_CHECK"),
    }
}

/// 完整解析输出（WriteChapterOutput 去掉 postWrite 字段）。对齐 TS `ParsedWriterOutput`。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedWriterOutput {
    pub chapter_number: u32,
    pub title: String,
    pub content: String,
    pub word_count: u32,
    pub pre_write_check: String,
    pub post_settlement: String,
    pub updated_state: String,
    pub updated_ledger: String,
    pub updated_hooks: String,
    pub chapter_summary: String,
    pub updated_subplots: String,
    pub updated_emotional_arcs: String,
    pub updated_character_matrix: String,
}

/// 解析 LLM 的 === TAG === 输出为完整章节数据。对齐 TS `parseWriterOutput`。
pub fn parse_writer_output(
    chapter_number: u32,
    content: &str,
    genre_profile: &GenreProfile,
    counting_mode: LengthCountingMode,
) -> ParsedWriterOutput {
    let chapter_content = extract_tag(content, "CHAPTER_CONTENT");
    let title = {
        let t = extract_tag(content, "CHAPTER_TITLE");
        if t.is_empty() { default_chapter_title(chapter_number, counting_mode) } else { t }
    };
    let updated_state = {
        let s = extract_tag(content, "UPDATED_STATE");
        if s.is_empty() { default_state_placeholder(counting_mode) } else { s }
    };
    let updated_ledger = if genre_profile.numerical_system {
        let s = extract_tag(content, "UPDATED_LEDGER");
        if s.is_empty() { default_ledger_placeholder(counting_mode) } else { s }
    } else {
        String::new()
    };
    let updated_hooks = {
        let s = extract_tag(content, "UPDATED_HOOKS");
        if s.is_empty() { default_hooks_placeholder(counting_mode) } else { s }
    };

    ParsedWriterOutput {
        chapter_number,
        title,
        content: chapter_content.clone(),
        word_count: count_chapter_length(&chapter_content, counting_mode),
        pre_write_check: extract_tag(content, "PRE_WRITE_CHECK"),
        post_settlement: extract_tag(content, "POST_SETTLEMENT"),
        updated_state,
        updated_ledger,
        updated_hooks,
        chapter_summary: extract_tag(content, "CHAPTER_SUMMARY"),
        updated_subplots: extract_tag(content, "UPDATED_SUBPLOTS"),
        updated_emotional_arcs: extract_tag(content, "UPDATED_EMOTIONAL_ARCS"),
        updated_character_matrix: extract_tag(content, "UPDATED_CHARACTER_MATRIX"),
    }
}

// --- fallback 提取（小模型常缺 === TAG ===）-----------------------------------

/// fallback 正文提取：# 第N章 标题 / Chapter N / 正文：标签 / 最后手段剥离元数据行。
fn fallback_extract_content(raw: &str, counting_mode: LengthCountingMode) -> String {
    // # 第N章 ... 后的内容。
    if let Some(m) = zh_heading_content_re().captures(raw) {
        return m.get(1).map(|c| c.as_str().trim().to_string()).unwrap_or_default();
    }
    if counting_mode == LengthCountingMode::EnWords {
        if let Some(m) = en_heading_content_re().captures(raw) {
            return m.get(2).map(|c| c.as_str().trim().to_string()).unwrap_or_default();
        }
    }
    // 正文/内容/章节内容：标签。
    if let Some(m) = zh_content_label_re().captures(raw) {
        return m.get(1).map(|c| c.as_str().trim().to_string()).unwrap_or_default();
    }
    if counting_mode == LengthCountingMode::EnWords {
        if let Some(m) = en_content_label_re().captures(raw) {
            return m.get(1).map(|c| c.as_str().trim().to_string()).unwrap_or_default();
        }
    }
    // 最后手段：剥离 tag/key-value 行，保留其余。
    let prose: String = raw
        .lines()
        .filter(|line| {
            let t = line.trim();
            if tag_line_re().is_match(t) {
                return false;
            }
            if metadata_kv_re().is_match(t) {
                return false;
            }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if prose.chars().count() > 100 {
        prose
    } else {
        String::new()
    }
}

/// fallback 标题提取：# 第N章 标题 / Chapter N / 章节标签 / 默认。
fn fallback_extract_title(raw: &str, chapter_number: u32, counting_mode: LengthCountingMode) -> String {
    if let Some(m) = zh_heading_title_re().captures(raw) {
        return m.get(1).map(|c| c.as_str().trim().to_string()).unwrap_or_default();
    }
    if counting_mode == LengthCountingMode::EnWords {
        if let Some(m) = en_heading_title_re().captures(raw) {
            return m.get(1).map(|c| c.as_str().trim().to_string()).unwrap_or_default();
        }
    }
    if let Some(m) = title_label_re().captures(raw) {
        return m.get(1).map(|c| c.as_str().trim().to_string()).unwrap_or_default();
    }
    default_chapter_title(chapter_number, counting_mode)
}

fn default_chapter_title(chapter_number: u32, counting_mode: LengthCountingMode) -> String {
    if counting_mode == LengthCountingMode::EnWords {
        format!("Chapter {chapter_number}")
    } else {
        format!("第{chapter_number}章")
    }
}

fn default_state_placeholder(counting_mode: LengthCountingMode) -> String {
    if counting_mode == LengthCountingMode::EnWords {
        "(state card not updated)".to_string()
    } else {
        "(状态卡未更新)".to_string()
    }
}

fn default_ledger_placeholder(counting_mode: LengthCountingMode) -> String {
    if counting_mode == LengthCountingMode::EnWords {
        "(ledger not updated)".to_string()
    } else {
        "(账本未更新)".to_string()
    }
}

fn default_hooks_placeholder(counting_mode: LengthCountingMode) -> String {
    if counting_mode == LengthCountingMode::EnWords {
        "(hooks pool not updated)".to_string()
    } else {
        "(伏笔池未更新)".to_string()
    }
}

// --- 正则（OnceLock）---------------------------------------------------------

fn zh_heading_content_re() -> &'static Regex {
    // /^#\s*第\d+章[^\n]*\n+([\s\S]+)/m
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^#\s*第\d+章[^\n]*\n+([\s\S]+)").unwrap())
}
fn en_heading_content_re() -> &'static Regex {
    // /^#\s*Chapter\s+\d+(?::|\s+)([^\n]*)\n+([\s\S]+)/im
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?im)^#\s*Chapter\s+\d+(?::|\s+)([^\n]*)\n+([\s\S]+)").unwrap())
}
fn zh_content_label_re() -> &'static Regex {
    // /(?:正文|内容|章节内容)[：:]\s*\n+([\s\S]+)/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?:正文|内容|章节内容)[：:]\s*\n+([\s\S]+)").unwrap())
}
fn en_content_label_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)(?:content|chapter content)[：:]\s*\n+([\s\S]+)").unwrap())
}
fn tag_line_re() -> &'static Regex {
    // /^===\s*[A-Z_]+\s*===/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^===\s*[A-Z_]+\s*===").unwrap())
}
fn metadata_kv_re() -> &'static Regex {
    // /^(PRE_WRITE_CHECK|CHAPTER_TITLE|章节标题|写作自检)[：:]/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(PRE_WRITE_CHECK|CHAPTER_TITLE|章节标题|写作自检)[：:]").unwrap())
}
fn zh_heading_title_re() -> &'static Regex {
    // /^#\s*第\d+章\s*(.+)/m
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^#\s*第\d+章\s*(.+)").unwrap())
}
fn en_heading_title_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?im)^#\s*Chapter\s+\d+(?::|\s+)\s*(.+)").unwrap())
}
fn title_label_re() -> &'static Regex {
    // /(?:章节标题|CHAPTER_TITLE)[：:]\s*(.+)/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?:章节标题|CHAPTER_TITLE)[：:]\s*(.+)").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(numerical: bool) -> GenreProfile {
        GenreProfile { numerical_system: numerical, ..GenreProfile::default() }
    }

    #[test]
    fn parse_creative_output_extracts_tagged_sections() {
        let content = "\
=== CHAPTER_TITLE ===
序章：暗夜

=== CHAPTER_CONTENT ===
这是正文内容，足够长以通过任何长度检查阈值，确保 fallback 不会误触发。

=== PRE_WRITE_CHECK ===
已检查伏笔";
        let out = parse_creative_output(1, content, LengthCountingMode::ZhChars);
        assert_eq!(out.title, "序章：暗夜");
        assert_eq!(out.content, "这是正文内容，足够长以通过任何长度检查阈值，确保 fallback 不会误触发。");
        assert!(out.word_count > 0);
        assert_eq!(out.pre_write_check, "已检查伏笔");
    }

    #[test]
    fn parse_creative_output_fallback_from_heading() {
        // 无 === TAG ===，但有 # 第N章 标题。
        let content = "# 第1章 开端\n\n这是 fallback 正文，需要超过 100 字符才能被采纳作为最后手段的正文，这里补足长度确保通过阈值检查。";
        let out = parse_creative_output(1, content, LengthCountingMode::ZhChars);
        assert_eq!(out.title, "开端");
        assert!(out.content.starts_with("这是 fallback 正文"));
    }

    #[test]
    fn parse_writer_output_full_fields_with_numerical_system() {
        let content = "\
=== CHAPTER_TITLE ===
第2章

=== CHAPTER_CONTENT ===
正文段。

=== UPDATED_STATE ===
森林深处

=== UPDATED_LEDGER ===
金币 +10

=== UPDATED_HOOKS ===
h01 推进

=== CHAPTER_SUMMARY ===
本章高潮
";
        let out = parse_writer_output(2, content, &profile(true), LengthCountingMode::ZhChars);
        assert_eq!(out.chapter_number, 2);
        assert_eq!(out.title, "第2章");
        assert_eq!(out.updated_state, "森林深处");
        assert_eq!(out.updated_ledger, "金币 +10", "numerical_system=true 时保留 ledger");
        assert_eq!(out.updated_hooks, "h01 推进");
        assert_eq!(out.chapter_summary, "本章高潮");
    }

    #[test]
    fn parse_writer_output_ledger_empty_when_numerical_system_false() {
        let content = "=== CHAPTER_CONTENT ===\n正文\n=== UPDATED_LEDGER ===\n金币\n";
        let out = parse_writer_output(1, content, &profile(false), LengthCountingMode::ZhChars);
        assert_eq!(out.updated_ledger, "", "numerical_system=false 强制空");
    }

    #[test]
    fn parse_writer_output_defaults_when_missing() {
        let content = "=== CHAPTER_CONTENT ===\n正文\n";
        let out = parse_writer_output(3, content, &profile(false), LengthCountingMode::ZhChars);
        assert_eq!(out.title, "第3章", "缺标题 → 默认");
        assert_eq!(out.updated_state, "(状态卡未更新)");
        assert_eq!(out.updated_hooks, "(伏笔池未更新)");
    }

    #[test]
    fn english_defaults_used_for_en_words_mode() {
        let content = "=== CHAPTER_CONTENT ===\nbody\n";
        let out = parse_writer_output(5, content, &profile(false), LengthCountingMode::EnWords);
        assert_eq!(out.title, "Chapter 5");
        assert_eq!(out.updated_state, "(state card not updated)");
        assert_eq!(out.updated_hooks, "(hooks pool not updated)");
    }

    #[test]
    fn fallback_extract_content_strips_tag_lines() {
        // 无标题 heading，无标签 → 最后手段剥离 tag 行。prose 需 >100 字符才被采纳。
        // >100 中文字符的 prose（最后手段要求 >100 char 才采纳）。
        let long_prose = "这是一段占位文本用于测试最后手段的正文提取，它必须明显超过一百个字符的阈值才能被采纳，所以我们故意写得非常冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长冗长。";
        let raw = format!(
            "=== CHAPTER_TITLE ===\n标题\n=== CHAPTER_CONTENT ===\n{long_prose}\nPRE_WRITE_CHECK：自检"
        );
        let c = fallback_extract_content(&raw, LengthCountingMode::ZhChars);
        assert!(!c.contains("=== CHAPTER"), "tag 行应剥离: c={c:?}");
        assert!(c.contains("占位文本"), "prose 应保留: c={c:?}");
        assert!(!c.contains("PRE_WRITE_CHECK：自检"), "metadata kv 行应剥离: c={c:?}");
    }

    #[test]
    fn fallback_extract_content_returns_empty_when_too_short() {
        let raw = "短文本";
        assert_eq!(fallback_extract_content(raw, LengthCountingMode::ZhChars), "");
    }

    #[test]
    fn fallback_extract_title_label_form() {
        let raw = "章节标题：测试标题\n正文";
        assert_eq!(fallback_extract_title(raw, 1, LengthCountingMode::ZhChars), "测试标题");
    }
}
