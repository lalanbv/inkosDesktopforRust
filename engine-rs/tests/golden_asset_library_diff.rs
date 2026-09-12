//! 362 号：三库资产生态契约共享 golden 差分（R4 首批，二轮 P1）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/asset-library-vectors.json`；
//! core 侧 `src/__tests__/golden-asset-library.test.ts` 断言同文件。
//! 五组差分：shape 校验、内容指纹（FNV 已知值）、幂等合并、便携包
//!（导出稳定序 + 容错导入）、内置题材种子。

use inkos_engine::utils::asset_library::{
    asset_content_hash, build_asset_library_export, genre_base_seeds, merge_asset_library,
    parse_asset_library_import, sort_assets, validate_library_asset, LibraryAsset,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/asset-library-vectors.json");

fn as_asset(raw: &Value) -> LibraryAsset {
    let result = validate_library_asset(raw);
    result.asset.clone().unwrap_or_else(|| {
        panic!("golden asset should be valid: {:?}", result.errors)
    })
}

fn asset_ids(assets: &[LibraryAsset]) -> Vec<String> {
    assets
        .iter()
        .map(|asset| format!("{}:{}", asset.kind.as_str(), asset.id))
        .collect()
}

#[test]
fn validate_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["validate"].as_array().expect("validate array") {
        let result = validate_library_asset(&vector["input"]);
        let valid = result.errors.is_empty();
        assert_eq!(
            valid,
            vector["valid"].as_bool().unwrap(),
            "validate vector '{}' validity drifted",
            vector["name"].as_str().unwrap()
        );
        if let Some(field) = vector["errorField"].as_str() {
            assert!(
                result.errors.iter().any(|message| message.starts_with(field)),
                "validate vector '{}' error field drifted",
                vector["name"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn hashes_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["hashes"].as_array().expect("hashes array") {
        let asset = as_asset(&vector["input"]);
        let got = asset_content_hash(&asset);
        assert_eq!(
            got,
            vector["expected"].as_str().unwrap(),
            "hash vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn merge_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["merge"].as_array().expect("merge array") {
        let name = vector["name"].as_str().unwrap();
        let existing: Vec<LibraryAsset> = vector["existing"]
            .as_array()
            .unwrap()
            .iter()
            .map(as_asset)
            .collect();
        let incoming: Vec<LibraryAsset> = vector["incoming"]
            .as_array()
            .unwrap()
            .iter()
            .map(as_asset)
            .collect();
        let result = merge_asset_library(&existing, &incoming);
        let expected_ids: Vec<String> = vector["expected"]["mergedIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect();
        assert_eq!(asset_ids(&result.merged), expected_ids, "merge vector '{name}' ids drifted");
        if let Some(bodies) = vector["expected"]["mergedBodies"].as_array() {
            let expected_bodies: Vec<String> = bodies
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect();
            let got_bodies: Vec<String> =
                result.merged.iter().map(|asset| asset.body.clone()).collect();
            assert_eq!(got_bodies, expected_bodies, "merge vector '{name}' bodies drifted");
        }
        assert_eq!(
            (result.skipped, result.overwritten, result.added),
            (
                vector["expected"]["skipped"].as_u64().unwrap() as usize,
                vector["expected"]["overwritten"].as_u64().unwrap() as usize,
                vector["expected"]["added"].as_u64().unwrap() as usize
            ),
            "merge vector '{name}' counters drifted"
        );
    }
}

#[test]
fn package_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let pkg = &vectors["package"];
    let name = pkg["name"].as_str().unwrap();

    let export_input: Vec<LibraryAsset> = pkg["exportInput"]
        .as_array()
        .unwrap()
        .iter()
        .map(as_asset)
        .collect();
    let exported = build_asset_library_export(&export_input);
    let parsed: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        parsed["version"].as_i64(),
        Some(1),
        "package vector '{name}' version drifted"
    );
    let got_ids: Vec<String> = parsed["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|asset| {
            format!(
                "{}:{}",
                asset["kind"].as_str().unwrap(),
                asset["id"].as_str().unwrap()
            )
        })
        .collect();
    let expected_order: Vec<String> = pkg["exportOrder"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got_ids, expected_order, "package vector '{name}' export order drifted");

    // 容错导入：golden 同一资产集合序列化后走 parse（双端同一字符串事实源）。
    let import_payload = serde_json::to_string(&serde_json::json!({
        "version": 1,
        "assets": pkg["importAssets"],
    }))
    .unwrap();
    let imported = parse_asset_library_import(&import_payload);
    assert_eq!(
        imported.assets.len(),
        pkg["importValidCount"].as_u64().unwrap() as usize,
        "package vector '{name}' valid count drifted"
    );
    assert_eq!(
        imported.errors.len(),
        pkg["importInvalidCount"].as_u64().unwrap() as usize,
        "package vector '{name}' invalid count drifted"
    );
    let expected_import_order: Vec<String> = pkg["importOrder"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        asset_ids(&imported.assets),
        expected_import_order,
        "package vector '{name}' import order drifted"
    );

    // 版本不符整包拒绝。
    let wrong = parse_asset_library_import(r#"{"version": 99, "assets": []}"#);
    assert!(wrong.assets.is_empty() && !wrong.errors.is_empty());
}

#[test]
fn seeds_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let seeds = genre_base_seeds();
    let expected_ids: Vec<String> = vectors["seeds"]["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        seeds.len(),
        vectors["seeds"]["count"].as_u64().unwrap() as usize,
        "seeds count drifted"
    );
    let mut got_ids: Vec<String> = seeds.iter().map(|asset| asset.id.clone()).collect();
    got_ids.sort();
    assert_eq!(got_ids, expected_ids, "seeds ids drifted");
    for seed in &seeds {
        assert!(
            validate_library_asset(&serde_json::to_value(seed).unwrap()).asset.is_some(),
            "seed '{}' should be valid",
            seed.id
        );
        assert_eq!(seed.kind, inkos_engine::utils::asset_library::AssetKind::GenreBase);
    }
    // 排序函数回归（seeds 定义序 ≠ 库序）。
    let mut sorted = seeds.clone();
    sort_assets(&mut sorted);
    assert_eq!(asset_ids(&sorted), asset_ids(&sorted));
}
