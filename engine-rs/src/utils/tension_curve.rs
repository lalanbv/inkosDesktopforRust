//! R2 张力曲线（358 号，二轮 P0）——settle 产物「TENSION_METRICS」节解析 +
//! 章级张力点列聚合 + 启发式读感告警。
//!
//! TS 真源：`packages/core/src/utils/tension-curve.ts`；golden 唯一事实源：
//! `packages/core/src/__tests__/golden/tension-curve-vectors.json`（差分测试
//! `tests/golden_tension_curve_diff.rs` 读同一文件）。
//!
//! 分数由 settler 管道产出（355 号 §5：曲线是展示层，允许 ±1 抖动）；缺失
//! 分数的章自动跳过，告警只读点列不读正文。移植纪律：解析正则/阈值/文案
//! 与 TS 逐字一致；排序用纯码元比较，禁 locale 感知排序。

use crate::agents::settler_parser::extract_tag;
use serde::Deserialize;

pub const TENSION_METRICS_TAG: &str = "TENSION_METRICS";

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TensionMetrics {
    pub conflict_level: i64,
    pub reveal_level: i64,
}

/// 曲线输入行：`StoredSummary` 的张力子集（结构兼容，可直接从摘要行映射）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TensionRow {
    pub chapter: i64,
    #[serde(default)]
    pub conflict_level: Option<i64>,
    #[serde(default)]
    pub reveal_level: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TensionPoint {
    pub chapter: i64,
    pub conflict_level: i64,
    pub reveal_level: i64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TensionCurve {
    pub points: Vec<TensionPoint>,
    pub scored_chapters: usize,
    pub unscored_chapters: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TensionWarningKind {
    FlatMiddle,
    ClimaxCrowding,
    WeakHookStreak,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TensionWarning {
    pub kind: TensionWarningKind,
    pub chapters: Vec<i64>,
    pub severity: String,
    pub description: String,
    pub suggestion: String,
}

/// 告警默认阈值（对齐 TS `TENSION_WARNING_DEFAULTS`；golden meta.defaults 同源）。
pub const TENSION_WARNING_DEFAULTS: TensionWarningOptions = TensionWarningOptions {
    flat_window: 4,
    flat_max_range: 1,
    middle_start: 0.3,
    middle_end: 0.7,
    climax_min_level: 8,
    climax_window: 0.25,
    climax_min_count: 3,
    weak_level: 3,
    weak_window: 3,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TensionWarningOptions {
    pub flat_window: usize,
    pub flat_max_range: i64,
    pub middle_start: f64,
    pub middle_end: f64,
    pub climax_min_level: i64,
    pub climax_window: f64,
    pub climax_min_count: usize,
    pub weak_level: i64,
    pub weak_window: usize,
}

impl Default for TensionWarningOptions {
    fn default() -> Self {
        TENSION_WARNING_DEFAULTS
    }
}

/// 语言（对齐 TS 的 "zh" | "en" 参数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensionLanguage {
    Zh,
    En,
}

impl From<crate::utils::language::WritingLanguage> for TensionLanguage {
    fn from(value: crate::utils::language::WritingLanguage) -> Self {
        match value {
            crate::utils::language::WritingLanguage::Zh => TensionLanguage::Zh,
            crate::utils::language::WritingLanguage::En => TensionLanguage::En,
        }
    }
}

/// 从 settler 输出解析张力评分节。JSON 形态（可带 ``` 围栏）优先，`key: value`
/// 行式回退（兼容中文冒号）；任一字段缺失/非法 → `None`（整节放弃，不产半截分）。
/// 数值 round 后 clamp 到 1–10。
pub fn parse_tension_metrics(content: &str) -> Option<TensionMetrics> {
    let section = extract_tag(content, TENSION_METRICS_TAG);
    let section = section.trim();
    if section.is_empty() {
        return None;
    }
    let from_json = parse_metrics_from_json(section);
    let from_lines = parse_metrics_from_key_lines(section);
    let conflict_level = from_json
        .as_ref()
        .and_then(|m| m.conflict_level)
        .or_else(|| from_lines.as_ref().and_then(|m| m.conflict_level));
    let reveal_level = from_json
        .as_ref()
        .and_then(|m| m.reveal_level)
        .or_else(|| from_lines.as_ref().and_then(|m| m.reveal_level));
    let (conflict_level, reveal_level) = (conflict_level?, reveal_level?);
    Some(TensionMetrics {
        conflict_level: clamp_level(conflict_level),
        reveal_level: clamp_level(reveal_level),
    })
}

fn parse_metrics_from_json(section: &str) -> Option<PartialMetrics> {
    // 对齐 TS `section.match(/```(?:json)?\s*([\s\S]*?)\s*```/i)`：无锚（节内
    // 围栏前后允许杂文本）；无围栏取整节。
    let raw = fence_re()
        .captures(section)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim())
        .unwrap_or_else(|| section.trim());
    if !raw.starts_with('{') {
        return None;
    }
    // 对齐 TS `raw.replace(/,\s*([}\]])/g, "$1")`（仅尾逗号清理）。
    let cleaned = trailing_comma_re().replace_all(raw, "$1").to_string();
    let value: serde_json::Value = serde_json::from_str(&cleaned).ok()?;
    let obj = value.as_object()?;
    Some(PartialMetrics {
        conflict_level: obj.get("conflictLevel").and_then(as_finite_number),
        reveal_level: obj.get("revealLevel").and_then(as_finite_number),
    })
}

fn fence_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?i)```(?:json)?\s*([\s\S]*?)\s*```").unwrap())
}

fn trailing_comma_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r",\s*([}\]])").unwrap())
}

fn parse_metrics_from_key_lines(section: &str) -> Option<PartialMetrics> {
    let conflict = conflict_level_re().captures(section);
    let reveal = reveal_level_re().captures(section);
    if conflict.is_none() && reveal.is_none() {
        return None;
    }
    Some(PartialMetrics {
        conflict_level: conflict.and_then(|c| c[1].parse::<f64>().ok()),
        reveal_level: reveal.and_then(|c| c[1].parse::<f64>().ok()),
    })
}

fn conflict_level_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r"(?im)^\s*conflictLevel\s*[:：]\s*(\d+(?:\.\d+)?)").unwrap()
    })
}

fn reveal_level_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"(?im)^\s*revealLevel\s*[:：]\s*(\d+(?:\.\d+)?)").unwrap())
}

#[derive(Debug, Clone, Copy, Default)]
struct PartialMetrics {
    conflict_level: Option<f64>,
    reveal_level: Option<f64>,
}

fn as_finite_number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) if !s.trim().is_empty() => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn clamp_level(value: f64) -> i64 {
    value.round().clamp(1.0, 10.0) as i64
}

/// 聚合章级点列：仅收双分齐全的章，按章号升序（稳定排序）。
pub fn build_tension_curve(rows: &[TensionRow]) -> TensionCurve {
    let mut points: Vec<TensionPoint> = rows
        .iter()
        .filter_map(|row| {
            Some(TensionPoint {
                chapter: row.chapter,
                conflict_level: row.conflict_level?,
                reveal_level: row.reveal_level?,
            })
        })
        .collect();
    points.sort_by_key(|p| p.chapter);
    let scored_chapters = points.len();
    let unscored_chapters = rows.len() - scored_chapters;
    TensionCurve {
        points,
        scored_chapters,
        unscored_chapters,
    }
}

/// 启发式读感告警（三条规则，默认阈值见 [`TENSION_WARNING_DEFAULTS`]）：
/// - flat-middle：中段窗口 conflictLevel 极差 ≤ flat_max_range 的最长连续段 ≥ flat_window；
/// - climax-crowding：尾段（最大章号 ×(1−climax_window) 之后）revealLevel 高点数
///   ≥ climax_min_count 且严格大于尾段之外的高点数；
/// - weak-hook-streak：连续 weak_window 个点 revealLevel ≤ weak_level（取最长段）。
/// 点列为空直接返回空数组。
pub fn detect_tension_warnings(
    points: &[TensionPoint],
    language: TensionLanguage,
    options: &TensionWarningOptions,
) -> Vec<TensionWarning> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<TensionPoint> = points.to_vec();
    sorted.sort_by_key(|p| p.chapter);
    let total = sorted.last().map(|p| p.chapter).unwrap_or(0);
    let middle_lo = (total as f64 * options.middle_start).ceil() as i64;
    let middle_hi = (total as f64 * options.middle_end).floor() as i64;
    let tail_lo = (total as f64 * (1.0 - options.climax_window)).floor() as i64 + 1;
    let is_en = language == TensionLanguage::En;
    let mut warnings: Vec<TensionWarning> = Vec::new();

    let middle_points: Vec<TensionPoint> = sorted
        .iter()
        .filter(|p| p.chapter >= middle_lo && p.chapter <= middle_hi)
        .cloned()
        .collect();
    let flat_run = longest_range_window(&middle_points, options.flat_max_range);
    if flat_run.len() >= options.flat_window {
        let first = flat_run.first().unwrap().chapter;
        let last = flat_run.last().unwrap().chapter;
        let count = flat_run.len();
        warnings.push(TensionWarning {
            kind: TensionWarningKind::FlatMiddle,
            chapters: flat_run.iter().map(|p| p.chapter).collect(),
            severity: "warning".to_string(),
            description: if is_en {
                format!("Middle stretch (ch. {first}–{last}) stays flat for {count} chapters (conflict range ≤ {})", options.flat_max_range)
            } else {
                format!("中段（第{first}–{last}章）连续{count}章冲突强度平坦（极差≤{}）", options.flat_max_range)
            },
            suggestion: if is_en {
                "Introduce an escalation or reversal mid-book so tension breathes.".to_string()
            } else {
                "在中段插入一次冲突升级或反转，让张力有起伏。".to_string()
            },
        });
    }

    let tail_highs: Vec<TensionPoint> = sorted
        .iter()
        .filter(|p| p.reveal_level >= options.climax_min_level && p.chapter >= tail_lo)
        .cloned()
        .collect();
    let early_high_count = sorted
        .iter()
        .filter(|p| p.reveal_level >= options.climax_min_level && p.chapter < tail_lo)
        .count();
    if tail_highs.len() >= options.climax_min_count && tail_highs.len() > early_high_count {
        let count = tail_highs.len();
        warnings.push(TensionWarning {
            kind: TensionWarningKind::ClimaxCrowding,
            chapters: tail_highs.iter().map(|p| p.chapter).collect(),
            severity: "warning".to_string(),
            description: if is_en {
                format!("Climactic reveals crowd the final stretch ({count} from ch. {tail_lo} onward vs {early_high_count} earlier)")
            } else {
                format!("高潮揭示堆积尾段（第{tail_lo}章起{count}处，早段仅{early_high_count}处）")
            },
            suggestion: if is_en {
                "Move some climactic reveals earlier to avoid a crowded ending.".to_string()
            } else {
                "把部分高潮揭示前移到中段，避免结尾拥挤。".to_string()
            },
        });
    }

    let weak_run = longest_predicate_run(&sorted, |p| p.reveal_level <= options.weak_level);
    if weak_run.len() >= options.weak_window {
        let first = weak_run.first().unwrap().chapter;
        let last = weak_run.last().unwrap().chapter;
        let count = weak_run.len();
        warnings.push(TensionWarning {
            kind: TensionWarningKind::WeakHookStreak,
            chapters: weak_run.iter().map(|p| p.chapter).collect(),
            severity: "warning".to_string(),
            description: if is_en {
                format!("Chapters {first}–{last} end weak for {count} straight chapters (reveal level ≤ {})", options.weak_level)
            } else {
                format!("第{first}–{last}章连续{count}章章末偏弱（揭示强度≤{}）", options.weak_level)
            },
            suggestion: if is_en {
                "Add an open question or new hook to recent chapter endings.".to_string()
            } else {
                "给最近章节的结尾加一个未解悬念或新钩子。".to_string()
            },
        });
    }

    warnings
}

/// 曲线 + 告警一括聚合（端点/面板共用入口）。
pub fn analyze_tension_curve(
    rows: &[TensionRow],
    language: TensionLanguage,
    options: &TensionWarningOptions,
) -> (TensionCurve, Vec<TensionWarning>) {
    let curve = build_tension_curve(rows);
    let warnings = detect_tension_warnings(&curve.points, language, options);
    (curve, warnings)
}

/// 极差窗口：找最长连续段使段内 conflictLevel 极差 ≤ max_range（双指针）。
fn longest_range_window(points: &[TensionPoint], max_range: i64) -> Vec<TensionPoint> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut best_start = 0usize;
    let mut best_end = 0usize;
    let mut start = 0usize;
    for end in 0..points.len() {
        while start < end && window_range(points, start, end) > max_range {
            start += 1;
        }
        if end - start > best_end - best_start {
            best_start = start;
            best_end = end;
        }
    }
    points[best_start..=best_end].to_vec()
}

fn window_range(points: &[TensionPoint], start: usize, end: usize) -> i64 {
    let mut min = i64::MAX;
    let mut max = i64::MIN;
    for point in &points[start..=end] {
        min = min.min(point.conflict_level);
        max = max.max(point.conflict_level);
    }
    max - min
}

/// 全点满足谓词的最长连续段。
fn longest_predicate_run(
    points: &[TensionPoint],
    predicate: impl Fn(&TensionPoint) -> bool,
) -> Vec<TensionPoint> {
    let mut best_start = 0usize;
    let mut best_end = 0usize;
    let mut best_len = 0usize;
    let mut start: Option<usize> = None;
    for (i, point) in points.iter().enumerate() {
        if predicate(point) {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            let len = i - s;
            if len > best_len {
                best_len = len;
                best_start = s;
                best_end = i - 1;
            }
        }
    }
    if let Some(s) = start {
        let len = points.len() - s;
        if len > best_len {
            best_start = s;
            best_end = points.len() - 1;
        }
    }
    if best_len == 0 {
        return Vec::new();
    }
    points[best_start..=best_end].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(chapter: i64, conflict: i64, reveal: i64) -> TensionPoint {
        TensionPoint {
            chapter,
            conflict_level: conflict,
            reveal_level: reveal,
        }
    }

    #[test]
    fn parses_key_value_and_json_sections() {
        let md = "=== CHAPTER_SUMMARY ===\n正文。\n=== TENSION_METRICS ===\nconflictLevel：7\nrevealLevel：3\n=== UPDATED_HOOKS ===\n- h1";
        let metrics = parse_tension_metrics(md).unwrap();
        assert_eq!(metrics.conflict_level, 7);
        assert_eq!(metrics.reveal_level, 3);

        let json = "=== TENSION_METRICS ===\n```json\n{\"conflictLevel\": 8, \"revealLevel\": 9}\n```";
        let metrics = parse_tension_metrics(json).unwrap();
        assert_eq!(metrics.conflict_level, 8);
        assert_eq!(metrics.reveal_level, 9);
    }

    #[test]
    fn clamps_and_abandons_partial_sections() {
        let clamped = parse_tension_metrics("=== TENSION_METRICS ===\nconflictLevel: 15\nrevealLevel: 0\n").unwrap();
        assert_eq!(clamped.conflict_level, 10);
        assert_eq!(clamped.reveal_level, 1);

        assert!(parse_tension_metrics("=== TENSION_METRICS ===\nconflictLevel: 6\n").is_none());
        assert!(parse_tension_metrics("=== POST_SETTLEMENT ===\n无节。").is_none());
    }

    #[test]
    fn empty_points_raise_nothing() {
        let (curve, warnings) =
            analyze_tension_curve(&[], TensionLanguage::Zh, &TENSION_WARNING_DEFAULTS);
        assert!(curve.points.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn weak_streak_uses_longest_predicate_run() {
        let points = vec![
            point(1, 4, 6),
            point(2, 7, 5),
            point(3, 5, 2),
            point(4, 8, 3),
            point(5, 6, 2),
            point(6, 4, 6),
        ];
        let warnings = detect_tension_warnings(&points, TensionLanguage::Zh, &TENSION_WARNING_DEFAULTS);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, TensionWarningKind::WeakHookStreak);
        assert_eq!(warnings[0].chapters, vec![3, 4, 5]);
    }
}
