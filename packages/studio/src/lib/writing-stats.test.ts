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

describe("pass rate by book (R16 前置)", () => {
  it("computes per-book pass rates within window", async () => {
    const { passRateByBook } = await import("./writing-stats");
    const NOW2 = "2026-09-13T12:00:00.000Z";
    const mk = (bookId: string, daysAgo: number, status: string) => ({
      bookId,
      updatedAt: new Date(new Date(`${NOW2.slice(0, 10)}T00:00:00Z`).getTime() - daysAgo * 86_400_000).toISOString(),
      wordCount: 100,
      status,
    });
    const rows = [
      mk("b1", 1, "audit-passed"),
      mk("b1", 2, "audit-failed"),
      mk("b2", 3, "ready-for-review"),
      mk("b2", 45, "audit-passed"), // 窗外
    ];
    const got = passRateByBook(rows, 30, NOW2);
    expect(got.map((entry) => entry.bookId)).toEqual(["b1", "b2"]);
    expect(got[0]!.passRate).toBe(50);
    expect(got[1]!.total).toBe(1);
  });
});
