//! 章节文档壳（890 号 M3c 下）：`packages/core/src/utils/chapter-document.ts`
//! 移植——标题属文档壳不属于正文（剥壳语义供 reviser 的 body() 归一）。

use crate::utils::chapter_splitter::read_chapter_heading;
use crate::utils::language::WritingLanguage;
use crate::utils::source_text::split_source_lines;

/// TS `chapterDocumentHeading`。
pub fn chapter_document_heading(
    number: u32,
    title: &str,
    language: WritingLanguage,
) -> String {
    if language == WritingLanguage::En {
        format!("# Chapter {number}: {title}")
    } else {
        format!("# 第{number}章 {title}")
    }
}

/// TS `chapterDocumentBody`：连续剥离前导同名/同章号标题行（含空行），
/// 剩余正文原样返回（无壳时返回原文）。break 条件=行空或章号≠number
///（`!line||readChapterHeading(line)?.number!==number` 逐字对齐）。
pub fn chapter_document_body(
    content: &str,
    number: u32,
    title: &str,
    language: WritingLanguage,
) -> String {
    let heading = chapter_document_heading(number, title, language);
    let lines = split_source_lines(content);
    let mut cursor = 0usize;
    let mut body_start = 0usize;
    while cursor < lines.len() {
        while cursor < lines.len() && lines[cursor].trim().is_empty() {
            cursor += 1;
        }
        let line = lines.get(cursor).map(|line| line.trim()).unwrap_or_default();
        let heading_number = read_chapter_heading(line).map(|(heading_number, _, _)| heading_number);
        // 继续 = 非空行且（==heading 或 章号==number）；否则 break。
        let keep_stripping = !line.is_empty()
            && (line == heading || heading_number.map(|value| value == number).unwrap_or(false));
        if !keep_stripping {
            break;
        }
        cursor += 1;
        while cursor < lines.len() && lines[cursor].trim().is_empty() {
            cursor += 1;
        }
        body_start = cursor;
    }
    if body_start > 0 {
        lines[body_start..].join("")
    } else {
        content.to_string()
    }
}

/// TS `renderChapterDocument`。
pub fn render_chapter_document(
    number: u32,
    title: &str,
    content: &str,
    language: WritingLanguage,
) -> String {
    format!(
        "{}\n\n{}",
        chapter_document_heading(number, title, language),
        chapter_document_body(content, number, title, language)
    )
}
