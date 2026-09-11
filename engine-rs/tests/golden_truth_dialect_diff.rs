//! 332 号：防呆写出方言共享 golden 差分（G7c）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/truth-dialect-vectors.json`；
//! core 侧 `src/__tests__/golden-truth-dialect.test.ts` 断言同文件。
//! 五组差分：标量渲染、meta 块、块列表、BOM 剥除、契约形状。

use inkos_engine::utils::truth_dialect::{
    flatten_truth_scalar, is_dangerous_truth_value, quote_truth_value, render_flat_meta_block,
    render_truth_block_list, render_truth_value, strip_utf8_bom, truth_dialect_contract,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/truth-dialect-vectors.json");

#[test]
fn scalar_rendering_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["value"].as_array().expect("value array") {
        let input = vector["input"].as_str().expect("input str");
        assert_eq!(
            render_truth_value(input),
            vector["expected"].as_str().expect("expected str"),
            "value vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn flatten_quote_dangerous_layering_matches_ts() {
    // 分层语义：flatten 平铺 → dangerous 判定 → quote 转义包裹。
    assert_eq!(flatten_truth_scalar("甲\r\n乙"), "甲 乙");
    assert!(is_dangerous_truth_value(""));
    assert!(is_dangerous_truth_value("@某人"));
    assert!(!is_dangerous_truth_value("plain"));
    assert_eq!(quote_truth_value("A\"B"), "\"A\\\"B\"");
}

#[test]
fn meta_blocks_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["meta"].as_array().expect("meta array") {
        let entries: Vec<(&str, &str)> = vector["input"]
            .as_array()
            .expect("entries array")
            .iter()
            .map(|pair| {
                (
                    pair[0].as_str().expect("key str"),
                    pair[1].as_str().expect("value str"),
                )
            })
            .collect();
        assert_eq!(
            render_flat_meta_block(&entries),
            vector["expected"].as_str().expect("expected str"),
            "meta vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn block_lists_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["list"].as_array().expect("list array") {
        let items: Vec<&str> = vector["input"]
            .as_array()
            .expect("items array")
            .iter()
            .map(|v| v.as_str().expect("item str"))
            .collect();
        assert_eq!(
            render_truth_block_list(&items, vector["emptyMarker"].as_str().expect("marker str")),
            vector["expected"].as_str().expect("expected str"),
            "list vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn bom_stripping_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["bom"].as_array().expect("bom array") {
        assert_eq!(
            strip_utf8_bom(vector["input"].as_str().expect("input str")),
            vector["expected"].as_str().expect("expected str"),
            "bom vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(truth_dialect_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
