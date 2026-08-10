//! 章节拆分器。
//!
//! 移植自 `packages/core/src/utils/chapter-splitter.ts`（80 行）。
//!
//! 按章节标题行（「第X章/回」含 CJK 数字 / 「Chapter N/M」罗马数字）把整篇文本拆成章节。

use regex::Regex;
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::sync::OnceLock;

/// 拆分出的单个章节。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct SplitChapter {
    pub title: String,
    pub content: String,
}

/// 默认章节标题正则（逐字移植 TS，\d→[0-9] 对齐 JS 无 /u 的 ASCII \d；`(?i)` 对齐 /i）。
/// 匹配：「第[CJK数字/0-9]+章|回」可选冒号/空白 + 标题；或「Chapter + 数字/罗马」。
fn default_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?i)^#{0,2}\s*(?:第[零〇○Ｏ０一二三四五六七八九十百千万0-9]+(?:章|回)(?:[:：]|\s+)?\s*(.*)|Chapter\s+(?:[0-9]+|[IVXLCDM]+)(?:\.|:|\s+)?\s*(.*))",
        ).expect("默认章节正则合法")
    })
}

fn chapter_word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // inferFallbackTitle 用：headingLine 是否匹配 chapter/罗马
    R.get_or_init(|| Regex::new(r"(?i)chapter\s+(?:[0-9]+|[ivxlcdm]+)").unwrap())
}

fn chinese_hui_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"第[零一二三四五六七八九十百千万0-9]+回").unwrap())
}

fn gutenberg_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?im)^\s*Project Gutenberg(?:™|\(TM\))?.*$").unwrap())
}

/// 按章节标题拆分文本。`pattern` 为 None 用默认正则；自定义按 `(?m)` 多行模式编译。
///
/// 每个匹配标记新章节起始，匹配间的内容归属前一章节。无匹配返回空 vec。
pub fn split_chapters(text: &str, pattern: Option<&str>) -> Vec<SplitChapter> {
    let custom = pattern.and_then(|p| Regex::new(&format!("(?m){p}")).ok());
    let re = custom.as_ref().unwrap_or_else(|| default_re());

    let lines: Vec<&str> = text.split('\n').collect();
    // (title, start_line_index)
    let mut marks: Vec<(String, usize)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(caps) = re.captures(line) {
            // JS: match[1] ?? match[2]（中文分支捕获组 1，Chapter 分支捕获组 2）
            let title = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            marks.push((title, i));
        }
    }

    if marks.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<SplitChapter> = Vec::with_capacity(marks.len());
    for (idx, (title, start)) in marks.iter().enumerate() {
        let next_start = if idx + 1 < marks.len() {
            marks[idx + 1].1
        } else {
            lines.len()
        };
        // 内容从标题行的下一行起
        let cap = next_start.min(lines.len());
        let content_lines = if *start < cap {
            &lines[*start + 1..cap]
        } else {
            &[][..]
        };
        let joined = content_lines.join("\n");
        let content = strip_trailing_license(&joined).trim().to_string();
        let title = if title.is_empty() {
            infer_fallback_title(lines.get(*start).copied().unwrap_or(""), idx + 1)
        } else {
            title.clone()
        };
        result.push(SplitChapter { title, content });
    }
    result
}

fn strip_trailing_license(content: &str) -> String {
    match gutenberg_re().find(content) {
        Some(m) => content[..m.start()].trim_end().to_string(),
        None => content.to_string(),
    }
}

fn infer_fallback_title(heading_line: &str, chapter_number: usize) -> String {
    if chapter_word_re().is_match(heading_line) {
        return format!("Chapter {chapter_number}");
    }
    if chinese_hui_re().is_match(heading_line) {
        return format!("第{chapter_number}回");
    }
    format!("第{chapter_number}章")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_chinese_zhang_chapters() {
        let text = "序言内容\n第一章 觉醒\n主角醒来。\n第二章 出发\n他们离开了。";
        let chapters = split_chapters(text, None);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "觉醒");
        assert_eq!(chapters[0].content, "主角醒来。");
        assert_eq!(chapters[1].title, "出发");
        assert!(chapters[1].content.contains("他们离开了"));
    }

    #[test]
    fn splits_with_heading_hash_prefix() {
        let text = "## 第3章 转折\n内容。\n## 第4章 结局\n结尾。";
        let chapters = split_chapters(text, None);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "转折");
    }

    #[test]
    fn splits_english_chapter_roman() {
        let text = "Intro\nCHAPTER I.\nFirst content.\nCHAPTER II.\nSecond.";
        let chapters = split_chapters(text, None);
        assert_eq!(chapters.len(), 2);
        // Chapter 分支的标题捕获组（II 后无标题文本）→ 空 → fallback "Chapter 1"
        assert_eq!(chapters[0].title, "Chapter 1");
        assert_eq!(chapters[1].title, "Chapter 2");
    }

    #[test]
    fn splits_hui_chapters_fallback_title() {
        let text = "第一回 引子\n内容。\n第二回 发展\n更多。";
        let chapters = split_chapters(text, None);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "引子");
        assert_eq!(chapters[1].title, "发展");
    }

    #[test]
    fn no_chapter_matches_returns_empty() {
        let text = "只有普通文本\n没有章节标题";
        assert!(split_chapters(text, None).is_empty());
    }

    #[test]
    fn strips_project_gutenberg_trailer() {
        let text = "第一章 内容\n正文。\nProject Gutenberg Literary Archive\n";
        let chapters = split_chapters(text, None);
        assert_eq!(chapters.len(), 1);
        assert!(chapters[0].content.contains("正文"));
        assert!(!chapters[0].content.contains("Project Gutenberg"));
    }

    #[test]
    fn custom_pattern_overrides() {
        // 自定义：每行「### 标题」作为分章点
        let text = "### A\n内容a\n### B\n内容b";
        let chapters = split_chapters(text, Some(r"^###\s+(.*)"));
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "A");
        assert_eq!(chapters[1].title, "B");
    }

    #[test]
    fn splits_chinese_numerals() {
        // CJK 数字（一、二、十、二十等）应被 [零〇○Ｏ０一二三四五六七八九十百千万] 识别
        let text = "第十章 高潮\n内容。\n第二十一章 结局\n结尾。";
        let chapters = split_chapters(text, None);
        assert_eq!(chapters.len(), 2);
    }
}
