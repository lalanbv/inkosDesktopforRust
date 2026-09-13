//! G10/338 号：承诺账本运营化 golden 断言（Phase B 批次一收尾）。
//!
//! 唯一事实源 = `golden/promise-ledger-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_promise_ledger_diff.rs` 读同一文件差分。
//! 五组断言：hookActivity 强度、节奏债告警、连续弱钩段、承诺时间线、契约形状。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  DEFAULT_CORE_HOOK_STALLED_THRESHOLD,
  DEFAULT_WEAK_HOOK_MIN_RUN,
  buildPromiseTimeline,
  detectPacingDebts,
  detectWeakHookRuns,
  hookActivityStrength,
  parseExpectedChapter,
} from "../utils/promise-ledger.js";
import { normalizeHookKind } from "../utils/hook-kind.js";
import type { StoredHook } from "../state/memory-db.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/promise-ledger-vectors.json"), "utf-8"),
) as {
  strength: Array<{ name: string; input: string; expected: string }>;
  pacing: Array<{
    name: string;
    input: { currentChapter: number; threshold: number; hooks: Array<Record<string, unknown>> };
    expected: Array<Record<string, unknown>>;
  }>;
  weakRuns: Array<{
    name: string;
    input: { minRunLength: number; summaries: Array<{ chapter: number; hookActivity: string }> };
    expected: { longestRun: number; runs: Array<Record<string, unknown>> };
  }>;
  timeline: Array<{
    name: string;
    input: { currentChapter: number; hooks: Array<Record<string, unknown>> };
    expected: Array<Record<string, unknown>>;
  }>;
  contract: unknown;
};

const asHooks = (rows: Array<Record<string, unknown>>): StoredHook[] =>
  rows.map((row) => {
    const kind = normalizeHookKind(typeof row.kind === "string" ? row.kind : "");
    return {
      hookId: String(row.hookId ?? ""),
      startChapter: Number(row.startChapter ?? 0),
      type: "",
      status: String(row.status ?? "open"),
      lastAdvancedChapter: Number(row.lastAdvancedChapter ?? 0),
      expectedPayoff: String(row.expectedPayoff ?? ""),
      notes: String(row.notes ?? ""),
      coreHook: row.coreHook === true,
      ...(kind ? { kind } : {}),
    };
  }) as StoredHook[];

describe("promise ledger (G10)", () => {
  it("classifies hook activity strength per shared vectors", () => {
    for (const vector of vectors.strength) {
      expect(hookActivityStrength(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("alerts only on stalled core hooks per shared vectors", () => {
    for (const vector of vectors.pacing) {
      const got = detectPacingDebts({
        hooks: asHooks(vector.input.hooks),
        currentChapter: vector.input.currentChapter,
        threshold: vector.input.threshold,
      });
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("detects consecutive weak-hook runs per shared vectors", () => {
    for (const vector of vectors.weakRuns) {
      const got = detectWeakHookRuns({
        summaries: vector.input.summaries,
        minRunLength: vector.input.minRunLength,
      });
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("projects promise timelines per shared vectors", () => {
    for (const vector of vectors.timeline) {
      const got = buildPromiseTimeline(asHooks(vector.input.hooks), vector.input.currentChapter);
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("parses expected payoff chapters and freezes the contract", () => {
    expect(parseExpectedChapter("第15章")).toBe(15);
    expect(parseExpectedChapter("15")).toBe(15);
    expect(parseExpectedChapter("")).toBeUndefined();
    expect(parseExpectedChapter("遥遥无期")).toBeUndefined();
    expect(vectors.contract).toEqual({
      defaults: {
        coreHookStalledThreshold: DEFAULT_CORE_HOOK_STALLED_THRESHOLD,
        weakHookMinRun: DEFAULT_WEAK_HOOK_MIN_RUN,
      },
      strengths: ["strong", "weak", "none"],
      timelineStates: ["open", "advancing", "fulfilled", "overdue"],
    });
  });
});
