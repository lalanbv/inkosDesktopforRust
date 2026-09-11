import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Scheduler, type SchedulerConfig } from "../pipeline/scheduler.js";
import type { BookConfig } from "../models/book.js";

function createConfig(): SchedulerConfig {
  return {
    client: {
      provider: "openai",
      apiFormat: "chat",
      stream: false,
      defaults: {
        temperature: 0.7,
        maxTokens: 1024,
        thinkingBudget: 0,
      },
    } as SchedulerConfig["client"],
    model: "test-model",
    projectRoot: process.cwd(),
    radarCron: "*/1 * * * *",
    writeCron: "*/1 * * * *",
    maxConcurrentBooks: 1,
    chaptersPerCycle: 1,
    retryDelayMs: 0,
    cooldownAfterChapterMs: 0,
    maxChaptersPerDay: 10,
  };
}

describe("Scheduler", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("does not start a second write cycle while one is still running", async () => {
    const scheduler = new Scheduler(createConfig());
    let releaseCycle: (() => void) | undefined;
    const blockedCycle = new Promise<void>((resolve) => {
      releaseCycle = resolve;
    });

    const runWriteCycle = vi
      .spyOn(scheduler as unknown as { runWriteCycle: () => Promise<void> }, "runWriteCycle")
      .mockImplementation(async () => {
        if (runWriteCycle.mock.calls.length === 1) {
          return;
        }
        await blockedCycle;
      });
    vi.spyOn(scheduler as unknown as { runRadarScan: () => Promise<void> }, "runRadarScan")
      .mockResolvedValue(undefined);

    await scheduler.start();

    await vi.advanceTimersByTimeAsync(60_000);
    expect(runWriteCycle).toHaveBeenCalledTimes(2);

    await vi.advanceTimersByTimeAsync(60_000);
    expect(runWriteCycle).toHaveBeenCalledTimes(2);

    releaseCycle?.();
    await blockedCycle;
    scheduler.stop();
  });

  it("treats state-degraded chapter results as handled failures", async () => {
    const onChapterComplete = vi.fn();
    const scheduler = new Scheduler({
      ...createConfig(),
      onChapterComplete,
    });
    const bookConfig: BookConfig = {
      id: "book-1",
      title: "Book 1",
      platform: "other",
      genre: "other",
      status: "active",
      targetChapters: 10,
      chapterWordCount: 2200,
      createdAt: "2026-04-01T00:00:00.000Z",
      updatedAt: "2026-04-01T00:00:00.000Z",
    };

    vi.spyOn(
      (scheduler as unknown as { pipeline: { writeNextChapter: (bookId: string, words?: number, temp?: number) => Promise<unknown> } }).pipeline,
      "writeNextChapter",
    ).mockResolvedValue({
        chapterNumber: 3,
        title: "Broken State",
        wordCount: 2100,
        revised: false,
        status: "state-degraded",
        auditResult: {
          passed: true,
          issues: [{
            severity: "warning",
            category: "state-validation",
            description: "state validation still failed after retry",
            suggestion: "repair state before continuing",
          }],
          summary: "clean",
        },
    });
    const handleAuditFailure = vi.spyOn(
      scheduler as unknown as { handleAuditFailure: (bookId: string, chapterNumber: number, issueCategories?: string[], bookConfig?: BookConfig) => Promise<boolean> },
      "handleAuditFailure",
    ).mockResolvedValue(false);

    const success = await (
      scheduler as unknown as {
        writeOneChapter: (bookId: string, bookConfig: BookConfig) => Promise<boolean>;
      }
    ).writeOneChapter("book-1", bookConfig);

    expect(success).toBe(false);
    expect(handleAuditFailure).toHaveBeenCalledWith("book-1", 3, ["state-validation"], bookConfig);
    expect(onChapterComplete).toHaveBeenCalledWith("book-1", 3, "state-degraded");
  });

  it("defers-and-continues when consecutive debts stay below the book threshold (G3/337)", async () => {
    const bookConfig: BookConfig = {
      id: "book-1",
      title: "Book 1",
      platform: "other",
      genre: "other",
      status: "active",
      targetChapters: 10,
      chapterWordCount: 2200,
      createdAt: "2026-04-01T00:00:00.000Z",
      updatedAt: "2026-04-01T00:00:00.000Z",
      // 完成优先 + 上限 5：失败 3/4 次 → defer-and-continue（记债继续）。
      governance: { policy: "completion-first", maxConsecutiveDebts: 5 },
    };
    const scheduler = new Scheduler({ ...createConfig() });
    vi.spyOn(
      (scheduler as unknown as { pipeline: { writeNextChapter: (bookId: string) => Promise<unknown> } }).pipeline,
      "writeNextChapter",
    ).mockResolvedValue({
      chapterNumber: 3,
      title: "Broken State",
      wordCount: 2100,
      revised: false,
      status: "state-degraded",
      auditResult: { passed: false, issues: [], summary: "fail" },
    });

    const recorded: Array<{ bookId: string; status: string }> = [];
    const handleAuditFailure = vi.spyOn(
      scheduler as unknown as {
        handleAuditFailure: (
          bookId: string,
          chapterNumber: number,
          issueCategories?: string[],
          bookConfig?: BookConfig,
        ) => Promise<boolean>;
      },
      "handleAuditFailure",
    );

    // 第 3、4 次失败：completion-first + 上限 5 → defer-and-continue（继续）。
    handleAuditFailure.mockRestore();
    const memoryModule = (await import("../state/memory-db.js")) as unknown as {
      MemoryDB: new (dir: string) => { recordDebt: (debt: { bookId: string; status: string }) => void };
    };
    const memorySpy = vi
      .spyOn(memoryModule, "MemoryDB")
      .mockImplementation(() => ({
        recordDebt: (debt: { bookId: string; status: string }) => {
          recorded.push({ bookId: debt.bookId, status: debt.status });
        },
      }) as never);

    const writeOne = () =>
      (
        scheduler as unknown as {
          writeOneChapter: (bookId: string, bookConfig: BookConfig) => Promise<boolean>;
        }
      ).writeOneChapter("book-1", bookConfig);
    // 制造连续失败计数到 3（failures<=maxAuditRetries=2 走重试分支返回 false）。
    const failOnce = async () => {
      await writeOne();
    };
    const inner = scheduler as unknown as {
      handleAuditFailure: (
        bookId: string,
        chapterNumber: number,
        issueCategories?: string[],
        bookConfig?: BookConfig,
      ) => Promise<boolean>;
      consecutiveFailures: Map<string, number>;
    };
    inner.consecutiveFailures.set("book-1", 2);
    const third = await writeOne();
    expect(third).toBe(true);
    expect(recorded).toHaveLength(1);
    expect(recorded[0]).toEqual({ bookId: "book-1", status: "deferred" });
    // defer 后失败计数清零。
    expect(inner.consecutiveFailures.has("book-1")).toBe(false);
    memorySpy.mockRestore();
    void failOnce;
  });
});
