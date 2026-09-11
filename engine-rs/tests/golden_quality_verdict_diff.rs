//! 336 号：质量债务账本契约共享 golden 差分（G3，Phase B 批次一）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/quality-verdict-vectors.json`；
//! core 侧 `src/__tests__/golden-quality-verdict.test.ts` 断言同文件。
//! 四组差分：八级判定决策表、每书治理解析、债务状态机、契约形状。

use inkos_engine::models::quality_governance::{
    quality_governance_contract, resolve_governance_policy, resolve_quality_verdict,
    transition_debt_status, DebtAction, DebtStatus, GovernanceConfig, GovernancePolicy,
    QualityVerdict, QualityVerdictInput,
};
use serde::Deserialize;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/quality-verdict-vectors.json");

#[test]
fn decision_table_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["verdict"].as_array().expect("verdict array") {
        let input: QualityVerdictInput =
            serde_json::from_value(vector["input"].clone()).expect("verdict input shape");
        let expected = vector["expected"].as_str().expect("expected str");
        let got = resolve_quality_verdict(&input);
        assert_eq!(
            got.as_str(),
            expected,
            "verdict vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
        // 决策表口径与派生语义一致：expected 的记债/继续标志可反查。
        assert_eq!(
            got.creates_debt(),
            matches!(
                got,
                QualityVerdict::PatchableObligationGap
                    | QualityVerdict::DraftObligationUnmet
                    | QualityVerdict::DeferAndContinue
                    | QualityVerdict::ReplanRequired
            )
        );
    }
}

#[test]
fn governance_resolution_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    #[derive(Deserialize)]
    struct PolicyCase {
        #[serde(default)]
        book: Option<GovernanceConfig>,
        #[serde(default)]
        project: Option<GovernanceConfig>,
    }
    #[derive(Deserialize)]
    struct GovernanceExpectation {
        policy: GovernancePolicy,
        #[serde(rename = "maxConsecutiveDebts")]
        max_consecutive_debts: u32,
    }
    for vector in vectors["policy"].as_array().expect("policy array") {
        let case: PolicyCase = serde_json::from_value(vector["input"].clone()).expect("policy case shape");
        let expected: GovernanceExpectation =
            serde_json::from_value(vector["expected"].clone()).expect("policy expected shape");
        let (policy, threshold) = resolve_governance_policy(case.book.as_ref(), case.project.as_ref());
        assert_eq!(
            (policy.as_str(), threshold),
            (expected.policy.as_str(), expected.max_consecutive_debts),
            "policy vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn debt_state_machine_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["transition"].as_array().expect("transition array") {
        let current: DebtStatus =
            serde_json::from_value(vector["current"].clone()).expect("debt status");
        let action: DebtAction =
            serde_json::from_value(vector["action"].clone()).expect("debt action");
        let expected_status: DebtStatus =
            serde_json::from_value(vector["expected"].clone()).expect("expected status");
        let valid = vector["valid"].as_bool().expect("valid bool");
        assert_eq!(
            transition_debt_status(current, action),
            (expected_status, valid),
            "transition vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(quality_governance_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
