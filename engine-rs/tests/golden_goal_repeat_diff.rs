//! 399 号：章节目标防复读门共享向量差分（TS 真源
//! `packages/core/src/utils/goal-repeat-gate.ts`）。
//! 唯一事实源 = `../core/…/golden/goal-repeat-vectors.json`（include_str 同文件）。
//! 三组断言：归一化、相似度、复读判定。
//! 教训预防（336/339/345 三次同款）：向量在 JSON 里的层级就是函数入参层级，
//! 骨架先写 input 层级断言。

use serde_json::Value;

const VECTORS: &str = include_str!(
    "../../packages/core/src/__tests__/golden/goal-repeat-vectors.json"
);

#[test]
fn input_shape_matches_expected_hierarchy() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let first = &vectors["similarity"][0];
    assert!(
        first["input"]["a"].is_string() && first["input"]["b"].is_string(),
        "similarity vectors must nest inputs under .input (a/b)"
    );
    let eval_first = &vectors["evaluate"][0];
    assert!(
        eval_first["input"]["goal"].is_string() && eval_first["input"]["references"].is_array(),
        "evaluate vectors must nest inputs under .input (goal/references)"
    );
}

#[test]
fn normalize_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["normalize"].as_array().expect("normalize array") {
        let got = inkos_engine::utils::goal_repeat_gate::normalize_goal_repeat_text(
            vector["input"].as_str().expect("input str"),
        );
        assert_eq!(
            got,
            vector["expected"].as_str().expect("expected str"),
            "normalize vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn similarity_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["similarity"].as_array().expect("similarity array") {
        let input = &vector["input"];
        let got = inkos_engine::utils::goal_repeat_gate::goal_repeat_similarity(
            input["a"].as_str().expect("a str"),
            input["b"].as_str().expect("b str"),
        );
        let expected = vector["expected"].as_f64().expect("expected f64");
        assert_eq!(
            got, expected,
            "similarity vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn evaluate_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["evaluate"].as_array().expect("evaluate array") {
        let input = &vector["input"];
        let references: Vec<String> = input["references"]
            .as_array()
            .expect("references array")
            .iter()
            .map(|r| r.as_str().expect("ref str").to_string())
            .collect();
        let threshold = input["threshold"].as_f64();
        let got = inkos_engine::utils::goal_repeat_gate::evaluate_goal_repeat(
            input["goal"].as_str().expect("goal str"),
            &references,
            threshold,
        );
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected,
            vector["expected"],
            "evaluate vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
