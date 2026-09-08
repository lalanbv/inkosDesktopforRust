//! 216 号：references 纯函数域共享 golden 差分。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/references-vectors.json`；
//! core 侧 `src/__tests__/golden-references.test.ts` 断言同文件。分节 /
//! 正文抽取 / slug anchor 三组向量双端逐一比对。

use serde_json::Value;

const VECTORS: &str = include_str!("../../packages/core/src/__tests__/golden/references-vectors.json");

fn normalize(value: Value) -> Value {
    value
}

#[test]
fn split_reference_sections_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["split"].as_array().expect("split array") {
        let input = &vector["input"];
        let note = input["note"].as_str();
        let uses: Vec<String> = input["uses"]
            .as_array()
            .expect("uses")
            .iter()
            .map(|v| v.as_str().expect("use str").to_string())
            .collect();
        let got = inkos_engine::references::split_reference_sections(
            &inkos_engine::references::SplitSectionsInput {
                material_id: input["materialId"].as_str().unwrap(),
                title: input["title"].as_str().unwrap(),
                uses: &uses,
                note,
                content: input["content"].as_str().unwrap(),
            },
        );
        let got_json: Vec<Value> = got
            .iter()
            .map(|section| normalize(serde_json::to_value(section).expect("serialize")))
            .collect();
        assert_eq!(
            got_json,
            *vector["output"].as_array().expect("output"),
            "split vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn extract_and_slug_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["extract"].as_array().expect("extract array") {
        assert_eq!(
            inkos_engine::references::extract_material_content(vector["input"].as_str().unwrap()),
            vector["output"].as_str().unwrap(),
            "extract vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
    for vector in vectors["slug"].as_array().expect("slug array") {
        assert_eq!(
            inkos_engine::references::slugify_anchor(vector["input"].as_str().unwrap()),
            vector["output"].as_str().unwrap(),
            "slug vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
