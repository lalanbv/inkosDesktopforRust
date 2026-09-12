//! 357 号：读者体验合同共享 golden 差分（R1）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/reader-experience-vectors.json`；
//! core 侧 `src/__tests__/golden-reader-experience.test.ts` 断言同文件。
//! 五组差分：memo 解析（生成/兼容/截断）、叙述块注入（writer/修稿共用）、
//! 审稿维度 40 激活、planner 提示词合同面、writer 备忘对齐合同面。

use inkos_engine::agents::continuity::build_dimension_list;
use inkos_engine::models::book::FanficMode;
use inkos_engine::agents::planner_prompts::{
    PLANNER_MEMO_SYSTEM_PROMPT, PLANNER_MEMO_SYSTEM_PROMPT_EN,
};
use inkos_engine::agents::writer_prompts::build_chapter_memo_contract;
use inkos_engine::models::genre_profile::GenreProfile;
use inkos_engine::models::input_governance::ChapterMemo;
use inkos_engine::utils::chapter_memo_parser::parse_memo;
use inkos_engine::utils::language::WritingLanguage;
use inkos_engine::utils::narrative_control::render_memo_as_narrative_block;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/reader-experience-vectors.json");

#[test]
fn parse_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["parse"].as_array().expect("parse array") {
        let input = vector["input"].as_str().unwrap();
        let got = parse_memo(input, 12, false).expect("memo should parse");
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected, vector["expected"],
            "parse vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn narrative_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["narrative"].as_array().expect("narrative array") {
        let memo: ChapterMemo =
            serde_json::from_value(vector["memo"].clone()).expect("memo shape");
        let language = match vector["language"].as_str().unwrap() {
            "en" => WritingLanguage::En,
            _ => WritingLanguage::Zh,
        };
        let got = render_memo_as_narrative_block(&memo, None, language);
        assert_eq!(
            got,
            vector["expected"].as_str().unwrap(),
            "narrative vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn dimensions_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["dimensions"].as_array().expect("dimensions array") {
        let gp = GenreProfile {
            name: "都市异能".to_string(),
            id: "urban".to_string(),
            language: "zh".to_string(),
            chapter_types: vec![],
            fatigue_words: vec![],
            numerical_system: false,
            power_scaling: false,
            era_research: false,
            pacing_rule: String::new(),
            satisfaction_types: vec![],
            audit_dimensions: vector["auditDimensions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect(),
        };
        let dims = build_dimension_list(
            &gp,
            None,
            WritingLanguage::Zh,
            false,
            None::<FanficMode>,
            false,
            vector["hasReaderExperience"].as_bool().unwrap(),
        );
        let got: Vec<u32> = dims.iter().map(|d| d.id).collect();
        let expected: Vec<u32> = vector["expectedIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        assert_eq!(
            got, expected,
            "dimensions vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn planner_prompt_carries_contract_spec() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for marker in vectors["plannerPrompt"]["zh"].as_array().unwrap() {
        assert!(
            PLANNER_MEMO_SYSTEM_PROMPT.contains(marker.as_str().unwrap()),
            "zh planner prompt missing marker: {marker}"
        );
    }
    for marker in vectors["plannerPrompt"]["en"].as_array().unwrap() {
        assert!(
            PLANNER_MEMO_SYSTEM_PROMPT_EN.contains(marker.as_str().unwrap()),
            "en planner prompt missing marker: {marker}"
        );
    }
}

#[test]
fn writer_contract_maps_new_section() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let zh = build_chapter_memo_contract(WritingLanguage::Zh, true);
    let en = build_chapter_memo_contract(WritingLanguage::En, true);
    for marker in vectors["writerContract"]["zh"].as_array().unwrap() {
        assert!(
            zh.contains(marker.as_str().unwrap()),
            "zh writer contract missing marker: {marker}"
        );
    }
    for marker in vectors["writerContract"]["en"].as_array().unwrap() {
        assert!(
            en.contains(marker.as_str().unwrap()),
            "en writer contract missing marker: {marker}"
        );
    }
}
