//! R53/1657 号：章版本比较 diff 纯函数 golden 差分（TS 生产向量 → Rust
//! 镜像对拍）。R53 为纯前端功能（S 为主），Rust 侧 golden 纪律镜像——
//! LCS 语义（kept/removed/added 序列）双端逐行一致。

use inkos_engine::utils::text_diff::{diff_chapter_text, TextDiffKind};

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/text-diff-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    vectors: Vec<DiffVector>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiffVector {
    name: String,
    old_text: String,
    new_text: String,
    expected: Vec<ExpectedLine>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum ExpectedKind {
    Kept,
    Removed,
    Added,
}

#[derive(serde::Deserialize)]
struct ExpectedLine {
    kind: ExpectedKind,
    text: String,
}

#[test]
fn text_diff_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(file.vectors.len() >= 8, "golden 非空防空转");
    for vector in &file.vectors {
        let diff = diff_chapter_text(&vector.old_text, &vector.new_text);
        assert_eq!(
            diff.len(),
            vector.expected.len(),
            "{} 行数漂移",
            vector.name
        );
        for (actual, expected) in diff.iter().zip(&vector.expected) {
            assert_eq!(actual.text, expected.text, "{} 行文本漂移", vector.name);
            assert_eq!(
                format!("{:?}", actual.kind),
                format!("{:?}", expected.kind),
                "{} 行分类漂移",
                vector.name
            );
        }
    }
}
