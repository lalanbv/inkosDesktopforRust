import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { docxEscapeXml, mdZipEntryPath } from "../interaction/export-artifact.js";
import { dailyWordDelta, expectedFinish, goalStreak, shiftDayKey } from "../utils/writing-goal.js";
import { usageProjection } from "../utils/usage-budget.js";
import { structureInvariant, type ChapterFingerprint, type StructureOp } from "../state/chapter-structure.js";

/** 语料样本形态（structure-invariant 组；与生成器 scripts/generate-property-corpus.mjs 同形）。 */
export interface StructureInvariantVector {
  op: StructureOp;
  before: ChapterFingerprint[];
  after: ChapterFingerprint[];
  ok: boolean;
  violations: string[];
}

// R62/1690 号：落盘语料双端回放（TS 侧）。语料由 scripts/generate-property-corpus.mjs
// 以固定 seed 生成（期望值=生成时的实现现算）；本测逐样本回放当前实现——实现
// 变更即红（有意变更须重跑生成器换基线，diff 是行为变更的审查面）。
// Rust 侧同语料回放：engine-rs/tests/property_corpus_replay.rs。
// 必红自检已验证：临时破坏一个镜像（如 docxEscapeXml 去 `>` 转义）→ 本测红。

const corpusDir = new URL("../../../studio/src/__tests__/golden/property-corpus/", import.meta.url);
const load = <T,>(name: string): { seed: number; generatedAt: string; sampleCount: number; samples: T[] } =>
  JSON.parse(readFileSync(new URL(`${name}.json`, corpusDir), "utf-8"));

const docxVectors = load<{ input: string; output: string }>("docx-escape-xml");
const zipVectors = load<{
  input: { volumeId?: string; volumes: { id: string; title: string; order: number }[]; chapterNumber: number; chapterTitle: string };
  output: string;
}>("md-zip-entry-path");
const deltaVectors = load<{
  rows: { ts: string; chapterNum: number; before: number; after: number }[];
  output: { dayKey: string; delta: number }[];
}>("daily-word-delta");
const streakVectors = load<{
  input: { deltas: { dayKey: string; delta: number }[]; dailyGoalWords: number; todayKey: string };
  output: number;
}>("goal-streak");
const usageVectors = load<{
  rows: { serviceKey: string; model: string; promptTokens: number; completionTokens: number; totalTokens: number; ts: string }[];
  todayKey: string;
  output: { serviceKey: string; daily: number; monthly: number; total: number }[];
}>("usage-projection");
const finishVectors = load<{
  input: { currentWords: number; targetWords: number; dailyPaceWords: number; todayKey: string };
  output: string | null;
}>("expected-finish");
const shiftVectors = load<{ input: { dayKey: string; days: number }; output: string }>("shift-day-key");
const invariantVectors = load<StructureInvariantVector>("structure-invariant");

describe("R62 语料回放（TS 侧）", () => {
  it("docxEscapeXml corpus", () => {
    for (const vector of docxVectors.samples) {
      expect(docxEscapeXml(vector.input)).toBe(vector.output);
    }
  });

  it("mdZipEntryPath corpus", () => {
    for (const vector of zipVectors.samples) {
      expect(mdZipEntryPath(vector.input.volumeId, vector.input.volumes, vector.input.chapterNumber, vector.input.chapterTitle)).toBe(vector.output);
    }
  });

  it("dailyWordDelta corpus", () => {
    for (const vector of deltaVectors.samples) {
      expect(dailyWordDelta(vector.rows, (row) => row.ts.slice(0, 10))).toEqual(vector.output);
    }
  });

  it("goalStreak corpus", () => {
    for (const vector of streakVectors.samples) {
      expect(goalStreak(vector.input.deltas, vector.input.dailyGoalWords, vector.input.todayKey)).toBe(vector.output);
    }
  });

  it("usageProjection corpus", () => {
    for (const vector of usageVectors.samples) {
      const projection = usageProjection(vector.rows, (row) => row.ts.slice(0, 10), vector.todayKey);
      const sorted = [...projection.entries()]
        .map(([serviceKey, usage]) => ({ serviceKey, ...usage }))
        .sort((a, b) => (a.serviceKey < b.serviceKey ? -1 : 1));
      expect(sorted).toEqual(vector.output);
    }
  });

  it("expectedFinish corpus", () => {
    for (const vector of finishVectors.samples) {
      expect(expectedFinish(vector.input.currentWords, vector.input.targetWords, vector.input.dailyPaceWords, vector.input.todayKey) ?? null)
        .toBe(vector.output);
    }
  });

  it("shiftDayKey corpus", () => {
    for (const vector of shiftVectors.samples) {
      expect(shiftDayKey(vector.input.dayKey, vector.input.days)).toBe(vector.output);
    }
  });

  it("structureInvariant corpus（含 contentFingerprint 段共用文件的样本量守卫）", () => {
    // 语料样本数守卫：每组恰 200（施工图下限），防生成器回归静默缩量。
    for (const [name, vectors] of [
      ["docx-escape-xml", docxVectors],
      ["md-zip-entry-path", zipVectors],
      ["daily-word-delta", deltaVectors],
      ["goal-streak", streakVectors],
      ["usage-projection", usageVectors],
      ["expected-finish", finishVectors],
      ["shift-day-key", shiftVectors],
      ["structure-invariant", invariantVectors],
    ] as const) {
      expect(vectors.sampleCount, name).toBe(200);
      expect(vectors.seed, name).toBe(20261126);
    }
    for (const vector of invariantVectors.samples) {
      const { violations } = structureInvariant(vector.op as StructureOp, vector.before as ChapterFingerprint[], vector.after as ChapterFingerprint[]);
      expect(violations).toEqual(vector.violations);
      expect(violations.length === 0).toBe(vector.ok);
    }
  });
});
