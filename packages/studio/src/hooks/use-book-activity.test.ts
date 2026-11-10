import { describe, expect, it } from "vitest";
import type { SSEMessage } from "./use-sse";
import {
  writeTaskSessionId,
  applyBookCollectionEvent,
  deriveActiveBookIds,
  deriveBookActivity,
  shouldRefetchBookCollections,
  shouldRefetchBookView,
  shouldRefetchDaemonStatus,
} from "./use-book-activity";

function msg(event: string, data: unknown, timestamp: number): SSEMessage {
  return { event, data, timestamp, seq: timestamp };
}

describe("deriveBookActivity", () => {
  it("keeps a book in writing state after write:start until completion", () => {
    const messages: ReadonlyArray<SSEMessage> = [
      msg("write:start", { bookId: "alpha" }, 1),
      msg("log", { message: "Phase 1" }, 2),
      msg("llm:progress", { totalChars: 1200 }, 3),
    ];

    expect(deriveBookActivity(messages, "alpha")).toMatchObject({
      writing: true,
      drafting: false,
      lastError: null,
    });
  });

  it("clears writing state after completion or error", () => {
    const completed: ReadonlyArray<SSEMessage> = [
      msg("write:start", { bookId: "alpha" }, 1),
      msg("write:complete", { bookId: "alpha", chapterNumber: 2 }, 2),
    ];
    const errored: ReadonlyArray<SSEMessage> = [
      msg("write:start", { bookId: "alpha" }, 1),
      msg("write:error", { bookId: "alpha", error: "locked" }, 2),
    ];

    expect(deriveBookActivity(completed, "alpha")).toMatchObject({
      writing: false,
      lastError: null,
    });
    expect(deriveBookActivity(errored, "alpha")).toMatchObject({
      writing: false,
      lastError: "locked",
    });
  });

  it("tracks drafting independently from writing", () => {
    const messages: ReadonlyArray<SSEMessage> = [
      msg("draft:start", { bookId: "alpha" }, 1),
      msg("write:start", { bookId: "beta" }, 2),
    ];

    expect(deriveBookActivity(messages, "alpha")).toMatchObject({
      writing: false,
      drafting: true,
    });
  });
});

describe("deriveActiveBookIds", () => {
  it("returns only books with in-flight background work", () => {
    const messages: ReadonlyArray<SSEMessage> = [
      msg("write:start", { bookId: "alpha" }, 1),
      msg("draft:start", { bookId: "beta" }, 2),
      msg("write:complete", { bookId: "alpha", chapterNumber: 2 }, 3),
      msg("write:start", { bookId: "gamma" }, 4),
      msg("draft:error", { bookId: "beta", error: "quota" }, 5),
    ];

    expect([...deriveActiveBookIds(messages)].sort()).toEqual(["gamma"]);
  });
});

describe("shouldRefetchBookView", () => {
  it("refreshes the book detail view after terminal background jobs for that book", () => {
    expect(shouldRefetchBookView(msg("write:complete", { bookId: "alpha" }, 1), "alpha")).toBe(true);
    expect(shouldRefetchBookView(msg("draft:error", { bookId: "alpha", error: "quota" }, 1), "alpha")).toBe(true);
    expect(shouldRefetchBookView(msg("rewrite:complete", { bookId: "alpha", chapterNumber: 3 }, 1), "alpha")).toBe(true);
    expect(shouldRefetchBookView(msg("revise:error", { bookId: "alpha", error: "bad" }, 1), "alpha")).toBe(true);
    expect(shouldRefetchBookView(msg("audit:complete", { bookId: "alpha", chapter: 3, passed: true }, 1), "alpha")).toBe(true);
    // 961 号：TS 2.0 回退端审阅完成改名 review:complete（Rust 仍 audit:complete）。
    expect(shouldRefetchBookView(msg("review:complete", { bookId: "alpha", chapter: 3, observationCount: 2 }, 1), "alpha")).toBe(true);
    expect(shouldRefetchBookCollections(msg("review:complete", { bookId: "alpha", chapter: 3 }, 1))).toBe(true);
    expect(shouldRefetchBookView(msg("audit:start", { bookId: "alpha", chapter: 3 }, 1), "alpha")).toBe(false);
    expect(shouldRefetchBookView(msg("rewrite:complete", { bookId: "beta" }, 1), "alpha")).toBe(false);
  });

  it("965 号：书籍状态变更类（双端同名同 bookId 负载）刷新本书视图，他书不刷", () => {
    const changed: Array<[string, Record<string, unknown>]> = [
      ["chapter:deleted", { bookId: "alpha", chapterNumber: 5 }],
      ["chapter:restored", { bookId: "alpha", chapterNumber: 4 }],
      ["consolidate:complete", { bookId: "alpha", archivedVolumes: 1 }],
      ["consolidate:error", { bookId: "alpha", error: "x" }],
      ["foundation:revised", { bookId: "alpha" }],
      ["foundation:error", { bookId: "alpha", error: "x" }],
      ["repair-state:complete", { bookId: "alpha", chapter: 3 }],
      ["repair-state:error", { bookId: "alpha", chapter: 3, error: "x" }],
    ];
    for (const [event, data] of changed) {
      expect(shouldRefetchBookView(msg(event, data, 1), "alpha"), event).toBe(true);
      expect(shouldRefetchBookView(msg(event, { ...data, bookId: "beta" }, 1), "alpha"), event).toBe(false);
    }
    // director:* 无 bookId 且结果走 HTTP 响应——不入刷新集。
    expect(shouldRefetchBookView(msg("director:complete", { count: 3 }, 1), "alpha")).toBe(false);
  });
});

describe("shouldRefetchBookCollections", () => {
  it("refreshes book lists for create/delete and chapter-changing terminal events", () => {
    expect(shouldRefetchBookCollections(msg("book:created", { bookId: "alpha" }, 1))).toBe(true);
    expect(shouldRefetchBookCollections(msg("book:deleted", { bookId: "alpha" }, 1))).toBe(true);
    expect(shouldRefetchBookCollections(msg("write:complete", { bookId: "alpha" }, 1))).toBe(true);
    expect(shouldRefetchBookCollections(msg("draft:error", { bookId: "alpha" }, 1))).toBe(true);
    expect(shouldRefetchBookCollections(msg("rewrite:complete", { bookId: "alpha" }, 1))).toBe(true);
    // 965 号：章节数变化类同步书目「N 章」；归并/地基/修复不改章数不刷书目。
    expect(shouldRefetchBookCollections(msg("chapter:deleted", { bookId: "alpha", chapterNumber: 5 }, 1))).toBe(true);
    expect(shouldRefetchBookCollections(msg("chapter:restored", { bookId: "alpha", chapterNumber: 4 }, 1))).toBe(true);
    expect(shouldRefetchBookCollections(msg("consolidate:complete", { bookId: "alpha" }, 1))).toBe(false);
    expect(shouldRefetchBookCollections(msg("foundation:revised", { bookId: "alpha" }, 1))).toBe(false);
    expect(shouldRefetchBookCollections(msg("audit:start", { bookId: "alpha" }, 1))).toBe(false);
    expect(shouldRefetchBookCollections(undefined)).toBe(false);
  });
});

describe("shouldRefetchDaemonStatus", () => {
  it("refreshes daemon status for daemon terminal events", () => {
    expect(shouldRefetchDaemonStatus(msg("daemon:started", {}, 1))).toBe(true);
    expect(shouldRefetchDaemonStatus(msg("daemon:stopped", {}, 1))).toBe(true);
    expect(shouldRefetchDaemonStatus(msg("daemon:error", {}, 1))).toBe(true);
    expect(shouldRefetchDaemonStatus(msg("daemon:chapter", {}, 1))).toBe(false);
  });
});

describe("applyBookCollectionEvent", () => {
  it("upserts a created book from the event payload without requiring a refetch", () => {
    const books = [
      { id: "alpha", title: "Alpha", genre: "urban", status: "active", chaptersWritten: 3 },
    ];

    expect(applyBookCollectionEvent(books, msg("book:created", {
      bookId: "beta",
      book: { id: "beta", title: "Beta", genre: "xuanhuan", status: "outlining", chaptersWritten: 0 },
    }, 1))).toEqual([
      { id: "alpha", title: "Alpha", genre: "urban", status: "active", chaptersWritten: 3 },
      { id: "beta", title: "Beta", genre: "xuanhuan", status: "outlining", chaptersWritten: 0 },
    ]);
  });

  it("returns null when a collection event lacks enough data for incremental update", () => {
    expect(applyBookCollectionEvent([], msg("book:created", { bookId: "beta" }, 1))).toBeNull();
  });
});

describe("writeTaskSessionId（176 号书籍页伪会话锚点）", () => {
  it("为每本书生成稳定且互异的伪会话 id", () => {
    expect(writeTaskSessionId("demo-book")).toBe("book:demo-book:write");
    expect(writeTaskSessionId("demo-book")).toBe(writeTaskSessionId("demo-book"));
    expect(writeTaskSessionId("a")).not.toBe(writeTaskSessionId("b"));
  });

  it("不含聊天会话目录形态——task-store 文件名键与 abort 端点对它透明", () => {
    const id = writeTaskSessionId("xianxia-01");
    // 形如 book:{id}:write；URL 使用处自行 encodeURIComponent（EventSource/POST 路径）。
    expect(id.startsWith("book:")).toBe(true);
    expect(id.endsWith(":write")).toBe(true);
  });
});
