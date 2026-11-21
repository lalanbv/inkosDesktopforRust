//! R51/1653 号：写作目标纯函数 golden 断言（双端一致性的 TS 锚面）。
//! shiftDayKey（跨月/跨年/闰年/回退）、dailyWordDelta（跨日/负增量/同章
//! 多次/空流）、goalStreak（今日未达跳过/今日计入/断链重置/首日/端点早于
//! 今日）、expectedFinish（NaNo 节奏/已达成/零节奏/ceil/跨月）。Rust 侧 =
//! engine-rs/tests/golden_writing_goal_diff.rs 读同一向量文件差分。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  dailyWordDelta,
  expectedFinish,
  goalStreak,
  shiftDayKey,
  type DayDelta,
  type WritingLogRow,
} from "../utils/writing-goal.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/writing-goal-vectors.json"), "utf-8"),
) as {
  shiftDayKey: Array<{ name: string; dayKey: string; days: number; expected: string }>;
  dailyWordDelta: Array<{
    name: string;
    rows: WritingLogRow[];
    dayKeys: string[];
    expected: DayDelta[];
  }>;
  goalStreak: Array<{ name: string; deltas: DayDelta[]; goal: number; todayKey: string; expected: number }>;
  expectedFinish: Array<{
    name: string; current: number; target: number; pace: number; todayKey: string; expected: string;
  }>;
};

describe("writing-goal golden（R51/1653 号）", () => {
  it.each(vectors.shiftDayKey.map((v) => [v.name, v] as const))("shiftDayKey: %s", (_name, vector) => {
    expect(shiftDayKey(vector.dayKey, vector.days)).toBe(vector.expected);
  });

  it.each(vectors.dailyWordDelta.map((v) => [v.name, v] as const))("dailyWordDelta: %s", (_name, vector) => {
    const keyByTs = new Map(vector.rows.map((row, index) => [row.ts, vector.dayKeys[index]!]));
    expect([...dailyWordDelta(vector.rows, (row) => keyByTs.get(row.ts)!)])
      .toEqual(vector.expected.map((entry) => ({dayKey: entry.dayKey, delta: entry.delta})));
  });

  it.each(vectors.goalStreak.map((v) => [v.name, v] as const))("goalStreak: %s", (_name, vector) => {
    expect(goalStreak(vector.deltas, vector.goal, vector.todayKey)).toBe(vector.expected);
  });

  it.each(vectors.expectedFinish.map((v) => [v.name, v] as const))("expectedFinish: %s", (_name, vector) => {
    // 向量空串 = undefined 语义（已达成/零节奏不可预测）。
    expect(expectedFinish(vector.current, vector.target, vector.pace, vector.todayKey) ?? "")
      .toBe(vector.expected);
  });

  it("golden 非空防空转", () => {
    for (const group of ["shiftDayKey", "dailyWordDelta", "goalStreak", "expectedFinish"] as const) {
      expect(vectors[group].length).toBeGreaterThan(0);
    }
  });
});
