//! R3 质量趋势与回灌幂等（360 号，二轮 P0 第三件契约层）。
//!
//! TS 真源：`packages/core/src/utils/quality-trend.ts`；golden 唯一事实源：
//! `packages/core/src/__tests__/golden/quality-trend-vectors.json`（差分测试
//! `tests/golden_quality_trend_diff.rs` 读同一文件）。
//!
//! 移植纪律：FNV-1a 64 逐字节对齐 TS（UTF-8、hex16 零填充）；排序纯码元
//! 比较，禁 locale 感知排序；缺分章不进均值但保留点位。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ReviewMetricRow {
    pub chapter: i64,
    #[serde(default)]
    pub overall_score: Option<i64>,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub critical_count: i64,
    #[serde(default)]
    pub warning_count: i64,
    #[serde(default)]
    pub info_count: i64,
    #[serde(default)]
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct QualityTrendPoint {
    pub chapter: i64,
    /// 缺分序列化为 null（golden 期望侧用 null 表示缺失）。
    pub overall_score: Option<i64>,
    pub passed: bool,
    pub issue_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct QualityTrend {
    pub points: Vec<QualityTrendPoint>,
    pub scored_chapters: usize,
    /// 有分章均值一位小数（×10 取整后 ÷10）；全部缺分为 null。
    pub average_score: Option<f64>,
    pub failing_chapters: Vec<i64>,
}

/// 章级趋势聚合：升序稳定排序，缺分章保留点位但不进均值。
pub fn build_quality_trend(rows: &[ReviewMetricRow]) -> QualityTrend {
    let mut sorted: Vec<&ReviewMetricRow> = rows.iter().collect();
    sorted.sort_by_key(|row| row.chapter);
    let points: Vec<QualityTrendPoint> = sorted
        .iter()
        .map(|row| QualityTrendPoint {
            chapter: row.chapter,
            overall_score: row.overall_score,
            passed: row.passed,
            issue_count: row.critical_count + row.warning_count + row.info_count,
        })
        .collect();
    let scored: Vec<i64> = sorted
        .iter()
        .filter_map(|row| row.overall_score)
        .collect();
    let average_score = if scored.is_empty() {
        None
    } else {
        let sum: i64 = scored.iter().sum();
        Some(((sum as f64) / (scored.len() as f64) * 10.0).round() / 10.0)
    };
    QualityTrend {
        points,
        scored_chapters: scored.len(),
        average_score,
        failing_chapters: sorted
            .iter()
            .filter(|row| !row.passed)
            .map(|row| row.chapter)
            .collect(),
    }
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn fnv1a_hex16(text: &str) -> String {
    let mut hash = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// 回灌幂等指纹：FNV-1a 64（UTF-8，hex16，对齐 348 号 chunkFingerprint 口径）。
/// 输入 = chapter|score|passed|critical|warning|info（**不含 recordedAt**——
/// 重放时间不同但内容相同必须命中同一指纹）；缺分章 score 为 `-`。
pub fn review_metric_content_hash(row: &ReviewMetricRow) -> String {
    let score = row
        .overall_score
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    fnv1a_hex16(&format!(
        "{}|{}|{}|{}|{}|{}",
        row.chapter,
        score,
        i32::from(row.passed),
        row.critical_count,
        row.warning_count,
        row.info_count
    ))
}

/// 幂等重放结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayDedup {
    pub fresh: Vec<ReviewMetricRow>,
    pub skipped: usize,
}

/// 幂等重放：按内容指纹去重（同内容仅首次 fresh；保序；时间戳不同的重复行同判）。
pub fn dedupe_replay_rows(rows: &[ReviewMetricRow]) -> ReplayDedup {
    let mut seen = std::collections::HashSet::new();
    let mut fresh = Vec::new();
    for row in rows {
        let hash = review_metric_content_hash(row);
        if seen.insert(hash) {
            fresh.push(row.clone());
        }
    }
    let skipped = rows.len() - fresh.len();
    ReplayDedup { fresh, skipped }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(chapter: i64, score: Option<i64>, passed: bool) -> ReviewMetricRow {
        ReviewMetricRow {
            chapter,
            overall_score: score,
            passed,
            critical_count: 0,
            warning_count: 0,
            info_count: 0,
            recorded_at: "2026-09-12T00:00:00.000Z".to_string(),
        }
    }

    #[test]
    fn average_is_one_decimal_and_unscored_rows_kept() {
        let rows = vec![row(3, Some(82), true), row(1, Some(70), false), row(2, None, true)];
        let trend = build_quality_trend(&rows);
        assert_eq!(trend.scored_chapters, 2);
        assert_eq!(trend.average_score, Some(76.0));
        assert_eq!(trend.failing_chapters, vec![1]);
        assert_eq!(trend.points.len(), 3);
        // 升序后 ch2（缺分）在 index 1。
        assert_eq!(trend.points[1].overall_score, None);
        assert_eq!(trend.points[1].chapter, 2);
    }

    #[test]
    fn hash_ignores_recorded_at() {
        let mut a = row(7, Some(82), true);
        let b = row(7, Some(82), true);
        a.recorded_at = "2020-01-01T00:00:00.000Z".to_string();
        assert_eq!(review_metric_content_hash(&a), review_metric_content_hash(&b));
    }
}
