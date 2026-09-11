//! 341 号：信息差账本共享 golden 差分（G7a，Phase B 批次二）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/info-gap-vectors.json`；
//! core 侧 `src/__tests__/golden-info-gap.test.ts` 断言同文件。
//! 六组差分：账本解析、渲染、泄密机检、废笔机检、审计注入文本、契约形状。

use inkos_engine::utils::info_gap_ledger::{
    detect_reader_redundancy, detect_secret_leaks, info_gap_contract, parse_info_gaps_markdown,
    render_info_gap_audit_notes, render_info_gaps_markdown, InfoGapEntry, ReaderRedundancyHit,
    SecretLeakHit,
};
use serde::Deserialize;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/info-gap-vectors.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GapCase {
    #[serde(default)]
    current_chapter: i64,
    content: String,
    gaps: Vec<InfoGapEntry>,
}

#[test]
fn parse_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["parse"].as_array().expect("parse array") {
        let got = parse_info_gaps_markdown(vector["input"].as_str().unwrap());
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected, vector["expected"],
            "parse vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn render_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["render"].as_array().expect("render array") {
        let entries: Vec<InfoGapEntry> =
            serde_json::from_value(vector["input"].clone()).expect("entries shape");
        assert_eq!(
            render_info_gaps_markdown(&entries),
            vector["expected"].as_str().unwrap(),
            "render vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn leaks_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["leaks"].as_array().expect("leaks array") {
        let case: GapCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let got: Vec<SecretLeakHit> =
            detect_secret_leaks(&case.content, &case.gaps, case.current_chapter);
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected, vector["expected"],
            "leaks vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn redundancy_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["redundancy"].as_array().expect("redundancy array") {
        let case: GapCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let got: Vec<ReaderRedundancyHit> =
            detect_reader_redundancy(&case.content, &case.gaps);
        let expected = serde_json::to_value(&got).unwrap();
        assert_eq!(
            expected, vector["expected"],
            "redundancy vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditNotesCase {
    leaks: Vec<SecretLeakHit>,
    redundancies: Vec<ReaderRedundancyHit>,
    language: String,
}

#[test]
fn audit_notes_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["auditNotes"].as_array().expect("auditNotes array") {
        let case: AuditNotesCase = serde_json::from_value(vector["input"].clone()).unwrap();
        let got = render_info_gap_audit_notes(
            &case.leaks,
            &case.redundancies,
            Some(&case.language),
        );
        let expected = vector["expected"].as_str().map(String::from);
        assert_eq!(
            got,
            expected,
            "auditNotes vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn contract_shape_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let got = serde_json::to_value(info_gap_contract()).expect("serialize contract");
    assert_eq!(got, vectors["contract"], "contract shape drifted");
}
