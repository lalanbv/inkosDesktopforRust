//! G11 best-of-N（371 号契约层，择机项启动）——计划解析 + 候选选优。
//!
//! TS 真源：`packages/core/src/utils/best-of-n.ts`；golden 唯一事实源：
//! `packages/core/src/__tests__/golden/best-of-n-vectors.json`（差分测试
//! `tests/golden_best_of_n_diff.rs` 读同一文件）。
//!
//! 质量优先时的多版选优：首版审查分数（continuity overallScore 0–100）
//! 低于 minScore 时追加生成候选；分数降序稳定选优（缺分最低、同分保序）。

use serde::Deserialize;

pub const BEST_OF_N_DEFAULTS: (usize, i64) = (2, 75);

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BestOfNConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub candidates: Option<usize>,
    #[serde(default)]
    pub min_score: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BestOfNPlan {
    pub enabled: bool,
    /// 首版之后还需生成的候选数（0 = 只用首版）。
    pub extra_candidates: usize,
    /// 候选总数（含首版）。
    pub total_candidates: usize,
    pub min_score: i64,
}

/// 计划解析：未启用 → 0 追加；启用 → 首版分数已知且 ≥ minScore 时 0 追加
/// （首版够好）；分数未知（审查未评分）按需扩展（保守触发）。
pub fn resolve_best_of_n_plan(config: Option<&BestOfNConfig>, first_score: Option<i64>) -> BestOfNPlan {
    let (enabled, candidates, min_score) = match config {
        Some(config) => (
            config.enabled.unwrap_or(false),
            config.candidates.unwrap_or(BEST_OF_N_DEFAULTS.0).clamp(2, 3),
            config.min_score.unwrap_or(BEST_OF_N_DEFAULTS.1),
        ),
        None => (false, BEST_OF_N_DEFAULTS.0, BEST_OF_N_DEFAULTS.1),
    };
    if !enabled {
        return BestOfNPlan {
            enabled: false,
            extra_candidates: 0,
            total_candidates: 1,
            min_score,
        };
    }
    let needs_more = match first_score {
        Some(score) => score < min_score,
        None => true,
    };
    BestOfNPlan {
        enabled: true,
        extra_candidates: if needs_more { candidates - 1 } else { 0 },
        total_candidates: candidates,
        min_score,
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BestCandidateSelection {
    pub payload: serde_json::Value,
    pub index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<i64>,
}

/// 候选选优：score 降序稳定排序（同分保持输入序；缺分排在有分之后）。
/// 空候选 panic（调用方保证至少一个候选）。
pub fn select_best_candidate(candidates: &[serde_json::Value]) -> BestCandidateSelection {
    assert!(
        !candidates.is_empty(),
        "select_best_candidate requires at least one candidate"
    );
    let score_of = |value: &serde_json::Value| -> Option<i64> {
        value.get("score").and_then(serde_json::Value::as_i64)
    };
    let payload_of = |value: &serde_json::Value| {
        value.get("payload").cloned().unwrap_or_else(|| value.clone())
    };
    let mut order: Vec<(usize, &serde_json::Value)> =
        candidates.iter().enumerate().collect();
    order.sort_by(|(index_a, candidate_a), (index_b, candidate_b)| {
        let a = score_of(candidate_a);
        let b = score_of(candidate_b);
        match (a, b) {
            (None, None) => index_a.cmp(index_b),
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(a), Some(b)) => b.cmp(&a).then_with(|| index_a.cmp(index_b)),
        }
    });
    let (index, best) = order[0];
    BestCandidateSelection {
        payload: payload_of(best),
        index,
        score: score_of(best),
    }
}
