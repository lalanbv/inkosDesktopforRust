//! 368 号：结构化错误目录共享 golden 差分（R7，二轮 P2）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/author-error-catalog-vectors.json`
//! （行为）+ `data/author-errors.json`（数据）。

use inkos_engine::utils::author_error_catalog::{
    load_author_error_catalog, next_action_for, resolve_author_error, CatalogLanguage,
};
use serde_json::Value;

const VECTORS: &str = include_str!(
    "../../packages/core/src/__tests__/golden/author-error-catalog-vectors.json"
);

fn language_of(raw: &str) -> CatalogLanguage {
    match raw {
        "en" => CatalogLanguage::En,
        _ => CatalogLanguage::Zh,
    }
}

#[test]
fn resolve_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["resolve"].as_array().expect("resolve array") {
        let got = resolve_author_error(
            vector["event"].as_str().unwrap(),
            vector["message"].as_str().unwrap(),
            language_of(vector["language"].as_str().unwrap()),
        );
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "resolve vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn next_actions_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["nextActionFor"].as_array().expect("nextActionFor array") {
        let got = next_action_for(
            vector["severity"].as_str().unwrap(),
            language_of(vector["language"].as_str().unwrap()),
        );
        assert_eq!(
            got,
            vector["expected"].as_str().unwrap(),
            "nextAction severity '{}' drifted",
            vector["severity"].as_str().unwrap()
        );
    }
}

#[test]
fn catalog_data_matches_shared_contract() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let catalog = load_author_error_catalog();
    assert_eq!(
        catalog.errors.len(),
        vectors["catalog"]["count"].as_u64().unwrap() as usize,
        "catalog count drifted"
    );
    let mut got_codes: Vec<String> = catalog.errors.iter().map(|entry| entry.code.clone()).collect();
    got_codes.sort();
    let mut expected_codes: Vec<String> = vectors["catalog"]["codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    expected_codes.sort();
    assert_eq!(got_codes, expected_codes, "catalog codes drifted");
}
