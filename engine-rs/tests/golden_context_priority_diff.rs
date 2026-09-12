//! 330 号：上下文来源优先级契约共享 golden 差分（G2）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/context-priority-vectors.json`；
//! core 侧 `src/__tests__/golden-context-priority.test.ts` 断言同文件。
//! 三组差分：层级分类（含保护一致性）、组装固化排序、契约形状。

use inkos_engine::models::input_governance::ContextSource;
use inkos_engine::utils::context_assembly::is_protected_context_source;
use inkos_engine::utils::context_source_tier::{
    context_source_priority_contract, context_source_tier, enforce_context_priority_order,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/context-priority-vectors.json");

#[test]
fn tier_classification_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["tier"].as_array().expect("tier array") {
        let source = vector["source"].as_str().expect("source str");
        let tier = context_source_tier(source);
        assert_eq!(
            tier.as_str(),
            vector["expected"].as_str().expect("expected str"),
            "tier vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
        // 保护一致性：tier.protected 必须与 is_protected_context_source 同进同退。
        assert_eq!(
            is_protected_context_source(source),
            tier.is_protected(),
            "protection consistency drifted for '{}' ({})",
            source,
            tier.as_str()
        );
    }
}

#[test]
fn priority_ordering_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["order"].as_array().expect("order array") {
        let input: Vec<ContextSource> = vector["input"]
            .as_array()
            .expect("input array")
            .iter()
            .map(|entry| ContextSource {
                source: entry["source"].as_str().expect("source str").to_string(),
                reason: entry["reason"].as_str().expect("reason str").to_string(),
                excerpt: None,
                rank: entry
                    .get("rank")
                    .and_then(|value| serde_json::from_value(value.clone()).ok()),
            })
            .collect();
        let got: Vec<String> = enforce_context_priority_order(input)
            .iter()
            .map(|entry| entry.source.clone())
            .collect();
        let expected: Vec<String> = vector["expected"]
            .as_array()
            .expect("expected array")
            .iter()
            .map(|v| v.as_str().expect("source str").to_string())
            .collect();
        assert_eq!(
            got, expected,
            "order vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(context_source_priority_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
