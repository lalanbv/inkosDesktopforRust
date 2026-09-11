//! 342 号：实体名册+新专名确认卡共享 golden 差分（G7b，批次二末项）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/entity-roster-vectors.json`；
//! core 侧 `src/__tests__/golden-entity-roster.test.ts` 断言同文件。
//! 六组差分：名册解析、渲染、候选提取、三选比对、确认卡渲染、契约形状。

use inkos_engine::utils::entity_roster::{
    apply_roster_confirmation, entity_roster_contract, extract_character_candidates,
    parse_entity_roster, render_entity_roster, render_roster_confirmation_card,
    resolve_roster_candidates, RosterEntity,
};
use serde::Deserialize;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/entity-roster-vectors.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CandidatesCase {
    summaries: Vec<(i64, String)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveCase {
    candidates: Vec<String>,
    roster: Vec<RosterEntity>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CardRenderCase {
    cards: Vec<Value>,
    language: String,
}

// RosterCandidateCard.action/confidence 为 &'static str（不可反序列化）——
// 差分输入经本地全 String 中转结构。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CardInput {
    candidate: String,
    action: String,
    target_name: Option<String>,
    confidence: String,
}

fn to_card(input: CardInput) -> inkos_engine::utils::entity_roster::RosterCandidateCard {
    inkos_engine::utils::entity_roster::RosterCandidateCard {
        candidate: input.candidate,
        action: match input.action.as_str() {
            "typo" => "typo",
            "alias" => "alias",
            _ => "new-entity",
        },
        target_name: input.target_name,
        confidence: match input.confidence.as_str() {
            "high" => "high",
            "medium" => "medium",
            _ => "low",
        },
    }
}

#[test]
fn parse_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["parse"].as_array().expect("parse array") {
        let got = parse_entity_roster(vector["input"].as_str().unwrap());
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected, vector["expected"],
            "parse vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn render_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["render"].as_array().expect("render array") {
        let entries: Vec<RosterEntity> =
            serde_json::from_value(vector["input"].clone()).expect("roster shape");
        assert_eq!(
            render_entity_roster(&entries),
            vector["expected"].as_str().unwrap(),
            "render vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn candidates_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["candidates"].as_array().expect("candidates array") {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SummaryRow {
            chapter: i64,
            characters: String,
        }
        let rows: Vec<SummaryRow> = serde_json::from_value(vector["input"].clone()).unwrap();
        let pairs: Vec<(i64, String)> = rows
            .into_iter()
            .map(|row| (row.chapter, row.characters))
            .collect();
        let got = extract_character_candidates(&pairs);
        assert_eq!(
            got,
            vector["expected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            "candidates vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn resolve_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["resolve"].as_array().expect("resolve array") {
        let case: ResolveCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let got = resolve_roster_candidates(&case.candidates, &case.roster);
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected, vector["expected"],
            "resolve vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn card_render_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["cardRender"].as_array().expect("cardRender array") {
        let case: CardRenderCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let cards: Vec<CardInput> =
            serde_json::from_value(Value::Array(case.cards)).expect("cards shape");
        let cards: Vec<_> = cards.into_iter().map(to_card).collect();
        let got = render_roster_confirmation_card(&cards, &case.language);
        assert_eq!(
            got,
            vector["expected"].as_str().map(String::from),
            "cardRender vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmCase {
    roster: Vec<RosterEntity>,
    confirmation: inkos_engine::utils::entity_roster::RosterConfirmation,
}

#[test]
fn confirmations_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["confirm"].as_array().expect("confirm array") {
        let case: ConfirmCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let expected_roster: Vec<RosterEntity> =
            serde_json::from_value(vector["expectedRoster"].clone()).unwrap();
        let applied_expected = vector["applied"].clone();
        let (roster, applied) =
            apply_roster_confirmation(&case.roster, &case.confirmation);
        assert_eq!(
            serde_json::to_value(&applied).unwrap(),
            applied_expected,
            "confirm applied '{}' drifted",
            vector["name"].as_str().unwrap()
        );
        assert_eq!(
            serde_json::to_value(&roster).unwrap(),
            serde_json::to_value(&expected_roster).unwrap(),
            "confirm roster '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(entity_roster_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
