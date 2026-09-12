//! 347 号：语义检索层共享 golden 差分（G1，Phase C 首项首批）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/semantic-retrieval-vectors.json`；
//! core 侧 `src/__tests__/golden-semantic-retrieval.test.ts` 断言同文件。
//! 六组差分：任务驱动查询、余弦相似度、向量 topK（浮点 1e-9 容差）、
//! RRF 混合融合、降级模式、契约形状。

use inkos_engine::utils::semantic_retrieval::{
    build_task_driven_query, cosine_similarity, reciprocal_rank_fusion, resolve_retrieval_mode,
    semantic_retrieval_contract, top_k_by_similarity, ChunkVector, RankedHit, TaskQueryInput,
};
use serde::Deserialize;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/semantic-retrieval-vectors.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TopKCase {
    k: usize,
    #[serde(rename = "queryVector")]
    query_vector: Vec<f64>,
    chunks: Vec<ChunkVector>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RrfCase {
    k: i64,
    semantic: Vec<RankedHit>,
    fts: Vec<RankedHit>,
}

#[test]
fn query_construction_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["query"].as_array().expect("query array") {
        let input: TaskQueryInput = serde_json::from_value(vector["input"].clone()).unwrap();
        assert_eq!(
            build_task_driven_query(&input),
            vector["expected"].as_str().unwrap(),
            "query vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn cosine_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["cosine"].as_array().expect("cosine array") {
        let a: Vec<f64> = serde_json::from_value(vector["a"].clone()).unwrap();
        let b: Vec<f64> = serde_json::from_value(vector["b"].clone()).unwrap();
        let expected = vector["expected"].as_f64().unwrap();
        assert!(
            (cosine_similarity(&a, &b) - expected).abs() < 1e-12,
            "cosine vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn topk_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["topK"].as_array().expect("topK array") {
        let case: TopKCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let got = top_k_by_similarity(&case.query_vector, &case.chunks, case.k);
        let expected = vector["expected"].as_array().unwrap();
        assert_eq!(
            got.len(),
            expected.len(),
            "topK vector '{}' length drifted",
            vector["name"].as_str().unwrap()
        );
        for (i, (hit, exp)) in got.iter().zip(expected.iter()).enumerate() {
            assert_eq!(hit.id, exp["id"].as_str().unwrap(), "topK '{}'#{i} id", vector["name"].as_str().unwrap());
            let exp_sim = exp["similarity"].as_f64().unwrap();
            assert!(
                (hit.similarity - exp_sim).abs() < 1e-9,
                "topK '{}'#{i} similarity drifted",
                vector["name"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn rrf_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["rrf"].as_array().expect("rrf array") {
        let case: RrfCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let fused = reciprocal_rank_fusion(&case.semantic, &case.fts, case.k);
        let order: Vec<String> = fused.iter().map(|hit| hit.id.clone()).collect();
        assert_eq!(
            order,
            vector["expected"]["order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            "rrf vector '{}' order drifted",
            vector["name"].as_str().unwrap()
        );
        for hit in &fused {
            let expected_sources = vector["expected"]["sources"][&hit.id].clone();
            assert_eq!(
                serde_json::to_value(&hit.sources).unwrap(),
                expected_sources,
                "rrf '{}' sources for '{}' drifted",
                vector["name"].as_str().unwrap(),
                hit.id
            );
        }
    }
}

#[test]
fn mode_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["mode"].as_array().expect("mode array") {
        let input = &vector["input"];
        let got = resolve_retrieval_mode(
            input["embeddingAvailable"].as_bool().unwrap(),
            input["embeddingError"].as_bool().unwrap(),
        );
        assert_eq!(
            got,
            vector["expected"].as_str().unwrap(),
            "mode vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(semantic_retrieval_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
