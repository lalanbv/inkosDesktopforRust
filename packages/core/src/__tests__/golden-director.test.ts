//! G6/352 号：自动导演 golden 断言（Phase C 末件首批）。
//!
//! 唯一事实源 = `golden/director-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_director_diff.rs` 读同一文件差分。
//! 五组断言：方向候选解析（含标题组重做剔除）、运行模式计划、阶段推进决策表、
//! 导演 prompt 构建、契约形状。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  DIRECTOR_CONTRACT,
  DIRECTOR_RUN_MODES,
  DIRECTOR_STAGES,
  buildDirectionCandidatesPrompt,
  nextDirectorStage,
  parseDirectionCandidates,
  resolveDirectorRunPlan,
  type DirectorRunMode,
  type DirectorStage,
} from "../models/director.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/director-vectors.json"), "utf-8"),
) as {
  candidates: Array<{
    name: string;
    input: { content: string; excludeTitles: string[] };
    expected: Array<{ id?: string; title: string; hook?: string; genre?: string; synopsis?: string; differentiator?: string; confidence: number }>;
  }>;
  runPlan: Array<{
    name: string;
    input: { mode: DirectorRunMode; fromChapter?: number; toChapter?: number | null; targetChapters: number };
    expected: { mode: string; fromChapter: number; toChapter: number | null; stopAfterDirections: boolean };
  }>;
  stages: Array<{
    name: string;
    input: { current: DirectorStage; mode: DirectorRunMode; writtenChapters: number; toChapter: number };
    expected: DirectorStage;
  }>;
  contract: unknown;
};

describe("director (G6)", () => {
  it("parses direction candidates per shared vectors", () => {
    for (const vector of vectors.candidates) {
      const got = parseDirectionCandidates(vector.input.content, vector.input.excludeTitles);
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("builds director prompts with inspiration and exclusion block", () => {
    const prompt = buildDirectionCandidatesPrompt({
      inspiration: { premise: "末世废土拾荒少年", genre: "科幻", keywords: ["废土", "怀表"] },
      count: 3,
      excludeTitles: ["旧书"],
      language: "zh",
    });
    expect(prompt).toContain("生成 3 套并列的开书方向");
    expect(prompt).toContain("末世废土拾荒少年");
    expect(prompt).toContain("已排除标题（不得复用）：旧书");
    const en = buildDirectionCandidatesPrompt({
      inspiration: { premise: "x", keywords: [] },
      count: 2,
      language: "en",
    });
    expect(en).toContain("Generate 2 alternative book directions");
    expect(en).not.toContain("Excluded titles");
  });

  it("resolves run plans per shared vectors", () => {
    for (const vector of vectors.runPlan) {
      const got = resolveDirectorRunPlan({
        mode: vector.input.mode,
        fromChapter: vector.input.fromChapter,
        toChapter: vector.input.toChapter ?? undefined,
        targetChapters: vector.input.targetChapters,
      });
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("advances cockpit stages per shared vectors", () => {
    for (const vector of vectors.stages) {
      expect(nextDirectorStage(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual(DIRECTOR_CONTRACT);
    expect(DIRECTOR_RUN_MODES).toHaveLength(3);
    expect(DIRECTOR_STAGES).toHaveLength(6);
  });
});
