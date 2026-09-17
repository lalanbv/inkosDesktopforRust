//! 521 号：搜索分词器共享 golden 差分。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/search-tokenizer-vectors.json`
//! （TS `tokenizeSearchText` 生成）。Rust `tokenize_search_text` 自 521 号起换
//! icu_segmenter（与 TS Intl.Segmenter 同源 ICU 词典分词），本测试锁死双端
//! token 序列一致——分词一致是 BM25 打分可比的前提（差分器实证：分词漂移
//! → tf/df 与长度归一化全漂移 → hybrid-search 边缘命中抖动）。
use inkos_engine::utils::local_search::tokenize_search_text;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/search-tokenizer-vectors.json");

#[test]
fn tokenizer_matches_shared_vectors() {
    let parsed: Value = serde_json::from_str(VECTORS).expect("golden json");
    let vectors = parsed["vectors"].as_array().expect("vectors array");
    assert!(
        !vectors.is_empty(),
        "golden 必须非空（空文件会静默跳过对拍）"
    );
    for vector in vectors {
        let input = vector["input"].as_str().expect("input");
        let expected: Vec<&str> = vector["tokens"]
            .as_array()
            .expect("tokens")
            .iter()
            .map(|t| t.as_str().expect("token str"))
            .collect();
        let got = tokenize_search_text(input);
        assert_eq!(
            got,
            expected,
            "分词序列漂移 input={input:?} got={got:?} expected={expected:?}"
        );
    }
}
