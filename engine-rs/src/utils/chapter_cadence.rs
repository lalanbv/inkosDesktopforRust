//! 章节节奏分析。
//!
//! 移植自 `packages/core/src/utils/chapter-cadence.ts`（211 行）。依赖已移植的
//! [`crate::utils::cadence_policy`]。分析最近窗口内的场景/情绪/标题重复压力。
//!
//! ## 移植要点
//! - 标题 token 提取：英文 `[a-z]{4,}`（去停用词）；中文 CJK 段 2-4 字滑动窗口 n-gram
//! - TS `localeCompare` 三级排序末序改用 Rust 默认字节序（仅同计数同长度时触发，
//!   golden 输入设计为有唯一最高计数 token，使 tiebreaker 不影响结果）

use crate::utils::cadence_policy::{
    pressure_thresholds, resolve_cadence_pressure, window_defaults, CadencePressure,
};
use crate::utils::language::WritingLanguage;
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 节奏分析输入行（章节摘要的节奏相关字段）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CadenceSummaryRow {
    pub chapter: u32,
    pub title: String,
    pub mood: String,
    pub chapter_type: String,
}

/// 场景重复压力。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SceneCadencePressure {
    pub pressure: CadencePressure,
    pub repeated_type: String,
    pub streak: u32,
}

/// 情绪高压持续压力。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct MoodCadencePressure {
    pub pressure: CadencePressure,
    pub high_tension_streak: u32,
    pub recent_moods: Vec<String>,
}

/// 标题 token 重复压力。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TitleCadencePressure {
    pub pressure: CadencePressure,
    pub repeated_token: String,
    pub count: u32,
    pub recent_titles: Vec<String>,
}

/// 章节节奏分析结果（三个压力维度，任一可为 None）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ChapterCadenceAnalysis {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_pressure: Option<SceneCadencePressure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mood_pressure: Option<MoodCadencePressure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_pressure: Option<TitleCadencePressure>,
}

/// 默认回看窗口（= CADENCE_WINDOW_DEFAULTS.summaryLookback = 4）。移植 TS `DEFAULT_CHAPTER_CADENCE_WINDOW`。
pub const DEFAULT_CHAPTER_CADENCE_WINDOW: u32 = window_defaults::SUMMARY_LOOKBACK;

/// 高压情绪关键词（中英）。逐字移植 TS `HIGH_TENSION_KEYWORDS`。
pub const HIGH_TENSION_KEYWORDS: &[&str] = &[
    "紧张", "冷硬", "压抑", "逼仄", "肃杀", "沉重", "凝重",
    "冷峻", "压迫", "阴沉", "焦灼", "窒息", "凛冽", "锋利",
    "克制", "危机", "对峙", "绷紧", "僵持", "杀意",
    "tense", "cold", "oppressive", "grim", "ominous", "dark",
    "bleak", "hostile", "threatening", "heavy", "suffocating",
];

const ENGLISH_STOP_WORDS: &[&str] = &[
    "the", "and", "with", "from", "into", "after", "before",
    "over", "under", "this", "that", "your", "their",
];

/// 分析最近窗口内的三类节奏压力。
pub fn analyze_chapter_cadence(rows: &[CadenceSummaryRow], language: WritingLanguage) -> ChapterCadenceAnalysis {
    let lookback = window_defaults::SUMMARY_LOOKBACK as usize;
    // 按章节号升序后取末尾 lookback 个
    let mut sorted: Vec<CadenceSummaryRow> = rows.to_vec();
    sorted.sort_by_key(|r| r.chapter);
    let recent: Vec<CadenceSummaryRow> = sorted.into_iter().rev().take(lookback).collect::<Vec<_>>().into_iter().rev().collect();

    ChapterCadenceAnalysis {
        scene_pressure: analyze_scene_pressure(&recent),
        mood_pressure: analyze_mood_pressure(&recent),
        title_pressure: analyze_title_pressure(&recent, language),
    }
}

/// 判定情绪是否属高压（关键词包含匹配，大小写不敏感）。
pub fn is_high_tension_mood(mood: &str) -> bool {
    let lower = mood.to_lowercase();
    HIGH_TENSION_KEYWORDS.iter().any(|k| lower.contains(k))
}

fn analyze_scene_pressure(rows: &[CadenceSummaryRow]) -> Option<SceneCadencePressure> {
    let types: Vec<String> = rows.iter()
        .map(|r| r.chapter_type.trim().to_string())
        .filter(|v| is_meaningful_value(v))
        .collect();
    if types.len() < 2 {
        return None;
    }
    let repeated_type = types.last()?.clone();
    let mut streak = 0u32;
    for t in types.iter().rev() {
        if t.to_lowercase() != repeated_type.to_lowercase() {
            break;
        }
        streak += 1;
    }
    let pressure = resolve_cadence_pressure(crate::utils::cadence_policy::CadencePressureParams {
        count: streak,
        total: types.len() as u32,
        high_threshold: pressure_thresholds::scene::HIGH_COUNT,
        medium_threshold: pressure_thresholds::scene::MEDIUM_COUNT,
        medium_window_floor: pressure_thresholds::scene::MEDIUM_WINDOW_FLOOR,
    });
    pressure.map(|p| SceneCadencePressure { pressure: p, repeated_type, streak })
}

fn analyze_mood_pressure(rows: &[CadenceSummaryRow]) -> Option<MoodCadencePressure> {
    let moods: Vec<String> = rows.iter()
        .map(|r| r.mood.trim().to_string())
        .filter(|v| is_meaningful_value(v))
        .collect();
    if moods.len() < 2 {
        return None;
    }
    // 从末尾起数高压连续
    let mut recent_moods: Vec<String> = Vec::new();
    let mut streak = 0u32;
    for mood in moods.iter().rev() {
        if !is_high_tension_mood(mood) {
            break;
        }
        recent_moods.insert(0, mood.clone());
        streak += 1;
    }
    let pressure = resolve_cadence_pressure(crate::utils::cadence_policy::CadencePressureParams {
        count: streak,
        total: moods.len() as u32,
        high_threshold: pressure_thresholds::mood::HIGH_COUNT,
        medium_threshold: pressure_thresholds::mood::MEDIUM_COUNT,
        medium_window_floor: pressure_thresholds::mood::MEDIUM_WINDOW_FLOOR,
    });
    pressure.map(|p| MoodCadencePressure { pressure: p, high_tension_streak: streak, recent_moods })
}

fn analyze_title_pressure(rows: &[CadenceSummaryRow], language: WritingLanguage) -> Option<TitleCadencePressure> {
    let titles: Vec<String> = rows.iter()
        .map(|r| r.title.trim().to_string())
        .filter(|v| is_meaningful_value(v))
        .collect();
    if titles.len() < 2 {
        return None;
    }
    // token 计数
    use std::collections::HashMap;
    let mut counts: HashMap<String, u32> = HashMap::new();
    for title in &titles {
        for token in extract_title_tokens(title, language) {
            *counts.entry(token).or_insert(0) += 1;
        }
    }
    // 排序：count desc, length desc, 字节序（替代 localeCompare，仅 tiebreaker）
    let mut entries: Vec<(String, u32)> = counts.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.len().cmp(&a.0.len())).then_with(|| a.0.cmp(&b.0)));
    let repeated = entries.into_iter().find(|(_, c)| *c >= pressure_thresholds::title::MINIMUM_REPEATED_COUNT)?;
    let (repeated_token, count) = repeated;
    let pressure = resolve_cadence_pressure(crate::utils::cadence_policy::CadencePressureParams {
        count,
        total: titles.len() as u32,
        high_threshold: pressure_thresholds::title::HIGH_COUNT,
        medium_threshold: pressure_thresholds::title::MEDIUM_COUNT,
        medium_window_floor: pressure_thresholds::title::MEDIUM_WINDOW_FLOOR,
    });
    pressure.map(|p| TitleCadencePressure { pressure: p, repeated_token, count, recent_titles: titles })
}

fn extract_title_tokens(title: &str, language: WritingLanguage) -> Vec<String> {
    use regex::Regex;
    use std::sync::OnceLock;
    match language {
        WritingLanguage::En => {
            static RE: OnceLock<Regex> = OnceLock::new();
            let re = RE.get_or_init(|| Regex::new(r"(?i)[a-z]{4,}").unwrap());
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for m in re.find_iter(title) {
                let w = m.as_str().to_lowercase();
                if !ENGLISH_STOP_WORDS.iter().any(|s| *s == w) && seen.insert(w.clone()) {
                    out.push(w);
                }
            }
            out
        }
        WritingLanguage::Zh => {
            static RE: OnceLock<Regex> = OnceLock::new();
            let re = RE.get_or_init(|| Regex::new(r"[\u{4e00}-\u{9fff}]{2,}").unwrap());
            let mut tokens = std::collections::HashSet::new();
            for m in re.find_iter(title) {
                let chars: Vec<char> = m.as_str().chars().collect();
                let max_size = std::cmp::min(4, chars.len());
                for size in 2..=max_size {
                    for i in 0..=chars.len().saturating_sub(size) {
                        let ngram: String = chars[i..i + size].iter().collect();
                        tokens.insert(ngram);
                    }
                }
            }
            tokens.into_iter().collect()
        }
    }
}

fn is_meaningful_value(value: &str) -> bool {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    normalized != "none" && normalized != "(none)" && normalized != "无"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ch: u32, title: &str, mood: &str, ctype: &str) -> CadenceSummaryRow {
        CadenceSummaryRow { chapter: ch, title: title.into(), mood: mood.into(), chapter_type: ctype.into() }
    }

    #[test]
    fn high_tension_keyword_detection() {
        assert!(is_high_tension_mood("紧张的对峙"));
        assert!(is_high_tension_mood("Cold and grim"));
        assert!(!is_high_tension_mood("轻松愉快"));
    }

    #[test]
    fn scene_pressure_detects_repeated_type_streak() {
        // 末尾连续 3 个 "战斗" → high（highCount=3），window total ≥ floor
        let rows = vec![row(1, "t1", "平静", "日常"), row(2, "t2", "紧张", "战斗"), row(3, "t3", "压抑", "战斗"), row(4, "t4", "危机", "战斗")];
        let a = analyze_chapter_cadence(&rows, WritingLanguage::Zh);
        assert_eq!(a.scene_pressure.as_ref().unwrap().pressure, CadencePressure::High);
        assert_eq!(a.scene_pressure.unwrap().streak, 3);
    }

    #[test]
    fn mood_pressure_detects_high_tension_streak() {
        let rows = vec![row(1, "t1", "平静", "x"), row(2, "t2", "紧张", "y"), row(3, "t3", "压抑", "z"), row(4, "t4", "肃杀", "w")];
        let a = analyze_chapter_cadence(&rows, WritingLanguage::Zh);
        assert!(a.mood_pressure.is_some());
        assert_eq!(a.mood_pressure.unwrap().high_tension_streak, 3);
    }

    #[test]
    fn title_pressure_detects_repeated_zh_token() {
        // 「系统」在多个标题出现 ≥2 次
        let rows = vec![row(1, "系统觉醒", "x", "y"), row(2, "系统升级", "x", "y"), row(3, "其他", "x", "y"), row(4, "再系统", "x", "y")];
        let a = analyze_chapter_cadence(&rows, WritingLanguage::Zh);
        // 至少检测出某个重复 token（具体 token 由排序决定）
        let tp = a.title_pressure.expect("应有标题压力");
        assert!(tp.count >= 2);
        assert_eq!(tp.pressure, CadencePressure::High); // count=3 ≥ highCount=3，total=4 ≥ floor
    }

    #[test]
    fn no_pressure_when_window_too_short() {
        let rows = vec![row(1, "t", "x", "y")]; // 仅 1 行
        let a = analyze_chapter_cadence(&rows, WritingLanguage::Zh);
        assert!(a.scene_pressure.is_none());
        assert!(a.mood_pressure.is_none());
        assert!(a.title_pressure.is_none());
    }

    #[test]
    fn en_title_token_extraction_skips_stopwords() {
        let tokens = extract_title_tokens("The Gathering Storm", WritingLanguage::En);
        // "the" 是停用词；"gathering"/"storm" ≥4 字母保留
        assert!(tokens.contains(&"gathering".to_string()));
        assert!(tokens.contains(&"storm".to_string()));
        assert!(!tokens.contains(&"the".to_string()));
    }
}
