//! 214 号：导出面 markdown→simple html 共享 golden 差分——与 core
//! `__tests__/golden-export.test.ts` 读同一份 `export-vectors.json`
//! （期望值以 TS 语义为基准；Rust 漂移即红即修）。

const VECTORS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../packages/core/src/__tests__/golden/export-vectors.json"
));

#[test]
fn export_markdown_html_matches_shared_golden_vectors() {
    let vectors: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
    for tc in vectors["cases"].as_array().unwrap() {
        let name = tc["name"].as_str().unwrap();
        let markdown = tc["markdown"].as_str().unwrap();
        let (title, html) = markdown_to_simple_html_pub(markdown);
        assert_eq!(
            title,
            tc["expectedTitle"].as_str().unwrap(),
            "{name}: title 漂移"
        );
        assert_eq!(html, tc["expectedHtml"].as_str().unwrap(), "{name}: html 漂移");
    }
}

/// 差分入口（复用模块内 markdown_to_simple_html——tests 面经 pub 包装）。
fn markdown_to_simple_html_pub(markdown: &str) -> (String, String) {
    inkos_engine::interaction::export_artifact::markdown_to_simple_html_for_test(markdown)
}
