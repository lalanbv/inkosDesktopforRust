//! 358 号：张力曲线共享 golden 差分（R2，二轮 P0）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/tension-curve-vectors.json`；
//! core 侧 `src/__tests__/golden-tension-curve.test.ts` 断言同文件。
//! 三组差分：TENSION_METRICS 节解析（JSON/行式/clamp/缺字段）、
//! 章级点列聚合（缺分跳过+升序）、启发式告警（平坦/高潮拥挤/弱钩连击/健康）。

use inkos_engine::utils::tension_curve::{
    build_tension_curve, detect_tension_warnings, parse_tension_metrics, TensionLanguage,
    TensionRow, TENSION_WARNING_DEFAULTS,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/tension-curve-vectors.json");

fn language_of(raw: &str) -> TensionLanguage {
    match raw {
        "en" => TensionLanguage::En,
        _ => TensionLanguage::Zh,
    }
}

#[test]
fn parse_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["parse"].as_array().expect("parse array") {
        let input = vector["input"].as_str().unwrap();
        let got = parse_tension_metrics(input);
        let expected = &vector["expected"];
        if expected.is_null() {
            assert!(
                got.is_none(),
                "parse vector '{}' should yield none, got {:?}",
                vector["name"].as_str().unwrap(),
                got
            );
            continue;
        }
        let metrics = got.unwrap_or_else(|| {
            panic!(
                "parse vector '{}' should parse, got none",
                vector["name"].as_str().unwrap()
            )
        });
        let got_json = serde_json::to_value(&metrics).unwrap();
        assert_eq!(
            got_json, *expected,
            "parse vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn curve_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["curve"].as_array().expect("curve array") {
        let rows: Vec<TensionRow> =
            serde_json::from_value(vector["input"].clone()).expect("tension rows shape");
        let got = build_tension_curve(&rows);
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "curve vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn warnings_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["warnings"].as_array().expect("warnings array") {
        let points: Vec<inkos_engine::utils::tension_curve::TensionPoint> =
            serde_json::from_value(vector["input"].clone()).expect("points shape");
        let got = detect_tension_warnings(
            &points,
            language_of(vector["language"].as_str().unwrap_or("zh")),
            &TENSION_WARNING_DEFAULTS,
        );
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "warning vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}
