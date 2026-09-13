//! R23/393 号：伏笔类型标注共享 golden 差分（四轮 P1）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/hook-kind-vectors.json`。
//! 三组：别名归一化（7 规范 id + zh/en 别名 + 未知 None）、双语标签
//! （7×2）、HookRecord.kind 零迁移兼容（带/不带/非法）。

use inkos_engine::models::runtime_state::{HookKind, HookRecord};
use inkos_engine::utils::hook_kind::{hook_kind_id, hook_kind_label, normalize_hook_kind};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/hook-kind-vectors.json");

fn kind_by_id(id: &str) -> HookKind {
    match id {
        "promise" => HookKind::Promise,
        "suspense" => HookKind::Suspense,
        "crisis" => HookKind::Crisis,
        "artifact" => HookKind::Artifact,
        "information" => HookKind::Information,
        "emotion" => HookKind::Emotion,
        "worldview" => HookKind::Worldview,
        other => panic!("unknown kind id '{other}'"),
    }
}

#[test]
fn normalize_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["normalize"].as_array().expect("normalize array") {
        let name = vector["name"].as_str().unwrap();
        let raw = vector["raw"].as_str().unwrap();
        let got = normalize_hook_kind(raw).map(hook_kind_id);
        let expected = vector["expected"].as_str();
        assert_eq!(got.as_deref(), expected, "normalize vector '{name}' drifted");
    }
}

#[test]
fn labels_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["labels"].as_array().expect("labels array") {
        let name = vector["name"].as_str().unwrap();
        let kind = kind_by_id(vector["kind"].as_str().unwrap());
        let language = vector["language"].as_str().unwrap();
        assert_eq!(
            hook_kind_label(kind, language),
            vector["expected"].as_str().unwrap(),
            "label vector '{name}' drifted"
        );
    }
}

#[test]
fn record_kind_zero_migration_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["recordRoundtrip"]
        .as_array()
        .expect("roundtrip array")
    {
        let name = vector["name"].as_str().unwrap();
        let parsed: Result<HookRecord, _> =
            serde_json::from_value(vector["input"].clone());
        assert_eq!(
            parsed.is_ok(),
            vector["parses"].as_bool().unwrap(),
            "roundtrip vector '{name}' parse drifted"
        );
        if let Ok(record) = parsed {
            let got = record.kind.map(hook_kind_id);
            let expected = vector["kind"].as_str();
            assert_eq!(got.as_deref(), expected, "roundtrip vector '{name}' kind drifted");
        }
    }
}
