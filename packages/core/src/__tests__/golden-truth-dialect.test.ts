//! G7c/332 号：防呆写出方言 golden 断言。
//!
//! 唯一事实源 = `golden/truth-dialect-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_truth_dialect_diff.rs` 读同一文件差分。
//! 五组断言：标量渲染（平铺+危险值引号）、meta 块、块列表、BOM 剥除、契约形状。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  TRUTH_DIALECT_CONTRACT,
  isDangerousTruthValue,
  quoteTruthValue,
  renderFlatMetaBlock,
  renderTruthBlockList,
  renderTruthValue,
  stripUtf8Bom,
} from "../utils/truth-dialect.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/truth-dialect-vectors.json"), "utf-8"),
) as {
  value: Array<{ name: string; input: string; expected: string }>;
  meta: Array<{ name: string; input: ReadonlyArray<readonly [string, string]>; expected: string }>;
  list: Array<{ name: string; input: string[]; emptyMarker: string; expected: string }>;
  bom: Array<{ name: string; input: string; expected: string }>;
  contract: unknown;
};

describe("truth dialect (G7c)", () => {
  it("renders every scalar per shared vectors", () => {
    for (const vector of vectors.value) {
      expect(renderTruthValue(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("keeps quoted escape idempotent at the quoting layer", () => {
    // 引号层只做转义+包裹（入参须已平铺），平铺逻辑归 renderTruthValue。
    expect(quoteTruthValue(`A"B`)).toBe(`"A\\"B"`);
    expect(isDangerousTruthValue("plain")).toBe(false);
  });

  it("renders flat meta blocks per shared vectors", () => {
    for (const vector of vectors.meta) {
      expect(renderFlatMetaBlock(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("renders block lists per shared vectors", () => {
    for (const vector of vectors.list) {
      expect(renderTruthBlockList(vector.input, vector.emptyMarker), vector.name)
        .toBe(vector.expected);
    }
  });

  it("strips leading BOM per shared vectors", () => {
    for (const vector of vectors.bom) {
      expect(stripUtf8Bom(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("freezes the machine-readable dialect contract", () => {
    expect(vectors.contract).toEqual(TRUTH_DIALECT_CONTRACT);
  });
});
