//! R50/1645 号：md-zip 条目路径构造 golden 断言（双端一致性的 TS 锚面）。
//! 覆盖：卷目录（order 补零+title）、未分组回退、未知 volumeId 兜底、
//! 路径分隔符 sanitize、空 title。Rust 侧 =
//! engine-rs/tests/golden_chapter_mdzip_diff.rs 读同一文件差分。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { mdZipEntryPath } from "../interaction/export-artifact.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/chapter-mdzip-vectors.json"), "utf-8"),
) as {
  cases: Array<{
    name: string;
    volumeId: string | undefined;
    volumes: Array<{ id: string; title: string; order: number }>;
    chapterNumber: number;
    chapterTitle: string;
    expected: string;
  }>;
};

describe("mdZipEntryPath golden（R50/1645 号）", () => {
  it.each(vectors.cases.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    expect(
      mdZipEntryPath(vector.volumeId, vector.volumes, vector.chapterNumber, vector.chapterTitle),
    ).toBe(vector.expected);
  });

  it("golden 非空防空转", () => {
    expect(vectors.cases.length).toBeGreaterThan(0);
  });
});
