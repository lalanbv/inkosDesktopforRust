import { describe, expect, it } from "vitest";
import { readFile } from "node:fs/promises";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { markdownToSimpleHtml, checkChapterExportSource, ChapterExportSourceError, type ExportSourceMismatchDetails } from "../interaction/export-artifact.js";

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

/**
 * 1681 号：导出源一致性校验共享 golden 差分——engine-rs
 * tests/golden_export_source_diff.rs 读同一 export-source-mismatch-vectors.json。
 * golden 的期望值由 TS 生产派生（这里自证：ChapterExportSourceError 用
 * expectedDetails 构造出的 message 必须逐字等于 expectedMessage——若有人改
 * 消息模板或键序，本测试先红，Rust 集成测试跟着红）。
 */
const sourceVectors = JSON.parse(
  await readFile(join(here, "golden", "export-source-mismatch-vectors.json"), "utf-8"),
) as {
  readonly cases: ReadonlyArray<{
    readonly name: string;
    readonly files: ReadonlyArray<string>;
    readonly indexedNumbers: ReadonlyArray<number>;
    readonly expectedDetails: ExportSourceMismatchDetails | null;
    readonly expectedJson: string;
    readonly expectedMessage: string;
  }>;
};

describe("export source-mismatch golden vectors (shared with engine-rs)", () => {
  for (const tc of sourceVectors.cases) {
    it(tc.name, () => {
      const details = checkChapterExportSource(tc.files, tc.indexedNumbers);
      if (tc.expectedDetails === null || !details) {
        // 校验应通过的用例：四类细节全空 ⇒ 返回 undefined。
        expect(tc.expectedDetails).toBeNull();
        expect(details).toBeUndefined();
        return;
      }
      expect(details).toEqual(tc.expectedDetails);
      // 消息模板 + JSON.stringify 键序的生产派生自证（Rust 侧逐字消费这两个串）。
      expect(JSON.stringify(details)).toBe(tc.expectedJson);
      expect(new ChapterExportSourceError(details).message).toBe(tc.expectedMessage);
    });
  }
});
