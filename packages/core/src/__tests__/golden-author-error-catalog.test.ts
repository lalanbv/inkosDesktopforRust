//! R7/368 号：结构化错误目录 golden 断言。
//!
//! 唯一事实源 = `golden/author-error-catalog-vectors.json`（行为）+
//! `data/author-errors.json`（数据）；Rust 侧
//! `engine-rs/tests/golden_author_error_catalog_diff.rs` include_str! 同两
//! 文件差分。三组断言：resolve（busy/llm/未知兜底/双语）、nextActionFor、
//! 目录数据契约（codes/count）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  loadAuthorErrorCatalog,
  nextActionFor,
  resolveAuthorError,
} from "../utils/author-error-catalog.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/author-error-catalog-vectors.json"), "utf-8"),
) as {
  resolve: Array<{
    name: string;
    event: string;
    message: string;
    language: "zh" | "en";
    expected: { code: string; severity: string; message: string; nextAction: string };
  }>;
  nextActionFor: Array<{ severity: string; language: "zh" | "en"; expected: string }>;
  catalog: { codes: string[]; count: number };
};

describe("author error catalog contract (R7)", () => {
  it("resolves errors per shared vectors", () => {
    for (const vector of vectors.resolve) {
      const got = resolveAuthorError(vector.event, vector.message, vector.language);
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("maps severities to next actions per shared vectors", () => {
    for (const vector of vectors.nextActionFor) {
      expect(
        nextActionFor(vector.severity as never, vector.language),
        vector.severity,
      ).toBe(vector.expected);
    }
  });

  it("ships the catalog matching the data contract", () => {
    const catalog = loadAuthorErrorCatalog();
    expect(catalog.errors).toHaveLength(vectors.catalog.count);
    expect(catalog.errors.map((entry) => entry.code).sort()).toEqual([...vectors.catalog.codes].sort());
  });
});
