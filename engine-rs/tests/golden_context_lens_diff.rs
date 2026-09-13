//! R21/391 号：Context Lens 共享契约 golden 差分（四轮 P0）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/context-lens-vectors.json`。
//! 三组用例：全层级装配（无压缩/rank 透传/空 excerpt/未注册来源）、
//! 预算压缩后实况（编译条目+留痕透传）、冷启动空装配。

use inkos_engine::models::input_governance::{ChapterTrace, ContextPackage};
use inkos_engine::utils::context_lens::build_context_lens;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/context-lens-vectors.json");

#[test]
fn lens_projection_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["cases"].as_array().expect("cases array") {
        let name = vector["name"].as_str().expect("case name");
        let package: ContextPackage =
            serde_json::from_value(vector["contextPackage"].clone()).expect("package input");
        let trace: ChapterTrace =
            serde_json::from_value(vector["trace"].clone()).expect("trace input");
        let got = build_context_lens(&package, &trace);
        let got_json = serde_json::to_value(&got).expect("lens serializable");
        assert_eq!(
            got_json,
            vector["expected"],
            "context lens vector '{name}' drifted"
        );
    }
}

#[test]
fn never_trusts_trace_tier_lists_for_protection() {
    // 保护判定必须独立于留痕：清空 trace 分层不影响 lens 的 protected/tier。
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let vector = &vectors["cases"][0];
    let mut trace_value = vector["trace"].clone();
    trace_value["contextTiers"] = serde_json::json!({
        "protectedSources": [],
        "compressibleSources": [],
    });
    let package: ContextPackage =
        serde_json::from_value(vector["contextPackage"].clone()).expect("package input");
    let trace: ChapterTrace = serde_json::from_value(trace_value).expect("trace input");
    let got = build_context_lens(&package, &trace);
    let got_json = serde_json::to_value(&got).unwrap();
    assert_eq!(
        got_json["entries"], vector["expected"]["entries"],
        "tampered trace tiers must not affect lens entries"
    );
}
