import { describe, expect, it } from "vitest";
import { parseHash, routeToHash } from "../hooks/use-hash-route";

/**
 * 208 号：chapter 页深链——此前 state-only 路由（HASH_PAGES 不含、无
 * hash 序列化），刷新即回首页、URL 不可分享。
 */
describe("chapter deep link route", () => {
  it("parses #/book/:id/chapter/:num", () => {
    expect(parseHash("#/book/b1/chapter/3")).toEqual({
      page: "chapter",
      bookId: "b1",
      chapterNumber: 3,
    });
  });

  it("percent-encoded bookId decodes", () => {
    expect(parseHash("#/book/%E5%A4%9C%E6%B8%AF/chapter/12")).toEqual({
      page: "chapter",
      bookId: "夜港",
      chapterNumber: 12,
    });
  });

  it("round-trips through routeToHash", () => {
    expect(routeToHash({ page: "chapter", bookId: "b1", chapterNumber: 3 })).toBe(
      "#/book/b1/chapter/3",
    );
    // 非法书名 id 编码后仍可解析回原值
    const hash = routeToHash({ page: "chapter", bookId: "夜港", chapterNumber: 1 });
    expect(parseHash(hash)).toEqual({ page: "chapter", bookId: "夜港", chapterNumber: 1 });
  });

  it("rejects non-numeric chapter segments", () => {
    // 「chapter/abc」不匹配章节路由，也不是单段 book 路由 → 回 dashboard
    expect(parseHash("#/book/b1/chapter/abc").page).toBe("dashboard");
  });
});
