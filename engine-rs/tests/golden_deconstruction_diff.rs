//! 350 号：拆书工作台共享 golden 差分（G5，Phase C 第二大件首批）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/deconstruction-vectors.json`；
//! core 侧 `src/__tests__/golden-deconstruction.test.ts` 断言同文件。
//! 五组差分：证据索引、四档人物档案、节奏/卖点统计、导出渲染、契约形状。

use inkos_engine::utils::deconstruction::{
    analyze_pacing_stats, build_evidence_index, deconstruction_contract,
    derive_character_dossier, render_deconstruction_export, DeconChapter,
};
use serde::Deserialize;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/deconstruction-vectors.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DossierCase {
    depth: String,
    expected: Value,
}

fn chapters_of(vectors: &Value) -> Vec<DeconChapter> {
    serde_json::from_value(vectors["chapters"].clone()).expect("chapters shape")
}

fn vector_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn vector_i64s(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect()
}

#[test]
fn evidence_index_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let chapters = chapters_of(&vectors);
    let index = build_evidence_index(&chapters);
    let keys: Vec<String> = index
        .by_character
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    assert_eq!(
        keys,
        vector_strings(&vectors["index"]["byCharacterKeys"]),
        "index keys drifted"
    );
    let lin_dong: Vec<i64> = index
        .by_character
        .iter()
        .find(|(name, _)| name == "林动")
        .map(|(_, entries)| entries.iter().map(|entry| entry.chapter).collect())
        .unwrap_or_default();
    assert_eq!(lin_dong, vector_i64s(&vectors["index"]["linDongChapters"]));
    let types: Vec<String> = index
        .chapter_types
        .iter()
        .map(|(_, kind)| kind.clone())
        .collect();
    assert_eq!(
        types,
        vector_strings(&vectors["index"]["chapterTypeSequence"]),
        "chapter type sequence drifted"
    );
}

#[test]
fn dossiers_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let chapters = chapters_of(&vectors);
    let index = build_evidence_index(&chapters);
    for vector in vectors["dossiers"].as_array().expect("dossiers array") {
        let case: DossierCase = serde_json::from_value(vector.clone()).unwrap();
        let got = derive_character_dossier(&index, "林动", case.depth.as_str(), Some(5));
        let got_value = serde_json::to_value(&got).unwrap();
        for (key, value) in case.expected.as_object().unwrap() {
            if key == "evidenceCount" {
                assert_eq!(
                    got.evidence.len(),
                    value.as_u64().unwrap() as usize,
                    "dossier '{}' evidenceCount drifted",
                    vector["name"].as_str().unwrap()
                );
                continue;
            }
            assert_eq!(
                &got_value[key], value,
                "dossier '{}' field '{key}' drifted",
                vector["name"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn pacing_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let chapters = chapters_of(&vectors);
    let got = analyze_pacing_stats(&chapters);
    let got_value = serde_json::to_value(&got).unwrap();
    assert_eq!(
        got_value,
        vectors["pacing"].as_array().unwrap()[0]["expected"],
        "pacing drifted"
    );
}

#[test]
fn export_render_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let chapters = chapters_of(&vectors);
    let index = build_evidence_index(&chapters);
    let dossiers: Vec<_> = ["full", "brief"]
        .iter()
        .map(|depth| derive_character_dossier(&index, "林动", depth, Some(5)))
        .collect();
    let pacing = analyze_pacing_stats(&chapters);
    for vector in vectors["exportRender"].as_array().expect("export array") {
        let text =
            render_deconstruction_export(&dossiers, &pacing, vector["language"].as_str().unwrap());
        for fragment in vector["expectedContains"].as_array().unwrap() {
            assert!(
                text.contains(fragment.as_str().unwrap()),
                "export '{}' missing '{}'",
                vector["name"].as_str().unwrap(),
                fragment.as_str().unwrap()
            );
        }
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(deconstruction_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
