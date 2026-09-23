//! 551 号：R33 遥测 schema 共享 golden 差分。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/inkos-ai-request-schema.json`
//! （第 31 守门域；543 号施工图 §4）。`inkos.ai.request` span 语义重合键镜像
//! 上游 pi.ai.request（AI_TELEMETRY_SCHEMA 0.87），域扩展键 inkos.*（RunLogEntry
//! 403 号九字段映射）。Rust 侧按裁决**只投影不引运行时**——本测试锁死 schema
//! 加载与键集形态，防止双端任一侧静默漂移。

use serde_json::Value;

const SCHEMA: &str =
    include_str!("../../packages/core/src/__tests__/golden/inkos-ai-request-schema.json");

const EXPECTED_START_KEYS: [&str; 9] = [
    "pi.ai.operation",
    "pi.ai.provider",
    "pi.ai.model",
    "pi.ai.api",
    "pi.ai.streaming",
    "inkos.agent",
    "inkos.attempt_index",
    "inkos.round",
    "inkos.took_over",
];

const EXPECTED_END_KEYS: [&str; 2] = ["pi.ai.response.model", "inkos.error_kind"];

#[test]
fn telemetry_schema_matches_shared_golden() {
    let parsed: Value = serde_json::from_str(SCHEMA).expect("golden json 解析");
    assert_eq!(parsed["version"].as_i64(), Some(1), "schema version");

    let spans = parsed["spans"].as_object().expect("spans object");
    assert_eq!(spans.len(), 1, "单一 span 域（新增域=显式更新双端测试）");
    let span = spans
        .get("inkos.ai.request")
        .expect("inkos.ai.request span 存在");

    let mut start_keys: Vec<&str> = span["startAttributes"]
        .as_object()
        .expect("startAttributes object")
        .keys()
        .map(String::as_str)
        .collect();
    start_keys.sort_unstable();
    let mut expected_start = EXPECTED_START_KEYS.to_vec();
    expected_start.sort_unstable();
    assert_eq!(
        start_keys, expected_start,
        "start 属性键集漂移——镜像 pi.ai.* 与 inkos.* 扩展面必须双端同步"
    );

    let mut end_keys: Vec<&str> = span["endAttributes"]
        .as_object()
        .expect("endAttributes object")
        .keys()
        .map(String::as_str)
        .collect();
    end_keys.sort_unstable();
    let mut expected_end = EXPECTED_END_KEYS.to_vec();
    expected_end.sort_unstable();
    assert_eq!(end_keys, expected_end, "end 属性键集漂移");

    // required 全集 = InkOS 请求面必填语义（构造器覆盖断言的 Rust 对偶）。
    for key in EXPECTED_START_KEYS {
        let required = span["startAttributes"][key]["required"]
            .as_bool()
            .unwrap_or(false);
        assert!(required, "start 属性 {key} 应为 required");
    }

    // 镜像键语义抽查：operation 枚举值与上游 pi.ai.request 一致。
    let operation_values: Vec<&str> = span["startAttributes"]["pi.ai.operation"]["values"]
        .as_array()
        .expect("operation values")
        .iter()
        .map(|v| v.as_str().expect("value str"))
        .collect();
    assert_eq!(
        operation_values,
        ["stream", "fetch_deferred", "cancel_deferred", "generate_images"],
        "pi.ai.operation 枚举漂移"
    );

    let error_kind_values: Vec<&str> = span["endAttributes"]["inkos.error_kind"]["values"]
        .as_array()
        .expect("error_kind values")
        .iter()
        .map(|v| v.as_str().expect("value str"))
        .collect();
    assert_eq!(
        error_kind_values,
        ["transient", "fatal"],
        "inkos.error_kind 枚举漂移（RunLogErrorKind 对齐面）"
    );

    let status = &span["status"];
    assert_eq!(status["default"].as_str(), Some("ok"), "status default");
    assert!(
        status["errorWhen"].as_str().is_some_and(|s| !s.is_empty()),
        "status errorWhen 必填"
    );
}
