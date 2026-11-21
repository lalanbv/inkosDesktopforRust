//! R49/1644 号：卷元数据纯函数 golden 差分（TS 生产向量 → Rust 镜像对拍）。
//! 唯一事实源 = `packages/core/src/__tests__/golden/chapter-volumes-vectors.json`；
//! 三组对齐 1638 号 R49 验收：卷排序 / 默认卷归属 / 删卷迁移 + id 唯一性守卫。

use inkos_engine::models::book::BookVolume;
use inkos_engine::models::chapter::ChapterMeta;
use inkos_engine::state::volumes::{
    migrate_chapters_on_volume_delete, project_volume_groups, sort_volumes, volume_ids_unique,
};

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/chapter-volumes-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    groups: Vec<GroupVector>,
    sort: Vec<SortVector>,
    migrate: Vec<MigrateVector>,
    unique: Vec<UniqueVector>,
}

#[derive(serde::Deserialize)]
struct GroupVector {
    name: String,
    volumes: Vec<BookVolume>,
    chapters: Vec<ChapterMeta>,
    expected: Vec<inkos_engine::state::volumes::VolumeGroup>,
}

#[derive(serde::Deserialize)]
struct SortVector {
    name: String,
    volumes: Vec<BookVolume>,
    expected: Vec<BookVolume>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrateVector {
    name: String,
    chapters: Vec<ChapterMeta>,
    removed_volume_id: String,
    expected: Vec<ChapterMeta>,
}

#[derive(serde::Deserialize)]
struct UniqueVector {
    name: String,
    volumes: Vec<BookVolume>,
    expected: bool,
}

#[test]
fn groups_match_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(!file.groups.is_empty(), "golden 非空防空转");
    for vector in &file.groups {
        assert_eq!(
            project_volume_groups(&vector.volumes, &vector.chapters),
            vector.expected,
            "{} 分组投影漂移",
            vector.name
        );
    }
}

#[test]
fn sort_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    for vector in &file.sort {
        assert_eq!(
            sort_volumes(&vector.volumes),
            vector.expected,
            "{} 排序漂移",
            vector.name
        );
    }
}

#[test]
fn migrate_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    for vector in &file.migrate {
        assert_eq!(
            migrate_chapters_on_volume_delete(&vector.chapters, &vector.removed_volume_id),
            vector.expected,
            "{} 删卷迁移漂移",
            vector.name
        );
    }
}

#[test]
fn unique_matches_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    for vector in &file.unique {
        assert_eq!(
            volume_ids_unique(&vector.volumes),
            vector.expected,
            "{} id 唯一性守卫漂移",
            vector.name
        );
    }
}
