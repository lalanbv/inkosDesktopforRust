//! 检测反馈环统计（detection insights）。
//!
//! 移植自 `packages/core/src/agents/detection-insights.ts`（72 行）与
//! `packages/core/src/pipeline/detection-runner.ts` 的 `loadDetectionHistory`
//! （163 行文件的读取面；detect-and-rewrite 循环依赖 detector provider，
//! 随检测 provider 域另行移植）。
//!
//! 数据源 `story/detection_history.json`：每条为一次检测/改写事件。
//! 聚合口径：按章分组（首现序），original = 最小 attempt 的 score、
//! final = 最大 attempt 的 score；均值四舍五入到千分位、passRate 到百分位。

use std::path::Path;

use serde::{Deserialize, Serialize};

/// 单条检测/改写事件。对齐 TS `DetectionHistoryEntry`（脏数据宽容：
/// 字段缺失回默认值；类型不符整文件回空——TS JSON.parse 不校验，
/// 数值脏数据会产出 NaN 统计，此处不复刻、偏差备案）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionHistoryEntry {
    pub chapter_number: u32,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub attempt: u32,
}

/// 章级聚合行。对齐 TS `chapterBreakdown` 元素。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterBreakdown {
    pub chapter_number: u32,
    pub original_score: f64,
    pub final_score: f64,
    pub rewrite_attempts: usize,
}

/// 聚合统计。对齐 TS `DetectionStats`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionStats {
    pub total_detections: usize,
    pub total_rewrites: usize,
    pub avg_original_score: f64,
    pub avg_final_score: f64,
    pub avg_score_reduction: f64,
    pub pass_rate: f64,
    pub chapter_breakdown: Vec<ChapterBreakdown>,
}

/// 聚合检测历史。对齐 TS `analyzeDetectionInsights`。
pub fn analyze_detection_insights(history: &[DetectionHistoryEntry]) -> DetectionStats {
    if history.is_empty() {
        return DetectionStats {
            total_detections: 0,
            total_rewrites: 0,
            avg_original_score: 0.0,
            avg_final_score: 0.0,
            avg_score_reduction: 0.0,
            pass_rate: 0.0,
            chapter_breakdown: Vec::new(),
        };
    }

    let total_detections = history.iter().filter(|h| h.action == "detect").count();
    let total_rewrites = history.iter().filter(|h| h.action == "rewrite").count();

    // 按章分组，保持首次出现顺序（TS Map 迭代序）。
    let mut groups: Vec<(u32, Vec<&DetectionHistoryEntry>)> = Vec::new();
    for entry in history {
        match groups.iter_mut().find(|(number, _)| *number == entry.chapter_number) {
            Some((_, entries)) => entries.push(entry),
            None => groups.push((entry.chapter_number, vec![entry])),
        }
    }

    let mut chapter_breakdown: Vec<ChapterBreakdown> = Vec::with_capacity(groups.len());
    let mut total_original = 0.0;
    let mut total_final = 0.0;
    for (chapter_number, entries) in groups {
        // attempt 升序稳定排序（相等保持原序，对齐 TS Array.sort 稳定性）。
        let mut sorted = entries;
        sorted.sort_by_key(|e| e.attempt);
        let original_score = sorted.first().map(|e| e.score).unwrap_or(0.0);
        let final_score = sorted.last().map(|e| e.score).unwrap_or(original_score);
        let rewrite_attempts = sorted.iter().filter(|e| e.action == "rewrite").count();
        chapter_breakdown.push(ChapterBreakdown {
            chapter_number,
            original_score,
            final_score,
            rewrite_attempts,
        });
        total_original += original_score;
        total_final += final_score;
    }

    let chapter_count = chapter_breakdown.len();
    let avg_original = total_original / chapter_count as f64;
    let avg_final = total_final / chapter_count as f64;
    // 通过口径：final ≤ original（分数下降或无需改写）。
    let passed = chapter_breakdown
        .iter()
        .filter(|c| c.final_score <= c.original_score)
        .count();

    DetectionStats {
        total_detections,
        total_rewrites,
        avg_original_score: round_millis(avg_original),
        avg_final_score: round_millis(avg_final),
        avg_score_reduction: round_millis(avg_original - avg_final),
        pass_rate: round_cents(passed as f64 / chapter_count as f64),
        chapter_breakdown,
    }
}

/// 读取 `story/detection_history.json`；缺失/损坏 → 空（TS catch 语义）。
pub async fn load_detection_history(book_dir: &Path) -> Vec<DetectionHistoryEntry> {
    let path = book_dir.join("story").join("detection_history.json");
    let Ok(raw) = tokio::fs::read_to_string(&path).await else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// `Math.round(x * 1000) / 1000`。
fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// `Math.round(x * 100) / 100`。
fn round_cents(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(chapter: u32, score: f64, action: &str, attempt: u32) -> DetectionHistoryEntry {
        DetectionHistoryEntry {
            chapter_number: chapter,
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
            provider: "test".to_string(),
            score,
            action: action.to_string(),
            attempt,
        }
    }

    #[test]
    fn empty_history_returns_zeroed_stats() {
        let stats = analyze_detection_insights(&[]);
        assert_eq!(stats.total_detections, 0);
        assert_eq!(stats.pass_rate, 0.0);
        assert!(stats.chapter_breakdown.is_empty());
    }

    #[test]
    fn aggregates_by_chapter_in_first_seen_order() {
        // 历史：第 2 章先出现（breakdown 顺序 = 首现序，非章号序）。
        let history = vec![
            entry(2, 0.9, "detect", 0),
            entry(1, 0.8, "detect", 0),
            entry(2, 0.5, "rewrite", 1),
            entry(2, 0.4, "rewrite", 2),
            entry(1, 0.75, "rewrite", 1),
        ];
        let stats = analyze_detection_insights(&history);
        assert_eq!(stats.total_detections, 2);
        assert_eq!(stats.total_rewrites, 3);
        let numbers: Vec<u32> = stats.chapter_breakdown.iter().map(|c| c.chapter_number).collect();
        assert_eq!(numbers, vec![2, 1]);
        let c2 = &stats.chapter_breakdown[0];
        assert_eq!((c2.original_score, c2.final_score, c2.rewrite_attempts), (0.9, 0.4, 2));
        let c1 = &stats.chapter_breakdown[1];
        assert_eq!((c1.original_score, c1.final_score, c1.rewrite_attempts), (0.8, 0.75, 1));
        // 均值：original (0.9+0.8)/2 = 0.85；final (0.4+0.75)/2 = 0.575。
        assert_eq!(stats.avg_original_score, 0.85);
        assert_eq!(stats.avg_final_score, 0.575);
        assert_eq!(stats.avg_score_reduction, 0.275);
        // 两章 final ≤ original → passRate 1。
        assert_eq!(stats.pass_rate, 1.0);
    }

    #[test]
    fn pass_rate_counts_unworsened_chapters_and_rounds_cents() {
        // 3 章：2 章 final ≤ original，1 章变差 → 2/3 = 0.666… → 0.67。
        let history = vec![
            entry(1, 0.5, "detect", 0),
            entry(2, 0.5, "detect", 0),
            entry(3, 0.3, "detect", 0),
            entry(2, 0.7, "rewrite", 1),
        ];
        let stats = analyze_detection_insights(&history);
        assert_eq!(stats.pass_rate, 0.67);
    }

    #[tokio::test]
    async fn load_history_missing_or_corrupt_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        assert!(load_detection_history(&book).await.is_empty());
        tokio::fs::create_dir_all(book.join("story")).await.unwrap();
        tokio::fs::write(book.join("story").join("detection_history.json"), "{oops").await.unwrap();
        assert!(load_detection_history(&book).await.is_empty());
        tokio::fs::write(
            book.join("story").join("detection_history.json"),
            r#"[{"chapterNumber":1,"score":0.8,"action":"detect","attempt":0}]"#,
        )
        .await
        .unwrap();
        let history = load_detection_history(&book).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].chapter_number, 1);
        // 字段缺省：timestamp/provider 空串。
        assert_eq!(history[0].timestamp, "");
    }
}
