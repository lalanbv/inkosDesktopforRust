//! R2/358 号：张力曲线 golden 断言。
//!
//! 唯一事实源 = `golden/tension-curve-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_tension_curve_diff.rs` 读同一文件差分。
//! 三组断言：TENSION_METRICS 节解析（JSON/行式/clamp/缺字段）、
//! 章级点列聚合（缺分跳过+升序）、启发式告警（平坦/高潮拥挤/弱钩连击/健康）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  buildTensionCurve,
  detectTensionWarnings,
  parseTensionMetrics,
} from "../utils/tension-curve.js";
import type { TensionPoint, TensionWarning } from "../utils/tension-curve.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/tension-curve-vectors.json"), "utf-8"),
) as {
  parse: Array<{ name: string; input: string; expected: { conflictLevel: number; revealLevel: number } | null }>;
  curve: Array<{
    name: string;
    input: Array<{ chapter: number; conflictLevel?: number; revealLevel?: number }>;
    expected: { points: TensionPoint[]; scoredChapters: number; unscoredChapters: number };
  }>;
  warnings: Array<{
    name: string;
    language: "zh" | "en";
    input: TensionPoint[];
    expected: TensionWarning[];
  }>;
};

describe("tension curve contract (R2)", () => {
  it("parses TENSION_METRICS sections per shared vectors", () => {
    for (const vector of vectors.parse) {
      expect(parseTensionMetrics(vector.input) ?? null, vector.name).toEqual(vector.expected);
    }
  });

  it("builds curves skipping unscored rows per shared vectors", () => {
    for (const vector of vectors.curve) {
      expect(buildTensionCurve(vector.input), vector.name).toEqual(vector.expected);
    }
  });

  it("detects heuristic warnings per shared vectors", () => {
    for (const vector of vectors.warnings) {
      expect(
        detectTensionWarnings(vector.input, vector.language),
        vector.name,
      ).toEqual(vector.expected);
    }
  });
});
