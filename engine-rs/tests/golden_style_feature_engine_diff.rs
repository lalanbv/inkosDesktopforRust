//! 339 号：写法引擎资产化共享 golden 差分（G4，Phase B 批次二首项）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/style-feature-engine-vectors.json`；
//! core 侧 `src/__tests__/golden-style-feature-engine.test.ts` 断言同文件。
//! 六组差分：特征池推导、启停组合、guidance 渲染、试写 prompt、专名泄露检测、
//! 契约形状；绑定解析（resolveStyleBinding）语义由 selection+guidance 两组覆盖。

use inkos_engine::models::style_profile::StyleProfile;
use inkos_engine::utils::style_feature_engine::{
    apply_feature_selection, build_trial_write_prompt, compose_style_guidance,
    derive_feature_pool, detect_proper_noun_leak, style_feature_engine_contract,
};
use serde::Deserialize;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/style-feature-engine-vectors.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectionCase {
    enabled_ids: Vec<String>,
    disabled_ids: Vec<String>,
    expected: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuidanceCase {
    enabled_ids: Vec<String>,
    language: String,
    expected: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrialCase {
    guidance: String,
    premise: String,
    scene_brief: String,
    target_chars: u32,
    language: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeakCase {
    content: String,
    protected_names: Vec<String>,
    min_occurrences: u32,
}

fn feature_ids(pool: &[inkos_engine::utils::style_feature_engine::StyleFeature]) -> Vec<Value> {
    pool.iter().map(|f| Value::String(f.id.clone())).collect()
}

#[test]
fn feature_pool_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let profile: StyleProfile = serde_json::from_value(vectors["profile"].clone()).unwrap();
    let pool = derive_feature_pool(&profile);
    assert_eq!(
        feature_ids(&pool),
        vectors["poolIds"].as_array().unwrap().clone(),
        "feature pool drifted"
    );
}

#[test]
fn selection_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let profile: StyleProfile = serde_json::from_value(vectors["profile"].clone()).unwrap();
    let pool = derive_feature_pool(&profile);
    for vector in vectors["selection"].as_array().expect("selection array") {
        let case: SelectionCase = serde_json::from_value(vector.clone()).unwrap();
        let got = apply_feature_selection(&pool, Some(&case.enabled_ids), Some(&case.disabled_ids));
        assert_eq!(
            feature_ids(&got),
            case.expected,
            "selection vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn guidance_rendering_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let profile: StyleProfile = serde_json::from_value(vectors["profile"].clone()).unwrap();
    let pool = derive_feature_pool(&profile);
    for vector in vectors["guidance"].as_array().expect("guidance array") {
        let case: GuidanceCase = serde_json::from_value(vector.clone()).unwrap();
        let enabled = apply_feature_selection(&pool, Some(&case.enabled_ids), Some(&[]));
        assert_eq!(
            compose_style_guidance(&enabled, &case.language, None),
            case.expected,
            "guidance vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn trial_prompts_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["trial"].as_array().expect("trial array") {
        let case: TrialCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let expected = vector["expected"].as_str().unwrap();
        let got = build_trial_write_prompt(
            &case.guidance,
            &case.premise,
            &case.scene_brief,
            case.target_chars,
            &case.language,
        );
        assert_eq!(
            got, expected,
            "trial vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn leak_detection_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["leak"].as_array().expect("leak array") {
        let case: LeakCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let got = detect_proper_noun_leak(&case.content, &case.protected_names, case.min_occurrences);
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected,
            vector["expected"],
            "leak vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(style_feature_engine_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
