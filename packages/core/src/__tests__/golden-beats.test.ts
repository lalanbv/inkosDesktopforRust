import { describe, expect, it } from "vitest";
import { readFile } from "node:fs/promises";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { mergeChapterBeats, parseBeatsJson, type PlotlineBeat } from "../pipeline/timeline-settle.js";
import type { Timeline } from "../models/timeline.js";

/**
 * 212 号：节拍沉淀纯函数域共享 golden 差分——engine-rs
 * tests/golden_beats_diff.rs 读同一 beats-vectors.json 逐例断言（双端
 * 「逐字对齐」由差分守门；向量即契约，任一侧改动漂移即红）。
 */
const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  await readFile(join(here, "golden", "beats-vectors.json"), "utf-8"),
) as {
  readonly parse: ReadonlyArray<{
    readonly name: string;
    readonly text: string;
    readonly validIds: ReadonlyArray<string>;
    readonly errorContains?: string;
    readonly expected?: ReadonlyArray<PlotlineBeat> | null;
  }>;
  readonly merge: ReadonlyArray<{
    readonly name: string;
    readonly timeline: Timeline;
    readonly chapter: number;
    readonly beats: ReadonlyArray<PlotlineBeat>;
    readonly expectedApplied: number;
    readonly expectedPlotlines: unknown;
    readonly expectedUpdatedAt?: string;
  }>;
};

describe("beats golden vectors (shared with engine-rs)", () => {
  for (const tc of vectors.parse) {
    it(`parse: ${tc.name}`, () => {
      if (tc.errorContains) {
        expect(() => parseBeatsJson(tc.text, new Set(tc.validIds))).toThrow(tc.errorContains);
        return;
      }
      const got = parseBeatsJson(tc.text, new Set(tc.validIds));
      // undefined 字段归一为缺席键再比较。
      const normalized = got.map((b) => JSON.parse(JSON.stringify(b)));
      expect(normalized).toEqual(tc.expected);
    });
  }

  for (const tc of vectors.merge) {
    it(`merge: ${tc.name}`, () => {
      const timeline = structuredClone(tc.timeline);
      const applied = mergeChapterBeats(timeline, tc.chapter, tc.beats);
      expect(applied).toBe(tc.expectedApplied);
      // updatedAt 非确定：断言刷新语义（applied>0 → 变化；=0 → 保持）。
      if (tc.expectedUpdatedAt !== undefined) {
        expect(timeline.updatedAt).toBe(tc.expectedUpdatedAt);
      } else if (tc.expectedApplied > 0) {
        expect(timeline.updatedAt).not.toBe(tc.timeline.updatedAt);
      }
      expect(JSON.parse(JSON.stringify(timeline.plotlines))).toEqual(tc.expectedPlotlines);
    });
  }
});
