//! R20/389 号：系列正典共享契约共享 golden 差分（四轮 P0）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/series-canon-vectors.json`。

use inkos_engine::utils::entity_codex::CodexMatch;
use inkos_engine::utils::language::WritingLanguage;
use inkos_engine::utils::series_canon::{
    is_valid_series_id, merge_codex_layers, parse_series_canon_file, render_series_codex_block,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/series-canon-vectors.json");

fn language_of(raw: &str) -> WritingLanguage {
    match raw {
        "en" => WritingLanguage::En,
        _ => WritingLanguage::Zh,
    }
}

#[test]
fn series_id_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["seriesId"].as_array().expect("seriesId array") {
        let name = vector["name"].as_str().expect("vector name");
        let series_id = vector["seriesId"].as_str().expect("seriesId input");
        let expected = vector["expected"].as_bool().expect("expected bool");
        assert_eq!(
            is_valid_series_id(series_id),
            expected,
            "seriesId vector '{name}' drifted"
        );
    }
}

#[test]
fn parse_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["parse"].as_array().expect("parse array") {
        let name = vector["name"].as_str().expect("vector name");
        let raw = vector.get("raw").expect("parse raw input");
        let expected = vector.get("expected").expect("parse expected");
        let got = parse_series_canon_file(raw);
        if expected.is_null() {
            assert!(got.is_none(), "parse vector '{name}' should be none");
            continue;
        }
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(got_json, *expected, "parse vector '{name}' drifted");
    }
}

#[test]
fn merge_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["merge"].as_array().expect("merge array") {
        let name = vector["name"].as_str().expect("vector name");
        let book: Vec<inkos_engine::utils::entity_codex::EntityCodexCard> =
            serde_json::from_value(vector["book"].clone()).expect("book cards input");
        let series: Vec<inkos_engine::utils::entity_codex::EntityCodexCard> =
            serde_json::from_value(vector["series"].clone()).expect("series entries input");
        let got = merge_codex_layers(&book, &series);
        let got_json = serde_json::json!({
            "bookNames": got.book.iter().map(|card| card.name.clone()).collect::<Vec<_>>(),
            "series": got.series,
        });
        assert_eq!(got_json, vector["expected"], "merge vector '{name}' drifted");
    }
}

#[test]
fn render_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["render"].as_array().expect("render array") {
        let name = vector["name"].as_str().expect("vector name");
        let matches: Vec<CodexMatch> =
            serde_json::from_value(vector["matches"].clone()).expect("matches input");
        let got = render_series_codex_block(&matches, language_of(vector["language"].as_str().unwrap()));
        let expected = vector.get("expected").expect("render expected");
        if expected.is_null() {
            assert!(got.is_none(), "render vector '{name}' should be none");
        } else {
            assert_eq!(
                got,
                Some(expected.as_str().expect("expected string").to_string()),
                "render vector '{name}' drifted"
            );
        }
    }
}
