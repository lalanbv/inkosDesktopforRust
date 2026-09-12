//! R1/357 号：读者体验合同 golden 断言。
//!
//! 唯一事实源 = `golden/reader-experience-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_reader_experience_diff.rs` 读同一文件差分。
//! 五组断言：memo 解析（生成/兼容/截断）、叙述块注入（writer/修稿共用）、
//! 审稿维度 40 激活、planner 提示词合同面、writer 备忘对齐合同面。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { parseMemo } from "../utils/chapter-memo-parser.js";
import { renderMemoAsNarrativeBlock } from "../utils/narrative-control.js";
import { buildDimensionList } from "../agents/continuity.js";
import { buildWriterSystemPrompt } from "../agents/writer-prompts.js";
import { getPlannerMemoSystemPrompt } from "../agents/planner-prompts.js";
import type { ChapterMemo } from "../models/input-governance.js";
import type { GenreProfile } from "../models/genre-profile.js";
import type { BookConfig } from "../models/book.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/reader-experience-vectors.json"), "utf-8"),
) as {
  parse: Array<{ name: string; input: string; expected: ChapterMemo }>;
  narrative: Array<{ name: string; memo: ChapterMemo; expected: string; language: "zh" | "en" }>;
  dimensions: Array<{
    name: string;
    auditDimensions: number[];
    hasReaderExperience: boolean;
    expectedIds: number[];
  }>;
  plannerPrompt: { zh: string[]; en: string[] };
  writerContract: { zh: string[]; en: string[] };
};

const baseGp: GenreProfile = {
  name: "都市异能",
  id: "urban",
  language: "zh",
  chapterTypes: [],
  fatigueWords: [],
  numericalSystem: false,
  powerScaling: false,
  eraResearch: false,
  pacingRule: "",
  satisfactionTypes: [],
  auditDimensions: [],
};

describe("reader experience contract (R1)", () => {
  it("parses memo with/without the contract per shared vectors", () => {
    for (const vector of vectors.parse) {
      const got = parseMemo(vector.input, 12, false);
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("renders the contract as a top-level narrative block per shared vectors", () => {
    for (const vector of vectors.narrative) {
      const got = renderMemoAsNarrativeBlock(vector.memo, undefined, vector.language);
      expect(got, vector.name).toBe(vector.expected);
    }
  });

  it("activates audit dimension 40 only when the contract is present", () => {
    for (const vector of vectors.dimensions) {
      const dims = buildDimensionList(
        { ...baseGp, auditDimensions: vector.auditDimensions },
        null,
        "zh",
        false,
        undefined,
        false,
        vector.hasReaderExperience,
      );
      expect(dims.map((d) => d.id), vector.name).toEqual(vector.expectedIds);
    }
  });

  it("planner memo prompts carry the contract section spec", () => {
    expect(getPlannerMemoSystemPrompt("zh")).toContain(vectors.plannerPrompt.zh[0]);
    for (const marker of vectors.plannerPrompt.zh) {
      expect(getPlannerMemoSystemPrompt("zh"), marker).toContain(marker);
    }
    for (const marker of vectors.plannerPrompt.en) {
      expect(getPlannerMemoSystemPrompt("en"), marker).toContain(marker);
    }
  });

  it("writer memo-alignment contract maps the new section for both languages", () => {
    const book: BookConfig = {
      id: "prompt-book",
      title: "Prompt Book",
      platform: "tomato",
      genre: "other",
      status: "active",
      targetChapters: 20,
      chapterWordCount: 3000,
      createdAt: "2026-03-22T00:00:00.000Z",
      updatedAt: "2026-03-22T00:00:00.000Z",
    };
    const gp = { ...baseGp };
    for (const marker of vectors.writerContract.zh) {
      expect(buildWriterSystemPrompt(book, gp, null, "", "", "", undefined, 12, "full", undefined, "zh", "governed"), marker).toContain(marker);
    }
    for (const marker of vectors.writerContract.en) {
      expect(buildWriterSystemPrompt(book, gp, null, "", "", "", undefined, 12, "full", undefined, "en", "governed"), marker).toContain(marker);
    }
  });
});
