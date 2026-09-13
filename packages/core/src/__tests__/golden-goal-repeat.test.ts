//! 399 号：章节目标防复读门 golden 断言（v5 五轮规划 P1）。
//!
//! 唯一事实源 = `golden/goal-repeat-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_goal_repeat_diff.rs` 读同一文件差分。
//! 三组断言：归一化、相似度（4 位小数 floor）、复读判定。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  GOAL_REPEAT_SIMILARITY_THRESHOLD,
  GOAL_REPEAT_WINDOW,
  evaluateGoalRepeat,
  goalRepeatSimilarity,
  normalizeGoalRepeatText,
} from "../utils/goal-repeat-gate.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/goal-repeat-vectors.json"), "utf-8"),
) as {
  normalize: Array<{ name: string; input: string; expected: string }>;
  similarity: Array<{ name: string; input: { a: string; b: string }; expected: number }>;
  evaluate: Array<{
    name: string;
    input: { goal: string; references: string[]; threshold: number | null };
    expected: { repeat: boolean; maxSimilarity: number; exactMatch: boolean };
  }>;
};

describe("goal repeat gate (R28/399)", () => {
  it("normalizes goal text per shared vectors", () => {
    for (const vector of vectors.normalize) {
      expect(normalizeGoalRepeatText(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("scores bigram dice similarity per shared vectors", () => {
    for (const vector of vectors.similarity) {
      expect(goalRepeatSimilarity(vector.input.a, vector.input.b), vector.name).toBe(
        vector.expected,
      );
    }
  });

  it("evaluates repeat verdicts per shared vectors", () => {
    for (const vector of vectors.evaluate) {
      const got = evaluateGoalRepeat(
        vector.input.goal,
        vector.input.references,
        vector.input.threshold ?? undefined,
      );
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("keeps conservative defaults and sorts threshold/window constants", () => {
    expect(GOAL_REPEAT_SIMILARITY_THRESHOLD).toBe(0.8);
    expect(GOAL_REPEAT_WINDOW).toBe(3);
  });
});
