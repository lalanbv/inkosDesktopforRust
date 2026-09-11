//! G4/339 号：写法引擎资产化 golden 断言（Phase B 批次二首项）。
//!
//! 唯一事实源 = `golden/style-feature-engine-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_style_feature_engine_diff.rs` 读同一文件差分。
//! 六组断言：特征池推导、启停组合、guidance 渲染、试写 prompt、专名泄露检测、
//! 契约形状；另覆盖绑定解析（命中/未命中/截断）。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { StyleProfile } from "../models/style-profile.js";
import {
  applyFeatureSelection,
  buildTrialWritePrompt,
  composeStyleGuidance,
  deriveFeaturePool,
  detectProperNounLeak,
  resolveStyleBinding,
} from "../utils/style-feature-engine.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/style-feature-engine-vectors.json"), "utf-8"),
) as {
  profile: StyleProfile;
  poolIds: string[];
  selection: Array<{ name: string; enabledIds: string[]; disabledIds: string[]; expected: string[] }>;
  guidance: Array<{ name: string; enabledIds: string[]; language: "zh" | "en"; expected: string }>;
  trial: Array<{ name: string; input: { guidance: string; premise: string; sceneBrief: string; targetChars: number; language: "zh" | "en" }; expected: string }>;
  leak: Array<{ name: string; input: { content: string; protectedNames: string[]; minOccurrences: number }; expected: Array<{ name: string; occurrences: number }> }>;
  contract: unknown;
};

describe("style feature engine (G4)", () => {
  it("derives the feature pool deterministically", () => {
    const pool = deriveFeaturePool(vectors.profile);
    expect(pool.map((feature) => feature.id)).toEqual(vectors.poolIds);
    const empty = deriveFeaturePool({
      avgSentenceLength: 10,
      sentenceLengthStdDev: 2,
      avgParagraphLength: 50,
      paragraphLengthRange: { min: 20, max: 80 },
      vocabularyDiversity: 0.5,
      topPatterns: [],
      rhetoricalFeatures: [],
    });
    expect(empty.map((feature) => feature.id)).toEqual([
      "metric:sentence-length",
      "metric:paragraph-length",
      "metric:vocabulary-diversity",
    ]);
  });

  it("applies whitelist-over-blacklist selection per shared vectors", () => {
    const pool = deriveFeaturePool(vectors.profile);
    for (const vector of vectors.selection) {
      const got = applyFeatureSelection(pool, vector.enabledIds, vector.disabledIds);
      expect(got.map((feature) => feature.id), vector.name).toEqual(vector.expected);
    }
  });

  it("renders bilingual guidance per shared vectors", () => {
    const pool = deriveFeaturePool(vectors.profile);
    for (const vector of vectors.guidance) {
      const enabled = applyFeatureSelection(pool, vector.enabledIds, []);
      expect(composeStyleGuidance(enabled, vector.language), vector.name).toBe(vector.expected);
    }
  });

  it("resolves book bindings (hit / miss / truncation)", () => {
    const profiles = [vectors.profile as unknown as Record<string, unknown>];
    const hit = resolveStyleBinding(profiles, {
      profileName: "参考A",
      enabledIds: ["pattern:对仗"],
      maxGuidanceChars: 10_000,
    });
    expect(hit?.profileName).toBe("参考A");
    expect(hit?.poolSize).toBe(6);
    expect(hit?.enabled.map((feature) => feature.id)).toEqual(["pattern:对仗"]);
    expect(resolveStyleBinding(profiles, { profileName: "不存在" })).toBeNull();
    const truncated = resolveStyleBinding(profiles, {
      profileName: "参考A",
      maxGuidanceChars: 30,
    });
    expect(truncated?.guidanceZh.length).toBeLessThanOrEqual(30);
    expect(truncated?.guidanceZh.endsWith("…")).toBe(true);
  });

  it("builds trial-write prompts per shared vectors", () => {
    for (const vector of vectors.trial) {
      expect(buildTrialWritePrompt(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("detects proper-noun leaks per shared vectors", () => {
    for (const vector of vectors.leak) {
      const got = detectProperNounLeak(vector.input);
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual({
      kinds: ["metric", "pattern", "rhetoric"],
      metricFeatureIds: ["metric:sentence-length", "metric:paragraph-length", "metric:vocabulary-diversity"],
      guidanceItemLimit: 12,
      defaults: { minOccurrences: 1 },
    });
  });
});
