//! 767 号：技能包契约公共件共享 golden 差分。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/skill-contract-vectors.json`
//! （TS 侧 external-skill-loader.test.ts 以同一 golden 锁定
//! `SKILL_CONTRACT_VERSION` 与 `parseSkillContractVersion`/`skillContractSatisfied`）。
//! 断言：双栈契约版本常量一致 + min-engine-version 比较向量逐 case 同判。
//! 改向量必须双端同批。
use inkos_engine::skills::external_loader::{
    parse_skill_contract_version, skill_contract_satisfied, SKILL_CONTRACT_VERSION,
};

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/skill-contract-vectors.json");

#[test]
fn golden_skill_contract_version_lock() {
    let vectors: serde_json::Value = serde_json::from_str(VECTORS).expect("vectors json");
    assert_eq!(vectors["version"].as_u64(), Some(1));
    assert_eq!(
        vectors["contractVersion"].as_str(),
        Some(SKILL_CONTRACT_VERSION),
        "双栈契约版本常量必须与 golden 唯一事实源一致"
    );
}

#[test]
fn golden_skill_contract_min_engine_vectors() {
    let vectors: serde_json::Value = serde_json::from_str(VECTORS).expect("vectors json");
    for case in vectors["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("case name");
        let raw = case["minEngineVersion"].as_str().expect("minEngineVersion");
        let parsed = parse_skill_contract_version(raw);
        if case["invalid"].as_bool() == Some(true) {
            assert!(parsed.is_none(), "{name}: 应判非法");
            continue;
        }
        let parsed = parsed.unwrap_or_else(|| panic!("{name}: 应合法可解析"));
        let expected = case["satisfied"].as_bool().unwrap_or_else(|| {
            panic!("{name}: 缺 satisfied 期望（非法 case 须标 invalid:true）")
        });
        assert_eq!(
            skill_contract_satisfied(parsed),
            expected,
            "{name}: 满足性判定与 golden 不一致"
        );
    }
}

#[test]
fn parse_skill_contract_version_rejects_edge_shapes() {
    assert!(parse_skill_contract_version("").is_none());
    assert!(parse_skill_contract_version("2.").is_none());
    assert!(parse_skill_contract_version(".2").is_none());
    assert!(parse_skill_contract_version("2.x").is_none());
    assert!(parse_skill_contract_version("-1.0").is_none());
    assert_eq!(parse_skill_contract_version(" 2.0 "), Some((2, 0)));
    assert_eq!(parse_skill_contract_version("2"), Some((2, 0)));
}
