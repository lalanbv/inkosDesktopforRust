//! R47/1640 号：全局搜索高亮裁剪 golden 差分（TS 生产向量 → Rust 镜像对拍）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/search-highlight-vectors.json`
//! （TS 生产函数导出，contract-test-samples-derive-from-production 纪律）；
//! TS 侧锚面 = `packages/core/src/__tests__/golden-search-highlight.test.ts`。
//! 偏移口径 = 码点下标（chars().count() 同口径，astral 向量锁定）。

use inkos_engine::utils::global_search::build_highlight_snippet;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/search-highlight-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    vectors: Vec<HighlightVector>,
}

#[derive(serde::Deserialize)]
struct HighlightVector {
    name: String,
    content: String,
    query: String,
    expected: Option<ExpectedSnippet>,
}

#[derive(serde::Deserialize)]
struct ExpectedSnippet {
    text: String,
    #[serde(rename = "matchStart")]
    match_start: usize,
    #[serde(rename = "matchLength")]
    match_length: usize,
}

#[test]
fn highlight_snippet_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(!file.vectors.is_empty(), "golden 非空防空转");
    for vector in &file.vectors {
        let got = build_highlight_snippet(&vector.content, &vector.query);
        match (&vector.expected, got) {
            (None, None) => {}
            (Some(expected), Some(snippet)) => {
                assert_eq!(snippet.text, expected.text, "{} 片段文本漂移", vector.name);
                assert_eq!(
                    snippet.match_start, expected.match_start,
                    "{} 命中下标漂移",
                    vector.name
                );
                assert_eq!(
                    snippet.match_length, expected.match_length,
                    "{} 命中长度漂移",
                    vector.name
                );
            }
            (None, Some(snippet)) => {
                panic!("{} 应无命中，Rust 产 {:?}", vector.name, snippet.text)
            }
            (Some(_), None) => panic!("{} 应有命中，Rust 返回 None", vector.name),
        }
    }
}
