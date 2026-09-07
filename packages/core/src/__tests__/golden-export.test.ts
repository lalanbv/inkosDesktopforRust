import { describe, expect, it } from "vitest";
import { readFile } from "node:fs/promises";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { markdownToSimpleHtml } from "../interaction/export-artifact.js";

/**
 * 214 号：导出面 markdown→simple html 共享 golden 差分——engine-rs
 * tests/golden_export_diff.rs 读同一 export-vectors.json（期望值以 TS 语义
 * 为基准，Rust 漂移即红即修）。
 */
const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  await readFile(join(here, "golden", "export-vectors.json"), "utf-8"),
) as {
  readonly cases: ReadonlyArray<{
    readonly name: string;
    readonly markdown: string;
    readonly expectedTitle: string;
    readonly expectedHtml: string;
  }>;
};

describe("export markdown→html golden vectors (shared with engine-rs)", () => {
  for (const tc of vectors.cases) {
    it(tc.name, () => {
      const got = markdownToSimpleHtml(tc.markdown);
      expect(got.title).toBe(tc.expectedTitle);
      expect(got.html).toBe(tc.expectedHtml);
    });
  }
});
