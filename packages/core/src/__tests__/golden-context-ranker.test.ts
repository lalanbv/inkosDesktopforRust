//! R6/367 号：上下文排序器 + openingHint golden 断言。
//!
//! 唯一事实源 = `golden/context-ranker-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_context_ranker_diff.rs` 读同一文件差分。
//! 三组断言：层内得分排序（权重/并列稳定/自定义权重）、组装组合
//!（层间 precedence + 层内 rank）、上章结尾承接约束块（截断/双语/空）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  buildOpeningHint,
  rankEntriesForComposition,
  rankWithinTier,
  type RankFeatures,
  type RankerWeights,
} from "../utils/context-ranker.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/context-ranker-vectors.json"), "utf-8"),
) as {
  rankWithinTier: Array<{
    name: string;
    weights: RankerWeights | null;
    input: Array<RankFeatures & { source: string }>;
    expected: Array<{ source: string; rankScore: number }>;
  }>;
  composition: Array<{
    name: string;
    weights: RankerWeights | null;
    input: Array<RankFeatures & { source: string }>;
    expectedOrder: string[];
    expectedScores: number[];
  }>;
  openingHint: Array<{
    name: string;
    language: "zh" | "en";
    previousEnding: string;
    maxChars: number;
    expected: string | null;
  }>;
};

describe("context ranker contract (R6)", () => {
  it("ranks within a tier per shared vectors", () => {
    for (const vector of vectors.rankWithinTier) {
      const got = rankWithinTier(vector.input, vector.weights ?? undefined);
      expect(
        got.map((ranked) => ({ source: ranked.entry.source, rankScore: ranked.rankScore })),
        vector.name,
      ).toEqual(vector.expected);
    }
  });

  it("composes tier precedence with intra-tier ranking per shared vectors", () => {
    for (const vector of vectors.composition) {
      const got = rankEntriesForComposition(vector.input, vector.weights ?? undefined);
      expect(got.map((ranked) => ranked.entry.source), vector.name).toEqual(vector.expectedOrder);
      expect(got.map((ranked) => ranked.rankScore), vector.name).toEqual(vector.expectedScores);
    }
  });

  it("builds opening hints per shared vectors", () => {
    for (const vector of vectors.openingHint) {
      const got = buildOpeningHint(vector.previousEnding, vector.language, vector.maxChars);
      expect(got ?? null, vector.name).toEqual(vector.expected);
    }
  });
});
