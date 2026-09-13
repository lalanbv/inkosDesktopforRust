//! R20/389 号：系列正典共享 golden 断言。
//!
//! 唯一事实源 = `golden/series-canon-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_series_canon_diff.rs` 读同一文件差分。
//! 四组断言：seriesId 规范、文件解析（整包校验/坏条目跳过/截断/排序）、
//! 双层合并（book 同名覆盖）、注入渲染（双语/空）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { EntityCodexCard } from "../utils/entity-codex.js";
import {
  isValidSeriesId,
  mergeCodexLayers,
  parseSeriesCanonFile,
  renderSeriesCodexBlock,
} from "../utils/series-canon.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/series-canon-vectors.json"), "utf-8"),
) as {
  seriesId: Array<{ name: string; seriesId: string; expected: boolean }>;
  parse: Array<{ name: string; raw: unknown; expected: unknown }>;
  merge: Array<{
    name: string;
    book: EntityCodexCard[];
    series: EntityCodexCard[];
    expected: { bookNames: string[]; series: EntityCodexCard[] };
  }>;
  render: Array<{
    name: string;
    language: "zh" | "en";
    matches: Array<{ card: EntityCodexCard; hits: number }>;
    expected: string | null;
  }>;
};

describe("series canon contract (R20)", () => {
  it("validates series ids per shared vectors", () => {
    for (const vector of vectors.seriesId) {
      expect(isValidSeriesId(vector.seriesId), vector.name).toBe(vector.expected);
    }
  });

  it("parses series canon files per shared vectors", () => {
    for (const vector of vectors.parse) {
      const got = parseSeriesCanonFile(vector.raw);
      expect(got ?? null, vector.name).toEqual(vector.expected);
    }
  });

  it("merges codex layers per shared vectors", () => {
    for (const vector of vectors.merge) {
      const got = mergeCodexLayers(vector.book, vector.series);
      expect(
        { bookNames: got.book.map((card) => card.name), series: got.series },
        vector.name,
      ).toEqual(vector.expected);
    }
  });

  it("renders series codex blocks per shared vectors", () => {
    for (const vector of vectors.render) {
      const got = renderSeriesCodexBlock(vector.matches, vector.language);
      expect(got ?? null, vector.name).toEqual(vector.expected);
    }
  });
});
