//! G11/371 号：best-of-N 计划与选优共享 golden 差分。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/best-of-n-vectors.json`。

use inkos_engine::utils::best_of_n::{
    resolve_best_of_n_plan, select_best_candidate, BestOfNConfig,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/best-of-n-vectors.json");

#[test]
fn plan_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["plan"].as_array().expect("plan array") {
        let config: BestOfNConfig =
            serde_json::from_value(vector["config"].clone()).expect("config shape");
        let first_score = vector["firstScore"].as_i64();
        let got = resolve_best_of_n_plan(Some(&config), first_score);
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "plan vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn select_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["select"].as_array().expect("select array") {
        let candidates = vector["candidates"].as_array().unwrap().clone();
        let got = select_best_candidate(&candidates);
        let expected = &vector["expected"];
        assert_eq!(
            got.payload,
            expected["winner"],
            "select vector '{}' winner drifted",
            vector["name"].as_str().unwrap()
        );
        assert_eq!(
            got.index as u64,
            expected["index"].as_u64().unwrap(),
            "select vector '{}' index drifted",
            vector["name"].as_str().unwrap()
        );
        assert_eq!(
            got.score,
            expected["score"].as_i64(),
            "select vector '{}' score drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
