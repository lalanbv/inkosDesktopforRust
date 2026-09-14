//! R12/380 号：向量引擎决策共享 golden 差分（三轮 P0 末件）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/vector-engine-vectors.json`。

use inkos_engine::utils::vector_engine::{resolve_vector_engine, VectorEngineContext};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/vector-engine-vectors.json");

#[test]
fn resolve_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["resolve"].as_array().expect("resolve array") {
        let context: VectorEngineContext =
            serde_json::from_value(vector["input"].clone()).expect("context shape");
        let got = resolve_vector_engine(&context);
        assert_eq!(
            got.as_str(),
            vector["expected"].as_str().unwrap(),
            "resolve vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
