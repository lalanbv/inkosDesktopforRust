//! G11/371 号：best-of-N 计划与选优 golden 断言。
//!
//! 唯一事实源 = `golden/best-of-n-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_best_of_n_diff.rs` 读同一文件差分。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  resolveBestOfNPlan,
  selectBestCandidate,
  type BestOfNConfig,
} from "../utils/best-of-n.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/best-of-n-vectors.json"), "utf-8"),
) as {
  plan: Array<{
    name: string;
    config: BestOfNConfig;
    firstScore: number | null;
    expected: { enabled: boolean; extraCandidates: number; totalCandidates: number; minScore: number };
  }>;
  select: Array<{
    name: string;
    candidates: Array<{ payload: string; score?: number }>;
    expected: { winner: string; index: number; score?: number };
  }>;
};

describe("best-of-n contract (G11)", () => {
  it("resolves plans per shared vectors", () => {
    for (const vector of vectors.plan) {
      const got = resolveBestOfNPlan(
        vector.config,
        vector.firstScore === null ? undefined : vector.firstScore,
      );
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("selects the best candidate per shared vectors", () => {
    for (const vector of vectors.select) {
      const got = selectBestCandidate(vector.candidates);
      expect(
        { winner: got.winner, index: got.index, score: got.score },
        vector.name,
      ).toEqual(vector.expected);
    }
  });

  it("throws on empty candidates", () => {
    expect(() => selectBestCandidate([])).toThrow();
  });
});
