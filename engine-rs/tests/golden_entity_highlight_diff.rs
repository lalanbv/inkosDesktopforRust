//! R52/1656 号：实体命中集扫描 golden 差分（TS 生产向量 → Rust 镜像对拍）
//! ——大小写敏感/别名最长优先/跨卡重叠贪心/排除短语/minLength/astral 码点
//! 口径。R52 为纯前端功能（S 为主），Rust 侧仅 golden 纪律镜像，零引擎
//! 接线。

use inkos_engine::utils::entity_highlight::{scan_entity_hits, EntityCardLike};

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/entity-highlight-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    cards: Vec<EntityCardLike>,
    vectors: Vec<HighlightVector>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HighlightVector {
    name: String,
    text: String,
    #[serde(rename = "minLength")]
    min_length: usize,
    exclude: Vec<String>,
    expected: Vec<ExpectedHit>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedHit {
    entity: String,
    card_index: usize,
    start: usize,
    end: usize,
}

#[test]
fn entity_highlight_scan_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(file.vectors.len() >= 8, "golden 非空防空转");
    for vector in &file.vectors {
        let hits = scan_entity_hits(
            &vector.text,
            &file.cards,
            vector.min_length,
            &vector.exclude,
        );
        let actual: Vec<(String, usize, usize, usize)> = hits
            .iter()
            .map(|hit| (hit.entity.clone(), hit.card_index, hit.start, hit.end))
            .collect();
        let expected: Vec<(String, usize, usize, usize)> = vector
            .expected
            .iter()
            .map(|hit| (hit.entity.clone(), hit.card_index, hit.start, hit.end))
            .collect();
        assert_eq!(actual, expected, "{} 命中集漂移", vector.name);
    }
}
