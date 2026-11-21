/**
 * 卷/幕元数据纯函数（R49，1644 号）。
 *
 * Rust 镜像 = engine-rs/src/state/volumes.rs（golden chapter-volumes-vectors.json
 * 锁双端：卷排序/默认卷归属/删卷迁移三组）。
 *
 * 组织面语义备案：卷是 book.json.writing.volumes 有序列表；章归属 =
 * ChapterMeta.volumeId（可选键）；**volumeId=null = 未分组归「正文」默认卷**
 * （投影默认组 title=null，文案由 UI 层本地化——纯函数不持 UI 文案）；
 * 卷不进装配注入（组织面非上下文层，1638 号规格红线）。
 */

import type { BookVolume } from "../models/book.js";
import type { ChapterMeta } from "../models/chapter.js";

/** 卷排序：order 升序（双端同 comparator）。 */
export function sortVolumes(volumes: ReadonlyArray<BookVolume>): BookVolume[] {
  return [...volumes].sort((a, b) => a.order - b.order);
}

/** 分组投影：卷按 order 升序各一组；未分组章归默认卷（volumeId=null、
 * title=null 由 UI 本地化「正文」）殿后；组内章按 number 升序。 */
export interface VolumeGroup {
  readonly volumeId: string | null;
  readonly title: string | null;
  readonly chapters: ReadonlyArray<number>;
}

export function projectVolumeGroups(
  volumes: ReadonlyArray<BookVolume>,
  chapters: ReadonlyArray<Pick<ChapterMeta, "number" | "volumeId">>,
): VolumeGroup[] {
  const byVolume = new Map<string, number[]>();
  const ungrouped: number[] = [];
  for (const chapter of [...chapters].sort((a, b) => a.number - b.number)) {
    if (chapter.volumeId) {
      const bucket = byVolume.get(chapter.volumeId);
      if (bucket) bucket.push(chapter.number);
      else byVolume.set(chapter.volumeId, [chapter.number]);
    } else {
      ungrouped.push(chapter.number);
    }
  }
  const groups: VolumeGroup[] = sortVolumes(volumes)
    .map((volume) => ({
      volumeId: volume.id,
      title: volume.title,
      chapters: byVolume.get(volume.id) ?? [],
    }));
  if (ungrouped.length > 0 || volumes.length === 0) {
    groups.push({ volumeId: null, title: null, chapters: ungrouped });
  }
  return groups;
}

/** 删卷迁移：`removedVolumeId` 归属章的 volumeId 置空（归默认卷）。
 * 返回新索引（未变更行保持引用语义不变——只拷贝被迁移行）。 */
export function migrateChaptersOnVolumeDelete(
  chapters: ReadonlyArray<ChapterMeta>,
  removedVolumeId: string,
): ChapterMeta[] {
  return chapters.map((chapter) =>
    chapter.volumeId === removedVolumeId
      ? { ...chapter, volumeId: undefined }
      : chapter,
  );
}

/** 卷 id 唯一性校验（PUT volumes 全量事务前置守卫）。重复 id → false。 */
export function volumeIdsUnique(volumes: ReadonlyArray<BookVolume>): boolean {
  const ids = new Set(volumes.map((volume) => volume.id));
  return ids.size === volumes.length;
}
