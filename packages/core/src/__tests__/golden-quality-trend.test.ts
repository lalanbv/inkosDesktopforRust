//! R3/360 号：质量趋势 + 回灌幂等 + 承诺紧迫度 golden 断言。
//!
//! 唯一事实源 = `golden/quality-trend-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_quality_trend_diff.rs` 读同一文件差分。
//! 三组断言：趋势聚合（升序/缺分不进均值/一位小数）、幂等指纹
//!（已知 FNV 锚定值 + 重放去重）、紧迫度（WNW 映射/置信度/目标章窗）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  buildQualityTrend,
  dedupeReplayRows,
  reviewMetricContentHash,
  type QualityTrend,
  type ReviewMetricRow,
} from "../utils/quality-trend.js";
import { resolvePromiseUrgency } from "../utils/promise-ledger.js";
import type { StoredHook } from "../state/memory-db.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/quality-trend-vectors.json"), "utf-8"),
) as {
  trend: Array<{ name: string; input: Array<Record<string, unknown>>; expected: QualityTrend }>;
  replay: Array<
    | {
      name: string;
      hashes: Array<{ input: Omit<ReviewMetricRow, "recordedAt">; expected: string }>;
    }
    | {
      name: string;
      input: Array<Record<string, unknown>>;
      expected: { freshChapters: number[]; skipped: number };
    }
  >;
  urgency: Array<{
    name: string;
    currentChapter: number;
    targetChapters: number | null;
    hook: Partial<StoredHook> & { hookId: string };
    expected: Record<string, unknown>;
  }>;
};

function asRow(raw: Record<string, unknown>): ReviewMetricRow {
  return {
    chapter: raw.chapter as number,
    ...(typeof raw.overallScore === "number" ? { overallScore: raw.overallScore as number } : {}),
    passed: raw.passed as boolean,
    criticalCount: raw.criticalCount as number,
    warningCount: raw.warningCount as number,
    infoCount: raw.infoCount as number,
    recordedAt: (raw.recordedAt as string) ?? "",
  };
}

/** golden 期望的 null（缺分）映射回 undefined 以对齐 TS 可选字段语义。 */
function normalizeTrend(expected: QualityTrend): QualityTrend {
  return {
    ...expected,
    averageScore: expected.averageScore ?? undefined,
    points: expected.points.map((point) => ({
      ...point,
      overallScore: point.overallScore ?? undefined,
    })),
  };
}

describe("quality trend contract (R3)", () => {
  it("aggregates review metrics per shared vectors", () => {
    for (const vector of vectors.trend) {
      const got = buildQualityTrend(vector.input.map(asRow));
      expect(got, vector.name).toEqual(normalizeTrend(vector.expected));
    }
  });

  it("hashes and dedupes replay rows per shared vectors", () => {
    for (const vector of vectors.replay) {
      if ("hashes" in vector) {
        for (const hash of vector.hashes) {
          expect(reviewMetricContentHash(hash.input), vector.name).toBe(hash.expected);
        }
        continue;
      }
      const { fresh, skipped } = dedupeReplayRows(vector.input.map(asRow));
      expect(fresh.map((row) => row.chapter), vector.name).toEqual(vector.expected.freshChapters);
      expect(skipped, vector.name).toBe(vector.expected.skipped);
    }
  });

  it("resolves promise urgency per shared vectors", () => {
    for (const vector of vectors.urgency) {
      const hook = {
        hookId: vector.hook.hookId,
        startChapter: vector.hook.startChapter ?? 0,
        type: vector.hook.type ?? "unspecified",
        status: vector.hook.status ?? "open",
        lastAdvancedChapter: vector.hook.lastAdvancedChapter ?? 0,
        expectedPayoff: vector.hook.expectedPayoff ?? "",
        payoffTiming: vector.hook.payoffTiming,
        notes: vector.hook.notes ?? "",
      } as StoredHook;
      const got = resolvePromiseUrgency({
        hook,
        currentChapter: vector.currentChapter,
        ...(vector.targetChapters !== null ? { targetChapters: vector.targetChapters } : {}),
      });
      expect(got, vector.name).toEqual(vector.expected);
    }
  });
});
