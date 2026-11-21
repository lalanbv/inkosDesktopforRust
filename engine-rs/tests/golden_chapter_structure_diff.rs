//! R48a/1641 号：章节结构纯函数 golden 差分（TS 生产向量 → Rust 镜像对拍）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/chapter-structure-vectors.json`
//! （TS 生产函数导出，contract-test-samples-derive-from-production 纪律）；
//! TS 侧锚面 = `packages/core/src/__tests__/golden-chapter-structure.test.ts`。
//! 覆盖四组：revision 章集合指纹（sha256 前 16 hex——双端 JSON 序列化逐位
//! 一致是乐观并发可比的前提）/ 重编号三族 / 拆分边界（UTF-16 契约）/
//! 合并分隔。

use inkos_engine::interaction::chapter_structure::{
    chapter_set_revision, merge_content, renumber_after_merge, renumber_after_split, reorder_plan,
    split_utf16, RenumberedEntry,
};
use inkos_engine::models::chapter::{ChapterMeta, ChapterStatus};

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/chapter-structure-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    revision: Vec<RevisionVector>,
    renumber: RenumberVectors,
    split: Vec<SplitVector>,
    merge: Vec<MergeVector>,
}

#[derive(serde::Deserialize)]
struct RevisionVector {
    name: String,
    index: Vec<IndexRow>,
    expected: String,
}

#[derive(serde::Deserialize)]
struct IndexRow {
    number: u32,
    #[serde(rename = "wordCount")]
    word_count: u32,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

#[derive(serde::Deserialize)]
struct RenumberVectors {
    split: Vec<SplitRenumberVector>,
    merge: Vec<MergeRenumberVector>,
    reorder: Vec<ReorderVector>,
}

#[derive(serde::Deserialize)]
struct SplitRenumberVector {
    name: String,
    numbers: Vec<u32>,
    at: u32,
    expected: Vec<RenumberedEntry>,
}

#[derive(serde::Deserialize)]
struct MergeRenumberVector {
    name: String,
    numbers: Vec<u32>,
    removed: u32,
    expected: Vec<RenumberedEntry>,
}

#[derive(serde::Deserialize)]
struct ReorderVector {
    name: String,
    numbers: Vec<u32>,
    order: Vec<u32>,
    #[serde(default)]
    invalid: bool,
    expected: Option<Vec<RenumberedEntry>>,
}

#[derive(serde::Deserialize)]
struct SplitVector {
    name: String,
    content: String,
    offset: usize,
    #[serde(default)]
    invalid: bool,
    expected: Option<SplitExpected>,
}

#[derive(serde::Deserialize)]
struct SplitExpected {
    head: String,
    tail: String,
}

#[derive(serde::Deserialize)]
struct MergeVector {
    name: String,
    a: String,
    b: String,
    separator: String,
    expected: String,
}

fn meta_from_row(row: &IndexRow) -> ChapterMeta {
    ChapterMeta {
        number: row.number,
        title: String::new(),
        status: ChapterStatus::ReadyForReview,
        word_count: row.word_count,
        created_at: "2026-11-21T00:00:00.000Z".to_string(),
        updated_at: row.updated_at.clone(),
        audit_issues: Vec::new(),
        volume_id: None,
        length_warnings: Vec::new(),
        review_note: None,
        detection_score: None,
        detection_provider: None,
        detected_at: None,
        length_telemetry: None,
        token_usage: None,
    }
}

fn entries(list: &[RenumberedEntry]) -> Vec<(u32, u32)> {
    list.iter().map(|entry| (entry.from, entry.to)).collect()
}

fn pairs(list: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    list
}

#[test]
fn revision_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(!file.revision.is_empty(), "golden 非空防空转");
    for vector in &file.revision {
        let index: Vec<ChapterMeta> = vector.index.iter().map(meta_from_row).collect();
        assert_eq!(
            chapter_set_revision(&index),
            vector.expected,
            "{} revision 指纹漂移（双端序列化面）",
            vector.name
        );
    }
}

#[test]
fn renumber_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    for vector in &file.renumber.split {
        assert_eq!(
            pairs(renumber_after_split(&vector.numbers, vector.at)),
            entries(&vector.expected),
            "{} split 重编号漂移",
            vector.name
        );
    }
    for vector in &file.renumber.merge {
        assert_eq!(
            pairs(renumber_after_merge(&vector.numbers, vector.removed)),
            entries(&vector.expected),
            "{} merge 重编号漂移",
            vector.name
        );
    }
    for vector in &file.renumber.reorder {
        let got = reorder_plan(&vector.numbers, &vector.order);
        if vector.invalid || vector.expected.is_none() {
            assert!(got.is_err(), "{} 应拒绝非双射", vector.name);
            continue;
        }
        assert_eq!(
            pairs(got.expect("合法序应成功")),
            entries(vector.expected.as_ref().expect("expected")),
            "{} reorder 重编号漂移",
            vector.name
        );
    }
}

#[test]
fn split_utf16_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(!file.split.is_empty(), "golden 非空防空转");
    for vector in &file.split {
        let got = split_utf16(&vector.content, vector.offset);
        if vector.invalid || vector.expected.is_none() {
            assert!(got.is_err(), "{} 应拒绝非法切点", vector.name);
            continue;
        }
        let expected = vector.expected.as_ref().expect("expected");
        let (head, tail) = got.expect("合法切点应成功");
        assert_eq!(head, expected.head, "{} 前段漂移", vector.name);
        assert_eq!(tail, expected.tail, "{} 后段漂移", vector.name);
    }
}

#[test]
fn merge_content_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(!file.merge.is_empty(), "golden 非空防空转");
    for vector in &file.merge {
        assert_eq!(
            merge_content(&vector.a, &vector.b, &vector.separator),
            vector.expected,
            "{} 合并拼接漂移",
            vector.name
        );
    }
}
