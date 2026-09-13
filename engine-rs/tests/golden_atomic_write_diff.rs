//! 404 号：持久层原子写共享向量差分（TS 真源
//! `packages/core/src/utils/atomic-write.ts`）。
//! 唯一事实源 = `../core/…/golden/atomic-write-vectors.json`（include_str 同文件）。
//! 断言：常量契约 + 重试决策纯函数（Rust 侧错误码语义映射见
//! `atomic_file_set::atomic_write_error_kind`：EPERM=1 / EBUSY=16）。

use serde_json::Value;

const VECTORS: &str = include_str!(
    "../../packages/core/src/__tests__/golden/atomic-write-vectors.json"
);

#[test]
fn contract_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let contract = &vectors["contract"];
    assert_eq!(
        inkos_engine::utils::atomic_file_set::ATOMIC_WRITE_MAX_RETRIES,
        contract["maxRetries"].as_u64().expect("maxRetries") as u32,
    );
    assert_eq!(
        inkos_engine::utils::atomic_file_set::ATOMIC_WRITE_RETRY_DELAY_MS,
        contract["retryDelayMs"].as_u64().expect("delayMs"),
    );
}

#[test]
fn retry_decision_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["retryDecision"].as_array().expect("array") {
        let input = &vector["input"];
        let error_kind = input["errorCode"].as_str();
        let attempt = input["attempt"].as_i64().expect("attempt") as u32;
        let got = inkos_engine::utils::atomic_file_set::atomic_write_retry_decision(
            error_kind,
            attempt,
            inkos_engine::utils::atomic_file_set::ATOMIC_WRITE_MAX_RETRIES,
        );
        assert_eq!(
            got,
            vector["expected"].as_bool().expect("expected bool"),
            "retry vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn io_error_kind_maps_unix_file_lock_codes() {
    use inkos_engine::utils::atomic_file_set::atomic_write_error_kind;
    let eperm = std::io::Error::from_raw_os_error(1);
    let ebusy = std::io::Error::from_raw_os_error(16);
    let enospc = std::io::Error::from_raw_os_error(28);
    assert_eq!(atomic_write_error_kind(&eperm), Some("EPERM"));
    assert_eq!(atomic_write_error_kind(&ebusy), Some("EBUSY"));
    assert_eq!(atomic_write_error_kind(&enospc), None);
    assert_eq!(
        atomic_write_error_kind(&std::io::Error::other("plain")),
        None
    );
}
