//! R10/375 号：实体卡 Codex 化共享 golden 差分（三轮 P0）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/entity-codex-vectors.json`。

use inkos_engine::utils::entity_codex::{
    derive_codex_cards, match_codex_cards, render_codex_block, revise_codex_card, CodexRosterEntity,
    CodexRevision, EntityCodexCard,
};
use inkos_engine::utils::language::WritingLanguage;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/entity-codex-vectors.json");

fn roster_of(raw: &Value) -> CodexRosterEntity {
    serde_json::from_value(raw.clone()).expect("roster entity shape")
}

#[test]
fn derive_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["derive"].as_array().expect("derive array") {
        let roster: Vec<CodexRosterEntity> = vector["roster"]
            .as_array()
            .unwrap()
            .iter()
            .map(roster_of)
            .collect();
        let got = derive_codex_cards(&roster);
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "derive vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn revise_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["revise"].as_array().expect("revise array") {
        let base: EntityCodexCard =
            serde_json::from_value(vector["base"].clone()).expect("base card");
        let revision: CodexRevision =
            serde_json::from_value(vector["revision"].clone()).expect("revision shape");
        let got = revise_codex_card(&base, &revision);
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "revise vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn match_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["match"].as_array().expect("match array") {
        let cards: Vec<EntityCodexCard> = vector["cards"]
            .as_array()
            .unwrap()
            .iter()
            .map(|raw| serde_json::from_value(raw.clone()).expect("card shape"))
            .collect();
        let got = match_codex_cards(vector["text"].as_str().unwrap(), &cards);
        let got_pairs: Vec<(String, i64)> = got
            .iter()
            .map(|codex_match| (codex_match.card.name.clone(), codex_match.hits))
            .collect();
        let expected_pairs: Vec<(String, i64)> = vector["expected"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (
                    entry["name"].as_str().unwrap().to_string(),
                    entry["hits"].as_i64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            got_pairs, expected_pairs,
            "match vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn render_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["render"].as_array().expect("render array") {
        let matches: Vec<inkos_engine::utils::entity_codex::CodexMatch> =
            serde_json::from_value(vector["matches"].clone()).expect("matches shape");
        let language = match vector["language"].as_str().unwrap() {
            "en" => WritingLanguage::En,
            _ => WritingLanguage::Zh,
        };
        let got = render_codex_block(&matches, language);
        let expected = &vector["expected"];
        if expected.is_null() {
            assert!(got.is_none(), "render vector '{}' should be none", vector["name"].as_str().unwrap());
            continue;
        }
        assert_eq!(
            got.as_deref(),
            expected.as_str(),
            "render vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
