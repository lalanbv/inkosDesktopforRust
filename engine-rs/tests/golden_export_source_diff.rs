//! 1681 号：导出源一致性校验共享 golden 差分——与 core
//! `__tests__/golden-export.test.ts` 读同一 `export-source-mismatch-vectors.json`
//! 逐例断言。期望值由 TS 生产派生（golden-export.test.ts 自证 expectedMessage
//! 出自 `ChapterExportSourceError` 同一模板）；Rust 消费同一向量：details JSON
//! 串与错误消息逐字对齐，漂移即红即修。

use inkos_engine::interaction::export_artifact::check_export_source_for_test;

const VECTORS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../packages/core/src/__tests__/golden/export-source-mismatch-vectors.json"
);

#[test]
fn export_source_mismatch_matches_shared_golden_vectors() {
    let raw = std::fs::read_to_string(VECTORS_PATH)
        .expect("shared golden vectors must be readable from engine-rs");
    let vectors: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    let cases = vectors
        .get("cases")
        .and_then(serde_json::Value::as_array)
        .expect("cases array");
    assert!(
        cases.len() >= 8,
        "向量面收窄信号： cases={}，四类细节+码点序+混合的锁不应缩水",
        cases.len()
    );
    for case in cases {
        let name = case
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let files: Vec<String> = case
            .get("files")
            .and_then(serde_json::Value::as_array)
            .expect("files array")
            .iter()
            .map(|v| v.as_str().expect("file name string").to_string())
            .collect();
        let indexed: Vec<u32> = case
            .get("indexedNumbers")
            .and_then(serde_json::Value::as_array)
            .expect("indexedNumbers array")
            .iter()
            .map(|v| v.as_u64().expect("chapter number") as u32)
            .collect();
        let expected_details = case.get("expectedDetails");
        let got = check_export_source_for_test(&files, &indexed);
        if expected_details
            .map(serde_json::Value::is_null)
            .unwrap_or(false)
        {
            assert!(
                got.is_empty(),
                "{name}: 校验应通过（四组细节全空），got {got:?}"
            );
            continue;
        }
        let expected_json = case
            .get("expectedJson")
            .and_then(serde_json::Value::as_str)
            .expect("expectedJson string");
        let expected_message = case
            .get("expectedMessage")
            .and_then(serde_json::Value::as_str)
            .expect("expectedMessage string");
        assert_eq!(
            got.details_json(),
            expected_json,
            "{name}: details JSON 串漂移（键序/格式须与 JSON.stringify 逐字同形）"
        );
        assert_eq!(got.message(), expected_message, "{name}: 消息漂移");
        // 字段面与 expectedDetails 逐组对齐（数组顺序含在断言内——码点序锁）。
        let got_value = got.details_value();
        for (key, expected) in expected_details
            .and_then(serde_json::Value::as_object)
            .expect("expectedDetails object")
        {
            let key_json = serde_json::to_string(expected).unwrap();
            let actual_json = serde_json::to_string(
                got_value
                    .get(key)
                    .unwrap_or_else(|| panic!("{name}: details 缺键 {key}")),
            )
            .unwrap();
            assert_eq!(actual_json, key_json, "{name}: details.{key} 漂移");
        }
    }
}
