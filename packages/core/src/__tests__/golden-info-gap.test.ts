//! G7a/341 号：信息差账本 golden 断言（Phase B 批次二）。
//!
//! 唯一事实源 = `golden/info-gap-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_info_gap_diff.rs` 读同一文件差分。
//! 六组断言：账本解析、渲染、泄密机检、废笔机检、审计注入文本、契约形状。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  detectReaderRedundancy,
  detectSecretLeaks,
  parseInfoGapsMarkdown,
  renderInfoGapAuditNotes,
  renderInfoGapsMarkdown,
  type InfoGapEntry,
} from "../utils/info-gap-ledger.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/info-gap-vectors.json"), "utf-8"),
) as {
  parse: Array<{ name: string; input: string; expected: InfoGapEntry[] }>;
  render: Array<{ name: string; input: InfoGapEntry[]; expected: string }>;
  leaks: Array<{
    name: string;
    input: { currentChapter: number; content: string; gaps: InfoGapEntry[] };
    expected: Array<Record<string, unknown>>;
  }>;
  redundancy: Array<{
    name: string;
    input: { content: string; gaps: InfoGapEntry[] };
    expected: Array<Record<string, unknown>>;
  }>;
  auditNotes: Array<{
    name: string;
    input: {
      leaks: Array<Record<string, unknown>>;
      redundancies: Array<Record<string, unknown>>;
      language: "zh" | "en";
    };
    expected: string | null;
  }>;
  contract: unknown;
};

describe("info gap ledger (G7a)", () => {
  it("parses author-editable info-gap markdown per shared vectors", () => {
    for (const vector of vectors.parse) {
      expect(parseInfoGapsMarkdown(vector.input), vector.name).toEqual(vector.expected);
    }
  });

  it("renders the ledger round-trip per shared vectors", () => {
    for (const vector of vectors.render) {
      expect(renderInfoGapsMarkdown(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("scans secret leaks per shared vectors", () => {
    for (const vector of vectors.leaks) {
      const got = detectSecretLeaks({
        content: vector.input.content,
        gaps: vector.input.gaps,
        currentChapter: vector.input.currentChapter,
      });
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("scans reader redundancy per shared vectors", () => {
    for (const vector of vectors.redundancy) {
      const got = detectReaderRedundancy({
        content: vector.input.content,
        gaps: vector.input.gaps,
      });
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("renders audit notes per shared vectors", () => {
    for (const vector of vectors.auditNotes) {
      const got = renderInfoGapAuditNotes({
        leaks: vector.input.leaks as never,
        redundancies: vector.input.redundancies as never,
        language: vector.input.language,
      }) ?? null;
      expect(got, vector.name).toBe(vector.expected);
    }
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual({
      dimensions: [
        { id: 38, zh: "泄密机检", en: "Secret Leak Check" },
        { id: 39, zh: "废笔机检", en: "Reader Redundancy Check" },
      ],
      truthFile: "story/info_gaps.md",
      dialogueWindow: 24,
    });
  });
});
