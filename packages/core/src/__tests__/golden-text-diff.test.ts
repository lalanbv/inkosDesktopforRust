//! R53/1657 号：章版本比较 diff 纯函数 golden 锚面（TS 生产向量）——
//! 同文/中行改/尾加/头删/旧空/双空/块重写/CRLF 行内容。Rust 侧 =
//! engine-rs/tests/golden_text_diff_diff.rs 同文件差分。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { diffChapterText, diffSummary, type TextDiffLine } from "../utils/text-diff.js";

const here = dirname(fileURLToPath(import.meta.url));
const fixture = JSON.parse(
  readFileSync(resolve(here, "golden/text-diff-vectors.json"), "utf-8"),
) as {
  vectors: Array<{ name: string; oldText: string; newText: string; expected: TextDiffLine[] }>;
  summarySample: { kept: number; removed: number; added: number };
};

describe("diffChapterText golden（R53/1657 号）", () => {
  it.each(fixture.vectors.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    expect([...diffChapterText(vector.oldText, vector.newText)]).toEqual(vector.expected);
  });

  it("diffSummary 汇总", () => {
    expect(fixture.summarySample).toEqual({ kept: 2, removed: 1, added: 1 });
    expect(diffSummary(diffChapterText("a\nb\n", "a\nc\n"))).toEqual({ kept: 2, removed: 1, added: 1 });
  });

  it("golden 非空防空转", () => {
    expect(fixture.vectors.length).toBeGreaterThanOrEqual(8);
  });
});
