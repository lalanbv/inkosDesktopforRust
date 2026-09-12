//! R11/378 号：场景节拍契约 golden 断言。
//!
//! 唯一事实源 = `golden/scene-beats-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_scene_beats_diff.rs` 读同一文件差分。
//! 三组断言：planner JSON 解析（clamp/无效 undefined）、提示词锚点
//!（zh/en）、writer 注入块（双语+纪律行）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  buildSceneBeatsPrompt,
  buildSceneBeatsWriterBlock,
  parseSceneBeatPlan,
  type SceneBeatPlan,
} from "../utils/scene-beats.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/scene-beats-vectors.json"), "utf-8"),
) as {
  parse: Array<{
    name: string;
    chapter: number;
    input: string;
    expected: SceneBeatPlan | null;
  }>;
  prompt: Array<{
    name: string;
    language: "zh" | "en";
    goal: string;
    outlineNode: string | null;
    sceneCount: number;
    expectedContains: string[];
  }>;
  writerBlock: Array<{
    name: string;
    language: "zh" | "en";
    plan: SceneBeatPlan;
    expected: string;
  }>;
};

describe("scene beats contract (R11)", () => {
  it("parses planner beat plans per shared vectors", () => {
    for (const vector of vectors.parse) {
      const got = parseSceneBeatPlan(vector.input, vector.chapter);
      expect(got ?? null, vector.name).toEqual(vector.expected);
    }
  });

  it("builds planner prompts per shared vectors", () => {
    for (const vector of vectors.prompt) {
      const prompt = buildSceneBeatsPrompt({
        goal: vector.goal,
        ...(vector.outlineNode ? { outlineNode: vector.outlineNode } : {}),
        sceneCount: vector.sceneCount,
        language: vector.language,
      });
      for (const anchor of vector.expectedContains) {
        expect(prompt, vector.name).toContain(anchor);
      }
    }
  });

  it("builds writer blocks per shared vectors", () => {
    for (const vector of vectors.writerBlock) {
      expect(
        buildSceneBeatsWriterBlock(vector.plan, vector.language),
        vector.name,
      ).toBe(vector.expected);
    }
  });
});
