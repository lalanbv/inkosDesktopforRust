//! R47/1640 号：全局搜索排序 golden 差分（TS 生产向量 → Rust 镜像对拍）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/search-ranking-vectors.json`
//! （TS 生产函数导出）；TS 侧锚面 =
//! `packages/core/src/__tests__/golden-search-ranking.test.ts`。
//! 双端 LocalSearchIndex（FTS5 bm25 5.0/1.0 权重）同输入同命中序；
//! 分数不进向量（ICU 版本漂移容忍面），序与命中集=契约面。

use inkos_engine::utils::local_search::{LocalSearchIndex, SearchDocument, SearchOptions};

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/search-ranking-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    vectors: Vec<RankingVector>,
}

#[derive(serde::Deserialize)]
struct RankingVector {
    name: String,
    documents: Vec<RankingDocument>,
    query: String,
    limit: usize,
    #[serde(rename = "expectedIds")]
    expected_ids: Vec<String>,
}

#[derive(serde::Deserialize)]
struct RankingDocument {
    id: String,
    title: String,
    body: String,
}

#[test]
fn search_order_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(!file.vectors.is_empty(), "golden 非空防空转");
    for vector in &file.vectors {
        let index = LocalSearchIndex::new(":memory:").expect("内存索引");
        let documents: Vec<SearchDocument> = vector
            .documents
            .iter()
            .map(|document| SearchDocument {
                id: document.id.clone(),
                scope: "golden:ranking".to_string(),
                kind: "chapter".to_string(),
                source: format!("{}.md", document.id),
                title: document.title.clone(),
                body: document.body.clone(),
                metadata: None,
            })
            .collect();
        index
            .replace_scope("golden:ranking", &documents)
            .expect("投影写入");
        let hits = index.search(
            &vector.query,
            &SearchOptions {
                scope: "golden:ranking",
                kinds: &[],
                limit: vector.limit,
            },
        );
        let ids: Vec<String> = hits.iter().map(|hit| hit.id.clone()).collect();
        assert_eq!(ids, vector.expected_ids, "{} 命中序漂移", vector.name);
    }
}
