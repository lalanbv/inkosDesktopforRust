//! 352 号：自动导演共享 golden 差分（G6，Phase C 末件首批）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/director-vectors.json`；
//! core 侧 `src/__tests__/golden-director.test.ts` 断言同文件。
//! 四组差分：方向候选解析（含标题组重做）、运行模式计划、阶段推进决策表、
//! 契约形状。

use inkos_engine::models::director::{
    director_contract, next_director_stage, parse_direction_candidates,
    resolve_director_run_plan, DirectorRunMode, DirectorStage,
};
use serde::Deserialize;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/director-vectors.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CandidatesCase {
    content: String,
    #[serde(default)]
    exclude_titles: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunPlanCase {
    mode: DirectorRunMode,
    #[serde(default)]
    from_chapter: Option<i64>,
    #[serde(default)]
    to_chapter: Option<i64>,
    target_chapters: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StageCase {
    current: DirectorStage,
    mode: DirectorRunMode,
    written_chapters: i64,
    to_chapter: i64,
}

fn stage_expected_of(vector: &Value) -> Value {
    vector["expected"].clone()
}

#[test]
fn candidates_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["candidates"].as_array().expect("candidates array") {
        let case: CandidatesCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let got = parse_direction_candidates(&case.content, &case.exclude_titles);
        let got_value = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_value, vector["expected"],
            "candidates vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn director_prompt_contains_inspiration_and_exclusions() {
    let inspiration = inkos_engine::models::director::InspirationCard {
        premise: "末世废土拾荒少年".into(),
        genre: Some("科幻".into()),
        platform: None,
        tone: None,
        keywords: vec!["废土".into(), "怀表".into()],
    };
    let zh = inkos_engine::models::director::build_direction_candidates_prompt(
        &inspiration,
        3,
        &["旧书".to_string()],
        Some("zh"),
        None,
    );
    assert!(zh.contains("生成 3 套并列的开书方向"));
    assert!(zh.contains("末世废土拾荒少年"));
    assert!(zh.contains("已排除标题（不得复用）：旧书"));
    let en = inkos_engine::models::director::build_direction_candidates_prompt(
        &inspiration,
        2,
        &[],
        Some("en"),
        None,
    );
    assert!(en.contains("Generate 2 alternative book directions"));
    assert!(!en.contains("Excluded titles"));
    // R4/364 号：assetGuidance 可选挂载——追加节，缺省不出现。
    let with_guidance = inkos_engine::models::director::build_direction_candidates_prompt(
        &inspiration,
        2,
        &[],
        Some("zh"),
        Some("## 库资产参考\n- 期待"),
    );
    assert!(with_guidance.contains("## 库资产参考\n- 期待"));
    let without = inkos_engine::models::director::build_direction_candidates_prompt(
        &inspiration,
        2,
        &[],
        Some("zh"),
        None,
    );
    assert!(!without.contains("库资产参考"));
}

#[test]
fn run_plans_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["runPlan"].as_array().expect("runPlan array") {
        let case: RunPlanCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let (mode, from, to, stop_after_directions) = resolve_director_run_plan(
            case.mode,
            case.from_chapter,
            case.to_chapter,
            case.target_chapters,
        );
        let got = serde_json::json!({
            "mode": mode.as_str(),
            "fromChapter": from,
            "toChapter": to,
            "stopAfterDirections": stop_after_directions,
        });
        assert_eq!(
            got, vector["expected"],
            "runPlan vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn stages_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["stages"].as_array().expect("stages array") {
        let case: StageCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let got = next_director_stage(
            case.current,
            case.mode,
            case.written_chapters,
            case.to_chapter,
        );
        assert_eq!(
            got.as_str(),
            stage_expected_of(vector).as_str().unwrap(),
            "stage vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(director_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
