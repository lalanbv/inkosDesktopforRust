//! 344 号：续跑建议共享 golden 差分（G9，Phase B 批次三首项）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/resume-advice-vectors.json`；
//! core 侧 `src/__tests__/golden-resume-advice.test.ts` 断言同文件。
//! 三组差分：决策表（五动作全覆盖 + detail 关键语义）、hint 拼接、契约形状。

use inkos_engine::utils::resume_advice::{
    format_resume_hint, resolve_resume_advice, resume_advice_contract, ResumeAdviceInput,
};
use serde::Deserialize;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/resume-advice-vectors.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdviceCase {
    input: ResumeAdviceInput,
    expected: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HintCase {
    input: ResumeAdviceInput,
    #[serde(rename = "expectedContains")]
    expected_contains: Vec<String>,
}

#[test]
fn decision_table_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["advice"].as_array().expect("advice array") {
        let case: AdviceCase = serde_json::from_value(vector.clone()).unwrap();
        let got = resolve_resume_advice(&case.input);
        assert_eq!(
            got.action.as_str(),
            case.expected["action"].as_str().unwrap(),
            "advice vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
        let expected_resume = case.expected["resumeFrom"].as_i64();
        assert_eq!(
            got.resume_from, expected_resume,
            "advice vector '{}' resumeFrom drifted",
            vector["name"].as_str().unwrap()
        );
        assert_eq!(
            got.title,
            case.expected["title"].as_str().unwrap(),
            "advice vector '{}' title drifted",
            vector["name"].as_str().unwrap()
        );
        if let Some(fragment) = case.expected["detailContains"].as_str() {
            assert!(
                got.detail.contains(fragment),
                "advice vector '{}' detail missing '{fragment}': {}",
                vector["name"].as_str().unwrap(),
                got.detail
            );
        }
    }
}

#[test]
fn hints_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["hint"].as_array().expect("hint array") {
        let case: HintCase = serde_json::from_value(vector.clone()).unwrap();
        let got = format_resume_hint(case.input.saved_chapters, case.input.language.as_deref());
        for fragment in &case.expected_contains {
            assert!(
                got.contains(fragment),
                "hint vector '{}' missing '{fragment}': {got}",
                vector["name"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(resume_advice_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
