//! 403 号：运行遥测共享向量差分（TS 真源
//! `packages/core/src/utils/run-log.ts`）。
//! 唯一事实源 = `../core/…/golden/run-log-vectors.json`（include_str 同文件）。
//! 骨架先写 input 层级断言（336/339/345 三次同款教训预防）。

use serde_json::Value;

const VECTORS: &str = include_str!(
    "../../packages/core/src/__tests__/golden/run-log-vectors.json"
);

#[test]
fn input_shape_matches_expected_hierarchy() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let project_first = &vectors["project"][0];
    assert!(
        project_first["input"]["snapshot"]["entries"].is_array(),
        "project vectors must nest snapshot under .input"
    );
    assert!(
        vectors["capacity"]["evictCase"]["appended"].is_array(),
        "capacity.evictCase must carry appended entries"
    );
}

fn entries_from(value: &Value) -> Vec<inkos_engine::utils::run_log::RunLogEntry> {
    serde_json::from_value(value.clone()).expect("entry shape")
}

#[test]
fn capacity_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    assert_eq!(
        inkos_engine::utils::run_log::DEFAULT_RUN_LOG_CAPACITY,
        vectors["capacity"]["default"].as_u64().expect("default") as usize,
    );
}

#[test]
fn append_evicts_oldest_per_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let evict_case = &vectors["capacity"]["evictCase"];
    let capacity = evict_case["capacity"].as_u64().expect("capacity") as usize;
    let mut buffer = inkos_engine::utils::run_log::RunLogBuffer::new(capacity);
    for entry in entries_from(&evict_case["appended"]) {
        buffer.append(entry);
    }
    let got = serde_json::to_value(buffer.snapshot()).unwrap();
    assert_eq!(got, evict_case["expected"], "evict case drifted");
}

#[test]
fn project_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["project"].as_array().expect("project array") {
        let input = &vector["input"];
        let snapshot = inkos_engine::utils::run_log::RunLogSnapshot {
            entries: entries_from(&input["snapshot"]["entries"]),
            total_appended: input["snapshot"]["totalAppended"].as_i64().expect("total"),
        };
        let limit = input["limit"].as_i64();
        let got = inkos_engine::utils::run_log::project_run_log(&snapshot, limit);
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected,
            vector["expected"],
            "project vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
