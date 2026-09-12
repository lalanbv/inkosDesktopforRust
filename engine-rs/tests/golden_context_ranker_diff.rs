//! 367 号：上下文排序器 + openingHint 共享 golden 差分（R6，二轮 P2）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/context-ranker-vectors.json`；
//! core 侧 `src/__tests__/golden-context-ranker.test.ts` 断言同文件。

use inkos_engine::utils::context_ranker::{
    build_opening_hint, rank_entries_for_composition, rank_within_tier, RankerWeights,
    DEFAULT_RANKER_WEIGHTS,
};
use inkos_engine::utils::language::WritingLanguage;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/context-ranker-vectors.json");

fn weights_of(raw: &Value) -> RankerWeights {
    match raw {
        Value::Null => DEFAULT_RANKER_WEIGHTS,
        obj => RankerWeights {
            recency: obj["recency"].as_f64().unwrap_or(DEFAULT_RANKER_WEIGHTS.recency),
            frequency: obj["frequency"].as_f64().unwrap_or(DEFAULT_RANKER_WEIGHTS.frequency),
            hook_bonus: obj["hookBonus"].as_f64().unwrap_or(DEFAULT_RANKER_WEIGHTS.hook_bonus),
        },
    }
}

#[test]
fn rank_within_tier_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["rankWithinTier"].as_array().expect("rankWithinTier array") {
        let weights = weights_of(&vector["weights"]);
        let entries: Vec<inkos_engine::utils::context_ranker::RankFeatures> =
            serde_json::from_value(vector["input"].clone()).expect("rank features");
        let got = rank_within_tier(&entries, &weights);
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "rankWithinTier vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn composition_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["composition"].as_array().expect("composition array") {
        let weights = weights_of(&vector["weights"]);
        let entries: Vec<inkos_engine::utils::context_ranker::RankFeatures> =
            serde_json::from_value(vector["input"].clone()).expect("rank features");
        let got = rank_entries_for_composition(&entries, &weights);
        let got_sources: Vec<String> = got.iter().map(|ranked| ranked.source.clone()).collect();
        let expected_order: Vec<String> = vector["expectedOrder"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            got_sources, expected_order,
            "composition vector '{}' order drifted",
            vector["name"].as_str().unwrap()
        );
        let got_scores: Vec<f64> = got.iter().map(|ranked| ranked.rank_score).collect();
        let expected_scores: Vec<f64> = vector["expectedScores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_f64().unwrap())
            .collect();
        assert_eq!(
            got_scores, expected_scores,
            "composition vector '{}' scores drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn opening_hint_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["openingHint"].as_array().expect("openingHint array") {
        let language = match vector["language"].as_str().unwrap() {
            "en" => WritingLanguage::En,
            _ => WritingLanguage::Zh,
        };
        let got = build_opening_hint(
            vector["previousEnding"].as_str().unwrap(),
            language,
            vector["maxChars"].as_u64().unwrap_or(200) as usize,
        );
        let expected = &vector["expected"];
        if expected.is_null() {
            assert!(got.is_none(), "hint vector '{}' should be none", vector["name"].as_str().unwrap());
            continue;
        }
        assert_eq!(
            got.as_deref(),
            expected.as_str(),
            "hint vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
