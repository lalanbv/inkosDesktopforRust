import { describe, expect, it } from "vitest";
import { renderForecastComparisonMarkdown } from "../forecast/render.js";
import { NarrativeForecastSchema } from "../forecast/schema.js";

// 818 号：forecast comparison 表格单元格转义锁定（与 Rust forecast/render.rs 测试镜像——
// comparison.md 为只写交付物无回读解析，此测试锁渲染形态防两侧漂移）：
// 竖线 `\|`、换行→空格，行保持单行。
const base = {
  version: 1 as const,
  forecastId: "fc-test-1",
  bookId: "b1",
  createdAt: "2026-08-18T00:00:00.000Z",
  language: "zh" as const,
  divergence: "合作还是对抗",
  horizon: 5,
  baseChapter: 3,
  contextFingerprint: "abc",
  status: "active" as const,
};

describe("forecast comparison table escaping (818)", () => {
  it("escapes pipes and flattens newlines in the branch title cell, keeping one row", () => {
    const forecast = NarrativeForecastSchema.parse({
      ...base,
      branches: [{
        branchId: "branch-1",
        title: "合作|对抗\n背水一战",
        premise: "p",
        beats: [{ chapter: 4, summary: "s" }],
        characterDecisions: [],
        projectedChanges: { characters: [], relationships: [], world: [], hooks: [] },
        risks: [],
        uncertainties: [],
        intentRationale: "r",
      }],
    });
    const md = renderForecastComparisonMarkdown(forecast);
    const rows = md.split("\n").filter((line) => line.startsWith("| branch-1 |"));
    expect(rows).toEqual(["| branch-1 | 合作\\|对抗 背水一战 | 0 | p |"]);
    expect(rows).toHaveLength(1); // 表格行不因换行截断
  });
});
