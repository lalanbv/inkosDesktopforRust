//! R49/1644 号：book.json 与 chapters/index.json 键面 parity 守卫（1604 号
//! 机制扩展到 book 面——TS 镜像 `book-config-keyset-parity.test.ts`）。
//!
//! fixtures（packages/core/src/__tests__/fixtures/book-config-keyset.json /
//! chapter-meta-keyset.json）为最小合法 JSON：serde 反序列化→序列化的键集
//! 比对锁定 Rust struct 键面。Rust struct 键面/默认值变化 → 本守卫红 →
//! 人工定性更新 fixture → TS 测试给跟随信号。R49 新增可选键：
//! writing.volumes（卷有序列表）与 ChapterMeta.volume_id（卷归属）——
//! skip_serializing_if = Option::is_none，存量书键缺省零迁移。

use inkos_engine::models::book::BookConfig;
use inkos_engine::models::chapter::ChapterMeta;

const BOOK_FIXTURE: &str =
    include_str!("../../packages/core/src/__tests__/fixtures/book-config-keyset.json");
const CHAPTER_FIXTURE: &str =
    include_str!("../../packages/core/src/__tests__/fixtures/chapter-meta-keyset.json");

fn key_set(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
    if let Some(object) = value.as_object() {
        for (key, child) in object {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            out.push(path.clone());
            key_set(child, &path, out);
        }
    }
    if let Some(array) = value.as_array() {
        if let Some(first) = array.first() {
            key_set(first, &format!("{prefix}[]"), out);
        }
    }
}

/// 键面漂移面：fixture 经 serde round-trip 后 writing.volumes 键集在位
/// （新增键反序列化缺失或序列化被 skip = 键面回退，立即红）。
#[test]
fn book_config_roundtrip_keeps_writing_volumes_keyset() {
    let config: BookConfig = serde_json::from_str(BOOK_FIXTURE).expect("book fixture 可解析");
    let reserialized = serde_json::to_value(&config).expect("序列化");
    let mut keys = Vec::new();
    key_set(&reserialized, "", &mut keys);
    for expected in [
        "writing.volumes",
        "writing.volumes[].id",
        "writing.volumes[].title",
        "writing.volumes[].order",
    ] {
        assert!(
            keys.iter().any(|key| key.contains(expected)),
            "键面漂移：{expected} 不在 round-trip 键集（serde skip/default 变化须人工定性更新 fixture）"
        );
    }
}

/// ChapterMeta.volume_id 键面：fixture 反序列化→序列化后键在位；
/// 缺键行（存量书）序列化不出 volumeId 键（skip_serializing_if 契约）。
#[test]
fn chapter_meta_roundtrip_keeps_volume_id_keyset() {
    let meta: ChapterMeta = serde_json::from_str(CHAPTER_FIXTURE).expect("chapter fixture 可解析");
    assert_eq!(meta.volume_id.as_deref(), Some("vol-1"));
    let reserialized = serde_json::to_value(&meta).expect("序列化");
    assert_eq!(
        reserialized.get("volumeId").and_then(|v| v.as_str()),
        Some("vol-1"),
        "volumeId 键面漂移（skip_serializing_if 契约变化须人工定性）"
    );
    // 存量行（键缺省）→ 不出 volumeId 键（零迁移契约面）。
    let legacy: ChapterMeta = {
        // 直接构造存量形态（无 volumeId 键的 JSON）。
        let value: serde_json::Value = serde_json::from_str(CHAPTER_FIXTURE).unwrap();
        let mut object = value.as_object().expect("对象").clone();
        object.remove("volumeId");
        serde_json::from_value(serde_json::Value::Object(object)).expect("存量行可解析")
    };
    let legacy_json = serde_json::to_value(&legacy).expect("序列化");
    assert!(
        legacy_json.get("volumeId").is_none(),
        "存量行不得序列化出 volumeId 键（skip_serializing_if 契约）"
    );
}
