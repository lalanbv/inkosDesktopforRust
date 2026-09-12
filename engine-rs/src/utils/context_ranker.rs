//! R6 上下文排序器 + openingHint（367 号契约层，二轮 P2）。
//!
//! TS 真源：`packages/core/src/utils/context-ranker.ts`；golden 唯一事实源：
//! `packages/core/src/__tests__/golden/context-ranker-vectors.json`（差分测试
//! `tests/golden_context_ranker_diff.rs` 读同一文件）。
//!
//! G2 层间序（330 号）之上做**层内**确定性排序：score = recency×wR +
//! frequency×wF + hookBonus×wH 降序稳定；无特征条目 score=0 保持组装序
//! （既有行为逐字节不变）。openingHint 为上章结尾承接约束块。

use serde::Serialize;

pub const DEFAULT_RANKER_WEIGHTS: RankerWeights = RankerWeights {
    recency: 0.5,
    frequency: 0.3,
    hook_bonus: 0.2,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankerWeights {
    pub recency: f64,
    pub frequency: f64,
    pub hook_bonus: f64,
}

impl Default for RankerWeights {
    fn default() -> Self {
        DEFAULT_RANKER_WEIGHTS
    }
}

/// 可排序条目特征（缺省视为 0——无特征条目 score=0，保持组装序）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankFeatures {
    #[serde(default)]
    pub recency: Option<f64>,
    #[serde(default)]
    pub frequency: Option<f64>,
    #[serde(default)]
    pub hook_bonus: Option<f64>,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedEntry {
    pub source: String,
    pub rank_score: f64,
}

fn clamp01(value: Option<f64>) -> f64 {
    match value {
        Some(value) if value.is_finite() => value.clamp(0.0, 1.0),
        _ => 0.0,
    }
}

fn round4(value: f64) -> f64 {
    (value * 10000.0).round() / 10000.0
}

/// 条目得分（4 位小数；缺特征维度计 0）。
pub fn rank_score(entry: &RankFeatures, weights: &RankerWeights) -> f64 {
    round4(
        clamp01(entry.recency) * weights.recency
            + clamp01(entry.frequency) * weights.frequency
            + clamp01(entry.hook_bonus) * weights.hook_bonus,
    )
}

/// 层内排序：score 降序稳定排序（同分保持输入序；禁 locale 感知排序）。
pub fn rank_within_tier(entries: &[RankFeatures], weights: &RankerWeights) -> Vec<RankedEntry> {
    let mut scored: Vec<(usize, RankedEntry)> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            (
                index,
                RankedEntry {
                    source: entry.source.clone(),
                    rank_score: rank_score(entry, weights),
                },
            )
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.rank_score
            .partial_cmp(&a.1.rank_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.into_iter().map(|(_, ranked)| ranked).collect()
}

/// 组装排序入口：按 contextSourceTier 分层 → 层内 rank → 层间 precedence
/// **降序**拼回（对齐 330 号：事实 100 在前，ephemeral 10 垫底）。
pub fn rank_entries_for_composition(
    entries: &[RankFeatures],
    weights: &RankerWeights,
) -> Vec<RankedEntry> {
    let mut tiers: Vec<(i32, Vec<RankFeatures>)> = Vec::new();
    for entry in entries {
        let tier = crate::utils::context_source_tier::context_source_tier(&entry.source);
        let precedence = tier.precedence();
        match tiers.iter_mut().find(|(tier_precedence, _)| *tier_precedence == precedence) {
            Some((_, items)) => items.push(entry.clone()),
            None => tiers.push((precedence, vec![entry.clone()])),
        }
    }
    tiers.sort_by(|a, b| b.0.cmp(&a.0));
    let mut ordered: Vec<RankedEntry> = Vec::new();
    for (_, items) in &tiers {
        ordered.extend(rank_within_tier(items, weights));
    }
    ordered
}

// ── openingHint：上章结尾承接约束 ──

pub const OPENING_HINT_MAX_CHARS: usize = 200;

/// 上章结尾承接约束块：显式指令 + 结尾摘录（≤max_chars 码元，取尾部）。
/// previousEnding 为空/空白返回 None（首章/无上文零打扰）。
pub fn build_opening_hint(
    previous_ending: &str,
    language: crate::utils::language::WritingLanguage,
    max_chars: usize,
) -> Option<String> {
    let trimmed = previous_ending.trim();
    if trimmed.is_empty() {
        return None;
    }
    let char_count = trimmed.chars().count();
    let clipped: String = if char_count > max_chars {
        let skip = char_count - max_chars;
        format!("…{}", trimmed.chars().skip(skip).collect::<String>())
    } else {
        trimmed.to_string()
    };
    Some(
        if language == crate::utils::language::WritingLanguage::En {
            format!(
                "## Opening constraint (carry over the previous ending)\nThe first paragraphs of this chapter must pick up exactly where the previous chapter ended — same scene, same tension, no reset.\nPrevious ending:\n{clipped}"
            )
        } else {
            format!(
                "## 开头承接约束（上章结尾）\n本章开头必须真实接住上一章结尾——同一场景、同一张力，不得重置或跳过。\n上章结尾：\n{clipped}"
            )
        },
    )
}

/// ContextSource 适配：G2 层间 precedence 之上做层内确定性排序，
/// 返回重排后的原条目（rank 字段随条目走，调用方写 trace/debug）。
pub fn rank_context_sources(
    entries: &[crate::models::input_governance::ContextSource],
    weights: &RankerWeights,
) -> Vec<crate::models::input_governance::ContextSource> {
    let features: Vec<RankFeatures> = entries
        .iter()
        .map(|entry| RankFeatures {
            recency: entry.rank.as_ref().and_then(|rank| rank.recency),
            frequency: entry.rank.as_ref().and_then(|rank| rank.frequency),
            hook_bonus: entry.rank.as_ref().and_then(|rank| rank.hook_bonus),
            source: entry.source.clone(),
        })
        .collect();
    let ranked = rank_entries_for_composition(&features, weights);
    ranked
        .iter()
        .filter_map(|ranked| {
            entries
                .iter()
                .find(|entry| entry.source == ranked.source)
                .cloned()
        })
        .collect()
}
