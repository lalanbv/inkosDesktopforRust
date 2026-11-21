//! R50/1645 号：md-zip 条目路径构造 golden 差分（TS 生产向量 → Rust 镜像
//! 对拍）。卷目录/未分组回退/sanitize/空 title 六案锁定——TS 侧 =
//! `golden-chapter-mdzip.test.ts`，向量由 TS 生产函数导出。

use inkos_engine::interaction::export_artifact::md_zip_entry_path;
use inkos_engine::models::book::BookVolume;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/chapter-mdzip-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    cases: Vec<PathVector>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathVector {
    name: String,
    volume_id: Option<String>,
    volumes: Vec<BookVolume>,
    chapter_number: u32,
    chapter_title: String,
    expected: String,
}

#[test]
fn md_zip_entry_paths_match_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(!file.cases.is_empty(), "golden 非空防空转");
    for vector in &file.cases {
        assert_eq!(
            md_zip_entry_path(
                vector.volume_id.as_deref(),
                &vector.volumes,
                vector.chapter_number,
                &vector.chapter_title,
            ),
            vector.expected,
            "{} 条目路径漂移",
            vector.name
        );
    }
}
