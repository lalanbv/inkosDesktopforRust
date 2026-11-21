//! R49/1644 号：卷元数据纯函数 golden 断言（双端一致性的 TS 锚面）。
//! 三组对齐 1638 号 R49 验收：卷排序 / 默认卷归属（含未知 volumeId 兜底
//! 归默认组） / 删卷迁移（章归「正文」默认卷）；另有卷 id 唯一性守卫组。
//! Rust 侧 = engine-rs/tests/golden_chapter_volumes_diff.rs 读同一文件差分。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  migrateChaptersOnVolumeDelete,
  projectVolumeGroups,
  sortVolumes,
  volumeIdsUnique,
} from "../state/volumes.js";
import type { BookVolume } from "../models/book.js";
import type { ChapterMeta } from "../models/chapter.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/chapter-volumes-vectors.json"), "utf-8"),
) as GoldenVectors;

interface GoldenVectors {
  groups: Array<{
    name: string;
    volumes: BookVolume[];
    chapters: ChapterMeta[];
    expected: ReturnType<typeof projectVolumeGroups>;
  }>;
  sort: Array<{ name: string; volumes: BookVolume[]; expected: BookVolume[] }>;
  migrate: Array<{
    name: string;
    chapters: ChapterMeta[];
    removedVolumeId: string;
    expected: ChapterMeta[];
  }>;
  unique: Array<{ name: string; volumes: BookVolume[]; expected: boolean }>;
}

describe("projectVolumeGroups golden（R49/1644 号）", () => {
  it.each(vectors.groups.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    expect(projectVolumeGroups(vector.volumes, vector.chapters)).toEqual(vector.expected);
  });
});

describe("sortVolumes golden（卷排序组）", () => {
  it.each(vectors.sort.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    expect(sortVolumes(vector.volumes)).toEqual(vector.expected);
  });
});

describe("migrateChaptersOnVolumeDelete golden（删卷迁移组）", () => {
  it.each(vectors.migrate.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    expect(migrateChaptersOnVolumeDelete(vector.chapters, vector.removedVolumeId)).toEqual(
      vector.expected,
    );
  });
});

describe("volumeIdsUnique golden（PUT volumes 前置守卫）", () => {
  it.each(vectors.unique.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    expect(volumeIdsUnique(vector.volumes)).toBe(vector.expected);
  });
});
