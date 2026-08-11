//! 章节字数同步（纯内核）。
//!
//! 移植自 `packages/core/src/state/chapter-word-sync.ts`（93 行）的纯逻辑部分。
//! fs I/O（readdir/readFile/saveChapterIndex）由调用方注入——这里只提供可单测的内核：
//! - [`parse_chapter_number_from_filename`]：文件名 → 章节号
//! - [`compute_word_count_changes`]：索引 + 内容快照 → 变更/缺失列表

use crate::models::chapter::ChapterMeta;
use crate::models::length_governance::LengthCountingMode;
use crate::utils::length_metrics::count_chapter_length;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterWordCountChange {
    pub number: u32,
    pub title: String,
    pub previous_word_count: u32,
    pub word_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WordSyncKernelResult {
    pub checked_chapters: usize,
    pub changes: Vec<ChapterWordCountChange>,
    pub missing_chapter_files: Vec<u32>,
    /// 需要写回的新索引（含变更章节的 wordCount/updatedAt 已更新）。
    pub next_index: Vec<ChapterMeta>,
}

fn filename_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\d+)[_-]?.*\.md$").unwrap())
}

/// 从章节文件名解析章节号（`12_title.md` / `12-title.md` / `12.md` → 12）。
pub fn parse_chapter_number_from_filename(name: &str) -> Option<u32> {
    filename_re()
        .captures(name)
        .and_then(|c| c.get(1).unwrap().as_str().parse::<u32>().ok())
}

/// 纯内核：给定索引 + 内容快照（章节号 → markdown 内容），计算字数变更。
/// `now_iso` 为更新时间戳（由调用方传入，避免内核依赖时钟）。
pub fn compute_word_count_changes(
    index: &[ChapterMeta],
    contents: &HashMap<u32, String>,
    counting_mode: LengthCountingMode,
    now_iso: &str,
) -> WordSyncKernelResult {
    let mut changes: Vec<ChapterWordCountChange> = Vec::new();
    let mut missing: Vec<u32> = Vec::new();
    let mut next_index: Vec<ChapterMeta> = Vec::with_capacity(index.len());

    for chapter in index {
        match contents.get(&chapter.number) {
            None => {
                missing.push(chapter.number);
                next_index.push(chapter.clone());
            }
            Some(content) => {
                let word_count = count_chapter_length(content, counting_mode);
                if word_count == chapter.word_count {
                    next_index.push(chapter.clone());
                    continue;
                }
                changes.push(ChapterWordCountChange {
                    number: chapter.number,
                    title: chapter.title.clone(),
                    previous_word_count: chapter.word_count,
                    word_count,
                });
                let mut updated = chapter.clone();
                updated.word_count = word_count;
                updated.updated_at = now_iso.to_string();
                next_index.push(updated);
            }
        }
    }

    WordSyncKernelResult {
        checked_chapters: index.len(),
        changes,
        missing_chapter_files: missing,
        next_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chapter::ChapterStatus;

    fn meta(number: u32, word_count: u32) -> ChapterMeta {
        ChapterMeta {
            number,
            title: format!("ch{}", number),
            status: ChapterStatus::Drafted,
            word_count,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            audit_issues: vec![],
            length_warnings: vec![],
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        }
    }

    #[test]
    fn parse_filename_variants() {
        assert_eq!(parse_chapter_number_from_filename("12_title.md"), Some(12));
        assert_eq!(parse_chapter_number_from_filename("12-title.md"), Some(12));
        assert_eq!(parse_chapter_number_from_filename("12.md"), Some(12));
        assert_eq!(parse_chapter_number_from_filename("intro.md"), None);
        assert_eq!(parse_chapter_number_from_filename("12.txt"), None);
        assert_eq!(parse_chapter_number_from_filename("12_title.txt.md"), Some(12));
    }

    #[test]
    fn detects_word_count_change() {
        let index = vec![meta(1, 100)];
        let mut contents = HashMap::new();
        contents.insert(1u32, "一二三四五六七八九十".into()); // CJK 字符数
        // countingMode CJK → 计 CJK 字符数；"一二三四五六七八九十" = 10
        let result = compute_word_count_changes(
            &index,
            &contents,
            LengthCountingMode::ZhChars,
            "2026-08-11T00:00:00Z",
        );
        assert_eq!(result.changes.len(), 1);
        assert_eq!(result.changes[0].previous_word_count, 100);
        assert_eq!(result.changes[0].word_count, 10);
        assert_eq!(result.next_index[0].word_count, 10);
        assert_eq!(result.next_index[0].updated_at, "2026-08-11T00:00:00Z");
    }

    #[test]
    fn unchanged_when_count_matches() {
        let index = vec![meta(1, 10)];
        let mut contents = HashMap::new();
        contents.insert(1u32, "一二三四五六七八九十".into());
        let result = compute_word_count_changes(&index, &contents, LengthCountingMode::ZhChars, "now");
        assert!(result.changes.is_empty());
        assert_eq!(result.next_index[0].updated_at, "2026-01-01T00:00:00Z"); // 未改
    }

    #[test]
    fn missing_files_tracked() {
        let index = vec![meta(1, 10), meta(2, 20)];
        let mut contents = HashMap::new();
        contents.insert(1u32, "一二三四五六七八九十".into()); // chapter 2 文件缺失
        let result = compute_word_count_changes(&index, &contents, LengthCountingMode::ZhChars, "now");
        assert_eq!(result.missing_chapter_files, vec![2]);
        assert!(result.changes.is_empty());
    }
}
