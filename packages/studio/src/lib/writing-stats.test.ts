//! R13/382 号：写作数据聚合单测（窗口/节奏桶/通过率/token）。
import { describe, expect, it } from "vitest";
import { aggregateWritingStats, type ChapterStatRow } from "./writing-stats";

const NOW = "2026-09-13T12:00:00.000Z";

function row(daysAgo: number, wordCount: number, status: string, totalTokens = 0): ChapterStatRow {
  return {
    updatedAt: new Date(new Date(`${NOW.slice(0, 10)}T00:00:00Z`).getTime() - daysAgo * 86_400_000).toISOString(),
    wordCount,
    status,
    totalTokens,
  };
}

describe("writing stats (R13)", () => {
  it("aggregates daily rhythm within window", () => {
    const stats = aggregateWritingStats(
      [row(0, 1000, "ready-for-review"), row(0, 500, "audit-passed"), row(40, 900, "published")],
      NOW,
    );
    // 40 天前在 30 天窗外。
    expect(stats.chapters30d).toBe(2);
    expect(stats.words30d).toBe(1500);
    expect(stats.daily).toHaveLength(1);
    expect(stats.daily[0]).toEqual({ date: NOW.slice(0, 10), words: 1500, chapters: 2 });
  });

  it("computes pass rate over all chapters", () => {
    const stats = aggregateWritingStats(
      [
        row(1, 100, "audit-passed"),
        row(2, 100, "approved"),
        row(3, 100, "audit-failed"),
        row(4, 100, "drafting"),
      ],
      NOW,
    );
    // 通过 = audit-passed/approved/published/ready-for-review；drafting 不算。
    expect(stats.passRate).toBe(50);
    expect(stats.totalChapters).toBe(4);
    expect(stats.totalWords).toBe(400);
  });

  it("sums tokens and handles empty input", () => {
    const empty = aggregateWritingStats([], NOW);
    expect(empty.totalChapters).toBe(0);
    expect(empty.passRate).toBeUndefined();
    expect(empty.daily).toHaveLength(0);

    const stats = aggregateWritingStats(
      [{ updatedAt: "2026-09-13T00:00:00.000Z", wordCount: 10, status: "audit-passed", totalTokens: 123 }],
      NOW,
    );
    expect(stats.totalTokens).toBe(123);
  });
});
