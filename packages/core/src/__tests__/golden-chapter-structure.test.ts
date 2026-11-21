//! R48a/1641 号：章节结构纯函数 golden 断言（双端一致性的 TS 锚面）。
//!
//! 唯一事实源 = `golden/chapter-structure-vectors.json`（TS 生产函数导出）；
//! Rust 侧 `engine-rs/tests/golden_chapter_structure_diff.rs` 读同一文件差分。
//! 覆盖四组：revision 章集合指纹（sha256 前 16 hex，重编号/拆分边界/合并分隔
//! 之外的第四组——双端 JSON 序列化逐位一致是乐观并发可比的前提）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  chapterSetRevision,
  mergeContent,
  reorderPlan,
  renumberAfterMerge,
  renumberAfterSplit,
  splitUtf16,
  type ChapterStructureError,
} from "../state/chapter-structure.js";
import type { ChapterMeta } from "../models/chapter.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/chapter-structure-vectors.json"), "utf-8"),
) as GoldenVectors;

interface GoldenVectors {
  revision: Array<{
    name: string;
    index: Array<{ number: number; wordCount: number; updatedAt: string }>;
    expected: string;
  }>;
  renumber: {
    split: Array<{ name: string; numbers: number[]; at: number; expected: Array<{ from: number; to: number }> }>;
    merge: Array<{ name: string; numbers: number[]; removed: number; expected: Array<{ from: number; to: number }> }>;
    reorder: Array<{ name: string; numbers: number[]; order: number[]; invalid?: boolean; expected: Array<{ from: number; to: number }> | null }>;
  };
  split: Array<{
    name: string;
    content: string;
    offset: number;
    invalid?: boolean;
    expected: { head: string; tail: string } | null;
  }>;
  merge: Array<{ name: string; a: string; b: string; separator: string; expected: string }>;
}

const asIndex = (rows: GoldenVectors["revision"][number]["index"]): ReadonlyArray<ChapterMeta> =>
  rows.map((row) => ({
    ...row,
    title: "",
    status: "ready-for-review",
    createdAt: "2026-11-21T00:00:00.000Z",
    auditIssues: [],
    lengthWarnings: [],
  })) as ReadonlyArray<ChapterMeta>;

describe("chapterSetRevision golden（R48a/1641 号）", () => {
  it.each(vectors.revision.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    expect(chapterSetRevision(asIndex(vector.index))).toBe(vector.expected);
  });
});

describe("renumber golden（重编号组）", () => {
  it.each(vectors.renumber.split.map((v) => [v.name, v] as const))("split %s", (_name, vector) => {
    expect(renumberAfterSplit(vector.numbers, vector.at)).toEqual(vector.expected);
  });
  it.each(vectors.renumber.merge.map((v) => [v.name, v] as const))("merge %s", (_name, vector) => {
    expect(renumberAfterMerge(vector.numbers, vector.removed)).toEqual(vector.expected);
  });
  it.each(vectors.renumber.reorder.map((v) => [v.name, v] as const))("reorder %s", (_name, vector) => {
    if (vector.invalid || vector.expected === null) {
      expect(() => reorderPlan(vector.numbers, vector.order)).toThrow();
      return;
    }
    expect(reorderPlan(vector.numbers, vector.order)).toEqual(vector.expected);
  });
});

describe("splitUtf16 golden（拆分边界组）", () => {
  it.each(vectors.split.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    if (vector.invalid || vector.expected === null) {
      expect(() => splitUtf16(vector.content, vector.offset)).toThrow();
      return;
    }
    const [head, tail] = splitUtf16(vector.content, vector.offset);
    expect({ head, tail }).toEqual(vector.expected);
  });
});

describe("mergeContent golden（合并分隔组）", () => {
  it.each(vectors.merge.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    expect(mergeContent(vector.a, vector.b, vector.separator)).toBe(vector.expected);
  });
});

describe("golden 非空防空转", () => {
  it("四组向量齐全", () => {
    expect(vectors.revision.length).toBeGreaterThan(0);
    expect(vectors.renumber.split.length).toBeGreaterThan(0);
    expect(vectors.split.length).toBeGreaterThan(0);
    expect(vectors.merge.length).toBeGreaterThan(0);
  });
});
