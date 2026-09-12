//! R11/378 号：场景节拍契约共享 golden 差分（三轮 P0）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/scene-beats-vectors.json`。

use inkos_engine::utils::scene_beats::{
    build_scene_beats_prompt, build_scene_beats_writer_block, parse_scene_beat_plan,
    SceneBeatPlan, SceneBeatsLanguage,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/scene-beats-vectors.json");

fn language_of(raw: &str) -> SceneBeatsLanguage {
    match raw {
        "en" => SceneBeatsLanguage::En,
        _ => SceneBeatsLanguage::Zh,
    }
}

#[test]
fn parse_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["parse"].as_array().expect("parse array") {
        let got = parse_scene_beat_plan(vector["input"].as_str().unwrap(), vector["chapter"].as_i64().unwrap());
        let expected = &vector["expected"];
        if expected.is_null() {
            assert!(got.is_none(), "parse vector '{}' should be none", vector["name"].as_str().unwrap());
            continue;
        }
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, *expected,
            "parse vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn prompt_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["prompt"].as_array().expect("prompt array") {
        let got = build_scene_beats_prompt(
            vector["goal"].as_str().unwrap(),
            vector["outlineNode"].as_str(),
            vector["sceneCount"].as_u64().unwrap() as usize,
            language_of(vector["language"].as_str().unwrap()),
        );
        for anchor in vector["expectedContains"].as_array().unwrap() {
            assert!(
                got.contains(anchor.as_str().unwrap()),
                "prompt vector '{}' missing anchor: {anchor}",
                vector["name"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn writer_block_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["writerBlock"].as_array().expect("writerBlock array") {
        let plan: SceneBeatPlan =
            serde_json::from_value(vector["plan"].clone()).expect("plan shape");
        let got = build_scene_beats_writer_block(&plan, language_of(vector["language"].as_str().unwrap()));
        assert_eq!(
            got,
            vector["expected"].as_str().unwrap(),
            "writer block vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
