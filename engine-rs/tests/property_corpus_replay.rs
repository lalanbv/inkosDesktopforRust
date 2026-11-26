//! R62/1690 号：落盘属性语料双端回放（Rust 侧）。语料由
//! scripts/generate-property-corpus.mjs 以固定 seed 生成（期望值=生成时实现
//! 现算），TS 侧回放 = packages/core/src/__tests__/property-corpus-replay.test.ts
//! （fast-check 原生断言亦在该域）；本测逐样本回放当前 Rust 实现——实现变更
//! 即红（有意变更须重跑生成器换基线）。必红自检已验证：临时破坏一个镜像
//! （structure_invariant 摘多重集）→ 本测红。

use inkos_engine::interaction::chapter_structure::{
    content_fingerprint, structure_invariant, ChapterFingerprint, StructureOp,
};
use inkos_engine::interaction::export_artifact::{docx_escape_xml, md_zip_entry_path};
use inkos_engine::models::book::BookVolume;
use inkos_engine::utils::usage_ledger::{usage_projection, UsageLogRow};
use inkos_engine::utils::writing_goal::{
    daily_word_delta, expected_finish, goal_streak, shift_day_key, DayDelta, WritingLogRow,
};
use serde::Deserialize;
use serde_json::Value;

const CORPUS_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../packages/studio/src/__tests__/golden/property-corpus"
);

fn load(name: &str) -> Value {
    let path = format!("{CORPUS_DIR}/{name}.json");
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
        .unwrap()
}

fn corpus_guard(file: &Value, name: &str) {
    assert_eq!(file["sampleCount"].as_u64(), Some(200), "{name} 样本量下限");
    assert_eq!(file["seed"].as_u64(), Some(20261126), "{name} seed");
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EscapeVector {
    input: String,
    output: String,
}

#[test]
fn docx_escape_xml_corpus() {
    let file = load("docx-escape-xml");
    corpus_guard(&file, "docx-escape-xml");
    for vector in serde_json::from_value::<Vec<EscapeVector>>(file["samples"].clone()).unwrap() {
        assert_eq!(
            docx_escape_xml(&vector.input),
            vector.output,
            "{}",
            vector.input
        );
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZipInput {
    #[serde(default)]
    volume_id: Option<String>,
    #[serde(default)]
    volumes: Vec<BookVolume>,
    chapter_number: u32,
    chapter_title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZipVector {
    input: ZipInput,
    output: String,
}

#[test]
fn md_zip_entry_path_corpus() {
    let file = load("md-zip-entry-path");
    corpus_guard(&file, "md-zip-entry-path");
    for vector in serde_json::from_value::<Vec<ZipVector>>(file["samples"].clone()).unwrap() {
        let output = md_zip_entry_path(
            vector.input.volume_id.as_deref(),
            &vector.input.volumes,
            vector.input.chapter_number,
            &vector.input.chapter_title,
        );
        assert_eq!(output, vector.output, "{}", vector.output);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeltaRow {
    ts: String,
    chapter_num: u32,
    before: i64,
    after: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeltaVector {
    rows: Vec<DeltaRow>,
    output: Vec<DayDelta>,
}

#[test]
fn daily_word_delta_corpus() {
    let file = load("daily-word-delta");
    corpus_guard(&file, "daily-word-delta");
    for vector in serde_json::from_value::<Vec<DeltaVector>>(file["samples"].clone()).unwrap() {
        let rows: Vec<WritingLogRow> = vector
            .rows
            .iter()
            .map(|row| WritingLogRow {
                ts: row.ts.clone(),
                chapter_num: row.chapter_num,
                before: row.before,
                after: row.after,
            })
            .collect();
        let output = daily_word_delta(&rows, |row| row.ts.chars().take(10).collect());
        assert_eq!(output, vector.output);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreakVector {
    input: StreakInput,
    output: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreakInput {
    deltas: Vec<DayDelta>,
    daily_goal_words: i64,
    today_key: String,
}

#[test]
fn goal_streak_corpus() {
    let file = load("goal-streak");
    corpus_guard(&file, "goal-streak");
    for vector in serde_json::from_value::<Vec<StreakVector>>(file["samples"].clone()).unwrap() {
        assert_eq!(
            goal_streak(
                &vector.input.deltas,
                vector.input.daily_goal_words,
                &vector.input.today_key
            ),
            vector.output,
            "{}",
            vector.input.today_key
        );
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageRow {
    service_key: String,
    model: String,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    ts: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageVector {
    rows: Vec<UsageRow>,
    today_key: String,
    output: Vec<Value>,
}

#[test]
fn usage_projection_corpus() {
    let file = load("usage-projection");
    corpus_guard(&file, "usage-projection");
    for vector in serde_json::from_value::<Vec<UsageVector>>(file["samples"].clone()).unwrap() {
        let rows: Vec<UsageLogRow> = vector
            .rows
            .iter()
            .map(|row| UsageLogRow {
                ts: row.ts.clone(),
                service_key: row.service_key.clone(),
                model: row.model.clone(),
                prompt_tokens: row.prompt_tokens,
                completion_tokens: row.completion_tokens,
                total_tokens: row.total_tokens,
            })
            .collect();
        let projection = usage_projection(
            &rows,
            |row| row.ts.chars().take(10).collect(),
            &vector.today_key,
        );
        let mut sorted: Vec<Value> = projection
            .into_iter()
            .map(|(service_key, usage)| {
                serde_json::json!({
                    "serviceKey": service_key,
                    "daily": usage.daily,
                    "monthly": usage.monthly,
                    "total": usage.total,
                })
            })
            .collect();
        sorted.sort_by_key(|entry| entry["serviceKey"].as_str().unwrap_or_default().to_string());
        let mut expected = vector.output;
        expected.sort_by_key(|entry| entry["serviceKey"].as_str().unwrap_or_default().to_string());
        assert_eq!(sorted, expected, "{}", vector.today_key);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinishVector {
    input: FinishInput,
    output: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinishInput {
    current_words: i64,
    target_words: i64,
    daily_pace_words: i64,
    today_key: String,
}

#[test]
fn expected_finish_corpus() {
    let file = load("expected-finish");
    corpus_guard(&file, "expected-finish");
    for vector in serde_json::from_value::<Vec<FinishVector>>(file["samples"].clone()).unwrap() {
        assert_eq!(
            expected_finish(
                vector.input.current_words,
                vector.input.target_words,
                vector.input.daily_pace_words,
                &vector.input.today_key,
            ),
            vector.output,
            "{}",
            vector.input.today_key
        );
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShiftVector {
    input: ShiftInput,
    output: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShiftInput {
    day_key: String,
    days: i64,
}

#[test]
fn shift_day_key_corpus() {
    let file = load("shift-day-key");
    corpus_guard(&file, "shift-day-key");
    for vector in serde_json::from_value::<Vec<ShiftVector>>(file["samples"].clone()).unwrap() {
        assert_eq!(
            shift_day_key(&vector.input.day_key, vector.input.days),
            vector.output,
            "{} +{}",
            vector.input.day_key,
            vector.input.days
        );
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InvariantVector {
    op: StructureOp,
    before: Vec<ChapterFingerprint>,
    after: Vec<ChapterFingerprint>,
    ok: bool,
    violations: Vec<String>,
}

#[test]
fn structure_invariant_corpus() {
    let file = load("structure-invariant");
    corpus_guard(&file, "structure-invariant");
    for vector in serde_json::from_value::<Vec<InvariantVector>>(file["samples"].clone()).unwrap() {
        let violations = structure_invariant(&vector.op, &vector.before, &vector.after);
        assert_eq!(
            violations, vector.violations,
            "op={:?} before={:?}",
            vector.op, vector.before
        );
        assert_eq!(violations.is_empty(), vector.ok, "op={:?}", vector.op);
    }
}

/// 指纹函数同族（语料指纹形态由生成器决定，本测锁算法恒定）。
#[test]
fn content_fingerprint_is_sha256_prefix16() {
    assert_eq!(
        content_fingerprint("第一章正文"),
        "21b87a81ef1739c6",
        "与 TS golden contentFingerprint 段同值"
    );
}
