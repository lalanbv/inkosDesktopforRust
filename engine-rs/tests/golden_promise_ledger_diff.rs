//! 338 号：承诺账本运营化共享 golden 差分（G10，Phase B 批次一收尾）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/promise-ledger-vectors.json`；
//! core 侧 `src/__tests__/golden-promise-ledger.test.ts` 断言同文件。
//! 五组差分：hookActivity 强度、节奏债告警、连续弱钩段、承诺时间线、契约形状。

use inkos_engine::utils::promise_ledger::{
    build_promise_timeline, detect_pacing_debts, detect_weak_hook_runs, hook_activity_strength,
    parse_expected_chapter, promise_ledger_contract, PromiseHookInput, WeakHookSummaryRow,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/promise-ledger-vectors.json");

fn hooks_from(value: &Value) -> Vec<PromiseHookInput> {
    serde_json::from_value(value.clone()).expect("hook input shape")
}

#[test]
fn strength_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["strength"].as_array().expect("strength array") {
        let got = hook_activity_strength(vector["input"].as_str().expect("input str"));
        assert_eq!(
            got.as_str(),
            vector["expected"].as_str().expect("expected str"),
            "strength vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn pacing_debts_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["pacing"].as_array().expect("pacing array") {
        let input = &vector["input"];
        let got = detect_pacing_debts(
            &hooks_from(&input["hooks"]),
            input["currentChapter"].as_i64().expect("current chapter"),
            Some(input["threshold"].as_i64().expect("threshold")),
        );
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected,
            vector["expected"],
            "pacing vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn weak_hook_runs_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["weakRuns"].as_array().expect("weakRuns array") {
        let input = &vector["input"];
        let summaries: Vec<WeakHookSummaryRow> =
            serde_json::from_value(input["summaries"].clone()).expect("summaries shape");
        let got = detect_weak_hook_runs(
            &summaries,
            Some(input["minRunLength"].as_u64().expect("min run") as usize),
        );
        let expected: Value = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected,
            vector["expected"],
            "weakRuns vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn timeline_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["timeline"].as_array().expect("timeline array") {
        let input = &vector["input"];
        let got = build_promise_timeline(
            &hooks_from(&input["hooks"]),
            input["currentChapter"].as_i64().expect("current chapter"),
        );
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected,
            vector["expected"],
            "timeline vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn parse_expected_chapter_matches_ts() {
    assert_eq!(parse_expected_chapter("第15章"), Some(15));
    assert_eq!(parse_expected_chapter("15"), Some(15));
    assert_eq!(parse_expected_chapter(""), None);
    assert_eq!(parse_expected_chapter("遥遥无期"), None);
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(promise_ledger_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
