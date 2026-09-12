//! 360 号：质量趋势 + 回灌幂等 + 承诺紧迫度共享 golden 差分（R3，二轮 P0）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/quality-trend-vectors.json`；
//! core 侧 `src/__tests__/golden-quality-trend.test.ts` 断言同文件。
//! 三组差分：趋势聚合（升序/缺分不进均值/一位小数）、幂等指纹
//!（已知 FNV 锚定值 + 重放去重）、紧迫度（WNW 映射/置信度/目标章窗）。

use inkos_engine::utils::promise_ledger::{
    resolve_promise_urgency, PromiseHookInput,
};
use inkos_engine::utils::quality_trend::{
    build_quality_trend, dedupe_replay_rows, review_metric_content_hash, ReviewMetricRow,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/quality-trend-vectors.json");

#[test]
fn trend_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["trend"].as_array().expect("trend array") {
        let rows: Vec<ReviewMetricRow> =
            serde_json::from_value(vector["input"].clone()).expect("review metric rows");
        let got = build_quality_trend(&rows);
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "trend vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn replay_hashes_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["replay"].as_array().expect("replay array") {
        let name = vector["name"].as_str().unwrap();
        if let Some(hashes) = vector["hashes"].as_array() {
            for hash in hashes {
                let row: ReviewMetricRow =
                    serde_json::from_value(hash["input"].clone()).expect("hash input row");
                let got = review_metric_content_hash(&row);
                assert_eq!(
                    got,
                    hash["expected"].as_str().unwrap(),
                    "hash vector '{name}' drifted"
                );
            }
            continue;
        }
        let rows: Vec<ReviewMetricRow> =
            serde_json::from_value(vector["input"].clone()).expect("replay rows");
        let got = dedupe_replay_rows(&rows);
        let fresh_chapters: Vec<i64> = got.fresh.iter().map(|row| row.chapter).collect();
        let expected_chapters: Vec<i64> = vector["expected"]["freshChapters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_i64().unwrap())
            .collect();
        assert_eq!(fresh_chapters, expected_chapters, "replay vector '{name}' fresh drifted");
        assert_eq!(
            got.skipped as u64,
            vector["expected"]["skipped"].as_u64().unwrap(),
            "replay vector '{name}' skipped drifted"
        );
    }
}

#[test]
fn urgency_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["urgency"].as_array().expect("urgency array") {
        let hook: PromiseHookInput =
            serde_json::from_value(vector["hook"].clone()).expect("hook input");
        let current_chapter = vector["currentChapter"].as_i64().unwrap();
        let target_chapters = vector["targetChapters"].as_i64();
        let got = resolve_promise_urgency(&hook, current_chapter, target_chapters);
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "urgency vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
