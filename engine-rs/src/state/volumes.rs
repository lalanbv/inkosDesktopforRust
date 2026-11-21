//! 卷/幕元数据纯函数（R49，1644 号）。
//!
//! 移植镜像 `packages/core/src/state/volumes.ts`（golden
//! chapter-volumes-vectors.json 锁双端：卷排序/默认卷归属/删卷迁移三组）。
//!
//! 组织面语义备案：卷是 book.json.writing.volumes 有序列表；章归属 =
//! `ChapterMeta.volume_id`（可选键）；**volume_id=None = 未分组归「正文」
//! 默认卷**（投影默认组 title=None，文案由 UI 层本地化）；卷不进装配注入。

use crate::models::book::BookVolume;
use crate::models::chapter::ChapterMeta;

/// 卷排序：order 升序（双端同 comparator）。
pub fn sort_volumes(volumes: &[BookVolume]) -> Vec<BookVolume> {
    let mut sorted: Vec<BookVolume> = volumes.to_vec();
    sorted.sort_by_key(|volume| volume.order);
    sorted
}

/// 分组投影（camelCase 序列化，TS 镜像逐键对齐）：卷按 order 升序各一组；
/// 未分组章归默认卷（volume_id=None、title=None 由 UI 本地化）殿后；
/// 组内章按 number 升序。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeGroup {
    pub volume_id: Option<String>,
    pub title: Option<String>,
    pub chapters: Vec<u32>,
}

pub fn project_volume_groups(volumes: &[BookVolume], chapters: &[ChapterMeta]) -> Vec<VolumeGroup> {
    let mut ordered: Vec<&ChapterMeta> = chapters.iter().collect();
    ordered.sort_by_key(|chapter| chapter.number);
    let mut by_volume: std::collections::BTreeMap<String, Vec<u32>> =
        std::collections::BTreeMap::new();
    let mut ungrouped: Vec<u32> = Vec::new();
    for chapter in ordered {
        match &chapter.volume_id {
            Some(volume_id) => by_volume
                .entry(volume_id.clone())
                .or_default()
                .push(chapter.number),
            None => ungrouped.push(chapter.number),
        }
    }
    let mut groups: Vec<VolumeGroup> = Vec::with_capacity(volumes.len());
    for volume in sort_volumes(volumes) {
        let chapters_in_volume = by_volume.remove(&volume.id).unwrap_or_default();
        groups.push(VolumeGroup {
            volume_id: Some(volume.id),
            title: Some(volume.title),
            chapters: chapters_in_volume,
        });
    }
    if !ungrouped.is_empty() || volumes.is_empty() {
        groups.push(VolumeGroup {
            volume_id: None,
            title: None,
            chapters: ungrouped,
        });
    }
    groups
}

/// 删卷迁移：`removed_volume_id` 归属章的 volume_id 置空（归默认卷）。
/// 返回新索引（未变更行 clone 保形）。
pub fn migrate_chapters_on_volume_delete(
    chapters: &[ChapterMeta],
    removed_volume_id: &str,
) -> Vec<ChapterMeta> {
    chapters
        .iter()
        .map(|chapter| {
            if chapter.volume_id.as_deref() == Some(removed_volume_id) {
                let mut next = chapter.clone();
                next.volume_id = None;
                next
            } else {
                chapter.clone()
            }
        })
        .collect()
}

/// 卷 id 唯一性校验（PUT volumes 全量事务前置守卫）。重复 id → false。
pub fn volume_ids_unique(volumes: &[BookVolume]) -> bool {
    let ids: std::collections::BTreeSet<&str> =
        volumes.iter().map(|volume| volume.id.as_str()).collect();
    ids.len() == volumes.len()
}
