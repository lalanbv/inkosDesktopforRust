//! 书稿质量评估（evaluateBookQuality 移植，47 号）。
//!
//! 移植自 `packages/core/src/utils/book-eval.ts`（152 行）：逐章 AI 痕迹
//! 密度/段落短小统计 + 伏笔回收率 + 重复标题 + 聚合质量分与趋势。
//!
//! ## 移植要点
//! - 所有 `.length` 均为 JS UTF-16 码元语义 → `encode_utf16().count()`
//! - `Math.round(x * 100) / 100` → `(x * 100.0).round() / 100.0`（正值域等价）
//! - parseChapterRange："5"→5..=5；"abc"→1..=∞；"5-x"→5..=∞（parseInt NaN 回退）

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::agents::ai_tells::analyze_ai_tells;
use crate::state::manager::StateManager;
use crate::utils::analytics::{compute_analytics, AnalyticsChapter};
use crate::utils::language::WritingLanguage;

/// 逐章评估。对齐 TS `ChapterEval`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterEval {
    pub number: u32,
    pub title: String,
    pub word_count: u32,
    pub audit_issue_count: usize,
    pub ai_tell_count: usize,
    pub ai_tell_density: f64,
    pub paragraph_warnings: u32,
    pub status: String,
}

/// 趋势点。对齐 TS `qualityTrend` 元素。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityTrendPoint {
    pub chapter: u32,
    pub score: f64,
}

/// 整书评估报告。对齐 TS `BookEval`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BookEval {
    pub book_id: String,
    pub total_chapters: u32,
    pub total_words: u64,
    pub audit_pass_rate: u32,
    pub avg_ai_tell_density: f64,
    pub avg_paragraph_warnings: f64,
    pub hook_resolve_rate: u32,
    pub duplicate_titles: u32,
    pub quality_score: u32,
    pub chapters: Vec<ChapterEval>,
    pub quality_trend: Vec<QualityTrendPoint>,
}

/// 逐章质量分：100 - 审计问题*5 - AI痕迹密度*20 - 段落警告*3，clamp 0-100。
pub fn compute_chapter_eval_score(ch: &ChapterEval) -> f64 {
    let score = 100.0
        - ch.audit_issue_count as f64 * 5.0
        - ch.ai_tell_density * 20.0
        - ch.paragraph_warnings as f64 * 3.0;
    score.clamp(0.0, 100.0)
}

/// 解析章节区间 query（"2-4" / "5" / 缺省全量）。end = ∞ 用 u32::MAX 表示。
pub fn parse_chapter_range(range: Option<&str>) -> (u32, u32) {
    let Some(range) = range else {
        return (1, u32::MAX);
    };
    let mut parts = range.split('-');
    let start_raw = js_parse_int(parts.next().unwrap_or(""));
    let end_raw = match parts.next() {
        Some(second) => js_parse_int(second).unwrap_or(u32::MAX),
        // 无第二段 → end = 原始 start；start 非数字时原始值 NaN → Infinity。
        None => start_raw.unwrap_or(u32::MAX),
    };
    (start_raw.unwrap_or(1), end_raw)
}

/// 对齐 JS `Number.parseInt(value, 10)`：前导数字前缀解析，无数字 → None。
fn js_parse_int(value: &str) -> Option<u32> {
    let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

fn duplicate_title_count(titles: &[String]) -> u32 {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut duplicates = 0;
    for title in titles {
        let norm = title.trim().to_lowercase();
        if seen.contains(&norm) {
            duplicates += 1;
        }
        seen.insert(norm);
    }
    duplicates
}

/// 伏笔回收率：表格行（跳过分隔线与含 hook/伏笔 的表头行），已回收计数。
fn compute_hook_resolve_rate(content: &str) -> u32 {
    if content.trim().is_empty() {
        return 0;
    }
    let mut total_hooks = 0u32;
    let mut resolved_hooks = 0u32;
    for line in content.split('\n') {
        if !is_table_row(line) {
            continue;
        }
        if line.contains("---") || line.contains("hook") || line.contains("Hook") || line.contains("HOOK")
            || line.contains("伏笔")
        {
            continue;
        }
        total_hooks += 1;
        if line.contains("resolved") || line.contains("Resolved") || line.contains("RESOLVED")
            || line.contains("已回收") || line.contains("已解决")
        {
            resolved_hooks += 1;
        }
    }
    if total_hooks > 0 {
        ((resolved_hooks as f64 / total_hooks as f64) * 100.0).round() as u32
    } else {
        0
    }
}

/// TS `/^\|.*\|.*\|/`：行首 `|` 且行内至少还有两个 `|`。
fn is_table_row(line: &str) -> bool {
    let mut chars = line.chars();
    if chars.next() != Some('|') {
        return false;
    }
    let rest: usize = chars.filter(|c| *c == '|').count();
    rest >= 2
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// TS `/\n\s*\n/`：空行分段（行间可有空白）。
fn paragraph_split_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n\s*\n").unwrap())
}

/// 整书质量评估。state 注入 StateManager（loadChapterIndex/bookDir）。
pub async fn evaluate_book_quality(
    state: &StateManager,
    book_id: &str,
    chapters_query: Option<&str>,
) -> Result<BookEval, String> {
    let index = state.load_chapter_index(book_id).await.map_err(|e| e.to_string())?;
    let book_dir = state.book_dir(book_id);
    let chapters_dir = book_dir.join("chapters");
    let (start, end) = parse_chapter_range(chapters_query);
    let filtered: Vec<&crate::models::chapter::ChapterMeta> = index
        .iter()
        .filter(|ch| ch.number >= start && ch.number <= end)
        .collect();
    let chapter_files = list_chapter_files(&chapters_dir).await;

    let mut chapter_evals: Vec<ChapterEval> = Vec::with_capacity(filtered.len());
    for ch in &filtered {
        let padded = format!("{:04}", ch.number);
        let file = chapter_files
            .iter()
            .find(|f| f.starts_with(&padded) && f.ends_with(".md"));
        let content = match file {
            Some(name) => tokio::fs::read_to_string(chapters_dir.join(name)).await.unwrap_or_default(),
            None => String::new(),
        };
        let ai_tells = if content.is_empty() {
            0
        } else {
            analyze_ai_tells(&content, language_of(&content)).issues.len()
        };
        let paragraphs: Vec<String> = paragraph_split_re()
            .split(&content)
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty() && !p.starts_with('#'))
            .collect();
        let short_paras = paragraphs
            .iter()
            .filter(|p| p.encode_utf16().count() < 35)
            .count();
        let paragraph_warnings: u32 =
            if !paragraphs.is_empty() && (short_paras as f64) > (paragraphs.len() as f64) * 0.4 { 1 } else { 0 };
        let ai_tell_density = if !content.is_empty() {
            round2((ai_tells as f64 / content.encode_utf16().count() as f64) * 1000.0)
        } else {
            0.0
        };

        chapter_evals.push(ChapterEval {
            number: ch.number,
            title: ch.title.clone(),
            word_count: ch.word_count,
            audit_issue_count: ch.audit_issues.len(),
            ai_tell_count: ai_tells,
            ai_tell_density,
            paragraph_warnings,
            status: serde_json::to_value(ch.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
        });
    }

    let hooks_content = tokio::fs::read_to_string(book_dir.join("story").join("pending_hooks.md"))
        .await
        .unwrap_or_default();
    let hook_resolve_rate = compute_hook_resolve_rate(&hooks_content);
    let duplicate_titles = duplicate_title_count(&index.iter().map(|ch| ch.title.clone()).collect::<Vec<_>>());

    let analytics_chapters: Vec<AnalyticsChapter> = index
        .iter()
        .map(AnalyticsChapter::from_meta)
        .collect();
    let analytics = compute_analytics(book_id, &analytics_chapters);

    let avg_ai_tell_density = if !chapter_evals.is_empty() {
        chapter_evals.iter().map(|c| c.ai_tell_density).sum::<f64>() / chapter_evals.len() as f64
    } else {
        0.0
    };
    let avg_paragraph_warnings = if !chapter_evals.is_empty() {
        chapter_evals.iter().map(|c| c.paragraph_warnings as f64).sum::<f64>() / chapter_evals.len() as f64
    } else {
        0.0
    };
    let quality_score = (analytics.audit_pass_rate as f64 * 0.3
        + (100.0 - avg_ai_tell_density * 30.0).max(0.0) * 0.25
        + (100.0 - avg_paragraph_warnings * 10.0).max(0.0) * 0.15
        + hook_resolve_rate as f64 * 0.2
        + (100.0 - duplicate_titles as f64 * 20.0).max(0.0) * 0.1)
        .round() as u32;

    let total_words: u64 = filtered.iter().map(|ch| ch.word_count as u64).sum();

    Ok(BookEval {
        book_id: book_id.to_string(),
        total_chapters: filtered.len() as u32,
        total_words,
        audit_pass_rate: analytics.audit_pass_rate,
        avg_ai_tell_density: round2(avg_ai_tell_density),
        avg_paragraph_warnings: round2(avg_paragraph_warnings),
        hook_resolve_rate,
        duplicate_titles,
        quality_score,
        quality_trend: chapter_evals
            .iter()
            .map(|ch| QualityTrendPoint { chapter: ch.number, score: compute_chapter_eval_score(ch) })
            .collect(),
        chapters: chapter_evals,
    })
}

/// 章节正文语言推断：TS `analyzeAITells(content)` 无 language 参数时内部默认 zh。
fn language_of(_content: &str) -> WritingLanguage {
    WritingLanguage::Zh
}

async fn list_chapter_files(chapters_dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(chapters_dir).await else {
        return names;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_range_parsing_matches_ts() {
        assert_eq!(parse_chapter_range(None), (1, u32::MAX));
        assert_eq!(parse_chapter_range(Some("2-4")), (2, 4));
        assert_eq!(parse_chapter_range(Some("5")), (5, 5));
        // "abc"：start NaN→1，end=原始 start NaN→∞。
        assert_eq!(parse_chapter_range(Some("abc")), (1, u32::MAX));
        // "5-x"：end NaN→∞。
        assert_eq!(parse_chapter_range(Some("5-x")), (5, u32::MAX));
    }

    #[test]
    fn duplicate_titles_case_insensitive() {
        let titles = vec!["第一章".to_string(), "第一章 ".to_string(), "第二章".to_string()];
        assert_eq!(duplicate_title_count(&titles), 1);
    }

    #[test]
    fn hook_resolve_rate_skips_header_and_separator() {
        let content = "| 伏笔 | 状态 |\n| --- | --- |\n| 旧信 | 已回收 |\n| 新谜 | 待定 |\n";
        assert_eq!(compute_hook_resolve_rate(content), 50);
        // 表头英文 hook（大小写不敏感）也跳过。
        let en = "| Hook | Status |\n|---|---|\n| letter | resolved |\n";
        assert_eq!(compute_hook_resolve_rate(en), 100);
        assert_eq!(compute_hook_resolve_rate(""), 0);
        assert_eq!(compute_hook_resolve_rate("普通文本"), 0);
    }

    #[test]
    fn chapter_eval_score_formula() {
        let ch = ChapterEval {
            number: 1,
            title: "t".into(),
            word_count: 100,
            audit_issue_count: 2,
            ai_tell_count: 1,
            ai_tell_density: 1.0,
            paragraph_warnings: 1,
            status: "approved".into(),
        };
        // 100 - 10 - 20 - 3 = 67
        assert_eq!(compute_chapter_eval_score(&ch), 67.0);
    }

    #[tokio::test]
    async fn missing_book_returns_empty_report_with_score_80() {
        // TS 怪癖：loadChapterIndex 对缺失书返回 []，evaluateBookQuality 不抛错；
        // qualityScore = 100*0.3 + 100*0.25 + 100*0.15 + 0*0.2 + 100*0.1 = 80。
        let dir = tempfile::tempdir().unwrap();
        let state = StateManager::new(dir.path());
        let report = evaluate_book_quality(&state, "ghost", None).await.unwrap();
        assert_eq!(report.total_chapters, 0);
        assert_eq!(report.quality_score, 80);
        assert_eq!(report.audit_pass_rate, 100);
    }

    #[tokio::test]
    async fn evaluates_persisted_book_like_ts_fixture() {
        // 对齐 TS book-eval.test.ts 的用例（duplicateTitles=1、hookResolveRate=100）。
        let dir = tempfile::tempdir().unwrap();
        let state = StateManager::new(dir.path());
        state
            .save_book_config(
                "demo-book",
                &serde_json::from_str::<crate::models::book::BookConfig>(
                    r#"{"id":"demo-book","title":"Demo Book","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":1200,"language":"zh","createdAt":"","updatedAt":""}"#,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let now = "2026-01-01T00:00:00.000Z";
        let index = vec![
            chapter_meta(1, "第一章", "approved", 1200, vec![], now),
            chapter_meta(2, "第一章", "audit-failed", 900, vec!["pov drift"], now),
        ];
        state.save_chapter_index("demo-book", &index).await.unwrap();
        let book_dir = state.book_dir("demo-book");
        tokio::fs::create_dir_all(book_dir.join("chapters")).await.unwrap();
        tokio::fs::create_dir_all(book_dir.join("story")).await.unwrap();
        tokio::fs::write(
            book_dir.join("chapters").join("0001_第一章.md"),
            "# 第一章\n\n他推开门，发现灯还亮着。",
        )
        .await
        .unwrap();
        tokio::fs::write(
            book_dir.join("chapters").join("0002_第一章.md"),
            "# 第一章\n\n她沉默。\n\n然后转身。",
        )
        .await
        .unwrap();
        tokio::fs::write(
            book_dir.join("story").join("pending_hooks.md"),
            "| 伏笔 | 状态 |\n| --- | --- |\n| 旧信 | 已回收 |\n",
        )
        .await
        .unwrap();

        let report = evaluate_book_quality(&state, "demo-book", None).await.unwrap();
        assert_eq!(report.book_id, "demo-book");
        assert_eq!(report.total_chapters, 2);
        assert_eq!(report.duplicate_titles, 1);
        assert_eq!(report.hook_resolve_rate, 100);
        assert_eq!(report.quality_trend.len(), 2);
        assert_eq!(report.chapters[1].number, 2);
        assert_eq!(report.chapters[1].audit_issue_count, 1);
        assert_eq!(report.chapters[1].status, "audit-failed");

        // 区间过滤：chapters=1 只评第 1 章。
        let only_first = evaluate_book_quality(&state, "demo-book", Some("1")).await.unwrap();
        assert_eq!(only_first.total_chapters, 1);
        // totalWords 取过滤窗口；duplicateTitles 仍按全量 index。
        assert_eq!(only_first.duplicate_titles, 1);
    }

    fn chapter_meta(
        number: u32,
        title: &str,
        status: &str,
        word_count: u32,
        issues: Vec<&str>,
        now: &str,
    ) -> crate::models::chapter::ChapterMeta {
        crate::models::chapter::ChapterMeta {
            number,
            title: title.to_string(),
            status: serde_json::from_value::<crate::models::chapter::ChapterStatus>(
                serde_json::json!(status),
            )
            .unwrap(),
            word_count,
            created_at: now.to_string(),
            updated_at: now.to_string(),
            audit_issues: issues.into_iter().map(str::to_string).collect(),
            length_warnings: Vec::new(),
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        }
    }
}
