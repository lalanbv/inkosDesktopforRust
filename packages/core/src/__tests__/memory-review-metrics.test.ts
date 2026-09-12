//! R3/361 号：MemoryDB review_metrics 表幂等沉淀单测。
import { mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MemoryDB, type StoredReviewMetric } from "../state/memory-db.js";
import { reviewMetricContentHash } from "../utils/quality-trend.js";

let bookDir: string;

beforeEach(() => {
  bookDir = join(tmpdir(), `inkos-review-metrics-${Date.now()}-${Math.random().toString(36).slice(2)}`);
  mkdirSync(join(bookDir, "story"), { recursive: true });
});

afterEach(() => {
  rmSync(bookDir, { recursive: true, force: true });
});

function metric(chapter: number, score: number | undefined, hashSalt = ""): StoredReviewMetric {
  const content = {
    chapter,
    ...(typeof score === "number" ? { overallScore: score } : {}),
    passed: true,
    criticalCount: 0,
    warningCount: 1,
    infoCount: 0,
  };
  return {
    ...content,
    contentHash: reviewMetricContentHash(content) + hashSalt,
    recordedAt: "2026-09-12T00:00:00.000Z",
  };
}

describe("MemoryDB review metrics (R3)", () => {
  it("records idempotently by content hash and overwrites on revision", () => {
    const memory = new MemoryDB(bookDir);
    expect(memory.recordReviewMetric(metric(7, 82))).toBe(true);
    // 同章同内容（同 hash）重放 → 幂等跳过。
    expect(memory.recordReviewMetric(metric(7, 82))).toBe(false);
    // 修订后内容更新（hash 不同）→ 覆盖。
    expect(memory.recordReviewMetric(metric(7, 65))).toBe(true);

    const rows = memory.listReviewMetrics();
    expect(rows).toHaveLength(1);
    expect(rows[0]!.chapter).toBe(7);
    expect(rows[0]!.overallScore).toBe(65);
  });

  it("lists metrics in chapter order with unscored rows kept", () => {
    const memory = new MemoryDB(bookDir);
    memory.recordReviewMetric(metric(3, 90));
    memory.recordReviewMetric(metric(1, undefined));
    memory.recordReviewMetric(metric(2, 75));

    const rows = memory.listReviewMetrics();
    expect(rows.map((row) => row.chapter)).toEqual([1, 2, 3]);
    expect(rows[0]!.overallScore).toBeUndefined();
    expect(rows[1]!.overallScore).toBe(75);
  });
});
