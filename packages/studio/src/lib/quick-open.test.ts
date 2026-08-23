import { describe, expect, it } from "vitest";
import {
  QUICK_OPEN_RENDER_LIMIT,
  buildQuickOpenItems,
  cacheChapters,
  filterQuickOpenItems,
  isChapterCacheFresh,
  readCachedChapters,
} from "./quick-open";

const input = {
  books: [
    { id: "b1", title: "山河志" },
    { id: "b2", title: "The Long Night" },
  ],
  films: [{ projectId: "f1", title: "Alpha 测试剧" }],
  sessions: [
    { sessionId: "s1", title: "世界观设定讨论", bookId: "b1" },
    { sessionId: "s2", title: "续写大纲", bookId: null },
  ],
  chaptersByBook: {
    b1: [
      { number: 1, title: "启程" },
      { number: 2 },
      { number: 12, title: "夜袭" },
    ],
  },
};

const items = buildQuickOpenItems(input);

describe("buildQuickOpenItems", () => {
  it("indexes books, chapters, films and sessions with stable ids", () => {
    expect(items.map((item) => item.id)).toEqual([
      "book:b1",
      "book:b2",
      "chapter:b1:1",
      "chapter:b1:2",
      "chapter:b1:12",
      "film:f1",
      "session:s1",
      "session:s2",
    ]);
  });

  it("derives chapter labels and routes, falling back to the chapter number", () => {
    const second = items.find((item) => item.id === "chapter:b1:2")!;
    expect(second.label).toBe("第 2 章");
    expect(second.detail).toBe("山河志");
    expect(second.route).toEqual({ page: "chapter", bookId: "b1", chapterNumber: 2 });
  });

  it("routes book sessions into the book and project sessions into chat", () => {
    expect(items.find((item) => item.id === "session:s1")!.route)
      .toEqual({ page: "book", bookId: "b1" });
    expect(items.find((item) => item.id === "session:s2")!.route)
      .toEqual({ page: "chat" });
  });
});

describe("filterQuickOpenItems", () => {
  it("returns the first page for a blank query", () => {
    const many = Array.from({ length: QUICK_OPEN_RENDER_LIMIT + 40 }, (_, i) => ({
      id: `book:b${i}`,
      kind: "book" as const,
      label: `书 ${i}`,
      route: { page: "book" as const, bookId: String(i) },
    }));
    expect(filterQuickOpenItems(many, "")).toHaveLength(QUICK_OPEN_RENDER_LIMIT);
    expect(filterQuickOpenItems(many, "   ")).toHaveLength(QUICK_OPEN_RENDER_LIMIT);
  });

  it("matches labels case-insensitively across kinds", () => {
    expect(filterQuickOpenItems(items, "山河").map((item) => item.id)).toContain("book:b1");
    expect(filterQuickOpenItems(items, "long night").map((item) => item.id)).toContain("book:b2");
    expect(filterQuickOpenItems(items, "alpha").map((item) => item.id)).toContain("film:f1");
  });

  it("matches chapter numbers so 第 N 章 is reachable by typing the number", () => {
    expect(filterQuickOpenItems(items, "12").map((item) => item.id)).toContain("chapter:b1:12");
  });

  it("applies space-separated terms as AND over label and detail", () => {
    expect(filterQuickOpenItems(items, "夜袭 山河").map((item) => item.id)).toEqual(["chapter:b1:12"]);
    expect(filterQuickOpenItems(items, "夜袭 别的书")).toEqual([]);
  });
});

describe("chapter cache", () => {
  it("stores and expires chapter indexes by book", () => {
    cacheChapters("b1", [{ number: 3, title: "新章" }], 1_000);
    expect(isChapterCacheFresh("b1", 1_000 + 29_999)).toBe(true);
    expect(isChapterCacheFresh("b1", 1_000 + 30_001)).toBe(false);
    expect(readCachedChapters("b1").map((chapter) => chapter.number)).toEqual([3]);
    expect(readCachedChapters("unknown")).toEqual([]);
  });
});
