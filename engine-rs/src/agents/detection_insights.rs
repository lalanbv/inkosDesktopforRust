//! 检测反馈环（detection-insights）。
//!
//! 移植自 `packages/core/src/agents/detection-insights.ts`（72 行）。
//! 分析 detection_history → 聚合统计（detect/rewrite 计数、平均分、通过率、按章节细分）。

use std::collections::BTreeMap;

use crate::models::detection::{
    ChapterDetectionBreakdown, DetectionAction, DetectionHistoryEntry, DetectionStats,
};

/// 分析检测历史，产出聚合统计。对齐 TS `analyzeDetectionInsights`。
///
/// - detect/rewrite 总数；
/// - 按章节分组，每章取首/末 attempt 的 score 为 original/final；
/// - passRate = finalScore <= originalScore 的章节占比；
/// - 平均分四舍五入到 3 位小数（对齐 TS `Math.round(x * 1000) / 1000`），passRate 2 位。
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

    let total_detections = history.iter().filter(|h| h.action == DetectionAction::Detect).count() as u32;
    let total_rewrites = history.iter().filter(|h| h.action == DetectionAction::Rewrite).count() as u32;

    // 按章节分组（BTreeMap 自动按键升序，稳定输出）。
    let mut chapter_map: BTreeMap<u32, Vec<&DetectionHistoryEntry>> = BTreeMap::new();
    for entry in history {
        chapter_map.entry(entry.chapter_number).or_default().push(entry);
    }

    let mut chapter_breakdown: Vec<ChapterDetectionBreakdown> = Vec::new();
    let mut total_original = 0.0_f64;
    let mut total_final = 0.0_f64;

    for (chapter_number, entries) in &chapter_map {
        let mut sorted: Vec<&&DetectionHistoryEntry> = entries.iter().collect();
        sorted.sort_by_key(|e| e.attempt);
        let original_score = sorted.first().map(|e| e.score).unwrap_or(0.0);
        let final_score = sorted.last().map(|e| e.score).unwrap_or(original_score);
        let rewrite_attempts = entries.iter().filter(|e| e.action == DetectionAction::Rewrite).count() as u32;
        chapter_breakdown.push(ChapterDetectionBreakdown {
            chapter_number: *chapter_number,
            original_score,
            final_score,
            rewrite_attempts,
        });
        total_original += original_score;
        total_final += final_score;
    }

    let chapter_count = chapter_breakdown.len();
    let avg_original = if chapter_count > 0 { total_original / chapter_count as f64 } else { 0.0 };
    let avg_final = if chapter_count > 0 { total_final / chapter_count as f64 } else { 0.0 };
    let passed = chapter_breakdown.iter().filter(|c| c.final_score <= c.original_score).count();
    let pass_rate = if chapter_count > 0 {
        (passed as f64 / chapter_count as f64 * 100.0).round() / 100.0
    } else {
        0.0
    };

    DetectionStats {
        total_detections,
        total_rewrites,
        avg_original_score: round3(avg_original),
        avg_final_score: round3(avg_final),
        avg_score_reduction: round3(avg_original - avg_final),
        pass_rate,
        chapter_breakdown,
    }
}

/// 四舍五入到 3 位小数（对齐 TS `Math.round(x * 1000) / 1000`）。
fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(chapter: u32, attempt: u32, score: f64, action: DetectionAction) -> DetectionHistoryEntry {
        DetectionHistoryEntry {
            chapter_number: chapter,
            attempt,
            score,
            action,
            // 其余字段用合理默认（检测时间戳/文本占位）。
            ..detect_entry_stub()
        }
    }

    fn detect_entry_stub() -> DetectionHistoryEntry {
        DetectionHistoryEntry {
            chapter_number: 0,
            timestamp: String::new(),
            provider: String::new(),
            score: 0.0,
            action: DetectionAction::Detect,
            attempt: 0,
        }
    }

    #[test]
    fn empty_history_returns_zeros() {
        let stats = analyze_detection_insights(&[]);
        assert_eq!(stats.total_detections, 0);
        assert_eq!(stats.pass_rate, 0.0);
        assert!(stats.chapter_breakdown.is_empty());
    }

    #[test]
    fn counts_detect_and_rewrite_actions() {
        let history = vec![
            entry(1, 1, 0.8, DetectionAction::Detect),
            entry(1, 2, 0.5, DetectionAction::Rewrite),
            entry(2, 1, 0.9, DetectionAction::Detect),
        ];
        let stats = analyze_detection_insights(&history);
        assert_eq!(stats.total_detections, 2);
        assert_eq!(stats.total_rewrites, 1);
    }

    #[test]
    fn chapter_breakdown_uses_first_and_last_attempt_scores() {
        let history = vec![
            entry(1, 1, 0.9, DetectionAction::Detect),
            entry(1, 2, 0.6, DetectionAction::Rewrite),
            entry(1, 3, 0.3, DetectionAction::Rewrite),
        ];
        let stats = analyze_detection_insights(&history);
        assert_eq!(stats.chapter_breakdown.len(), 1);
        let c = &stats.chapter_breakdown[0];
        assert_eq!(c.chapter_number, 1);
        assert_eq!(c.original_score, 0.9);
        assert_eq!(c.final_score, 0.3);
        assert_eq!(c.rewrite_attempts, 2);
    }

    #[test]
    fn pass_rate_counts_chapters_where_final_le_original() {
        // ch1: 0.9→0.3（降，pass）；ch2: 0.4→0.4（平，pass）；ch3: 0.3→0.7（升，fail）。
        let history = vec![
            entry(1, 1, 0.9, DetectionAction::Detect),
            entry(1, 2, 0.3, DetectionAction::Rewrite),
            entry(2, 1, 0.4, DetectionAction::Detect),
            entry(3, 1, 0.3, DetectionAction::Detect),
            entry(3, 2, 0.7, DetectionAction::Rewrite),
        ];
        let stats = analyze_detection_insights(&history);
        // passRate = 2/3 ≈ 0.67。
        assert_eq!(stats.pass_rate, 0.67);
    }

    #[test]
    fn averages_rounded_to_3_decimals() {
        let history = vec![
            entry(1, 1, 0.123456, DetectionAction::Detect),
            entry(2, 1, 0.654321, DetectionAction::Detect),
        ];
        let stats = analyze_detection_insights(&history);
        // (0.123456 + 0.654321) / 2 = 0.388888... → round3 = 0.389。
        assert_eq!(stats.avg_original_score, 0.389);
    }
}
