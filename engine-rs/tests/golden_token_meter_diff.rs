//! 563 号：R41 token 计量共享 golden 差分。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/token-meter-vectors.json`
//! （双端共享；TS 侧消费测试 564 号落地）。回放 append/noteUsage/measure
//! 操作序列，逐 case 对拍 [`TokenMeterSnapshot`] 序列化形态——锚点三态
//! （usage 锚/估算/收缩拒绝）、clamp、覆盖率、超窗旗标全部码点级锁死。

use inkos_engine::utils::token_meter::TokenMeter;
use serde_json::Value;

const VECTORS: &str = include_str!("../../packages/core/src/__tests__/golden/token-meter-vectors.json");

#[test]
fn token_meter_golden_vectors_replay() {
    let parsed: Value = serde_json::from_str(VECTORS).expect("golden json 解析");
    assert_eq!(parsed["version"].as_i64(), Some(1), "向量版本");
    let cases = parsed["cases"].as_array().expect("cases array");
    assert!(!cases.is_empty(), "向量非空");

    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let meter = TokenMeter::new(case["inputWindow"].as_u64().expect("inputWindow"));
        for op in case["ops"].as_array().expect("ops") {
            match op["op"].as_str().expect("op kind") {
                "append" => {
                    let text = op["text"].as_str().expect("append text");
                    let repeat = op["repeat"].as_u64().unwrap_or(1);
                    for _ in 0..repeat {
                        meter.append(text);
                    }
                }
                "noteUsage" => {
                    let accepted = meter.note_usage(
                        op["model"].as_str(),
                        op["promptTokens"].as_u64().expect("promptTokens"),
                        op["completionTokens"].as_u64().expect("completionTokens"),
                        op["totalTokens"].as_u64().expect("totalTokens"),
                    );
                    assert_eq!(
                        accepted,
                        op["accepted"].as_bool().expect("accepted"),
                        "case {name}: noteUsage 接受性漂移"
                    );
                }
                "measure" => {
                    let actual = serde_json::to_value(meter.measure()).expect("snapshot 序列化");
                    assert_eq!(
                        &actual,
                        &op["expect"],
                        "case {name}: 快照漂移（双端共享向量，改向量须双端同批）"
                    );
                }
                other => panic!("case {name}: 未知 op {other}"),
            }
        }
    }
}
