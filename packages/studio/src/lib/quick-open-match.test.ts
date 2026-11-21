//! R47/1640 号：⌘P 正文命中档条目投影纯函数（search 命中 → QuickOpenItem）。
import { describe, expect, it } from "vitest";
import { buildMatchQuickOpenItems } from "./quick-open";

describe("buildMatchQuickOpenItems（R47/1640 号）", () => {
  it("命中投影为 match 档条目：route 直达章、detail 承载片段", () => {
    const items = buildMatchQuickOpenItems("b1", [
      { chapterNumber: 2, title: "灵脉", snippet: "…灵气紊乱…" },
      { chapterNumber: 12, title: "风起", snippet: "…" },
    ]);
    expect(items.map((item) => item.id)).toEqual(["match:b1:2", "match:b1:12"]);
    expect(items[0]).toMatchObject({
      kind: "match",
      label: "灵脉",
      detail: "…灵气紊乱…",
      searchable: "2",
      route: { page: "chapter", bookId: "b1", chapterNumber: 2 },
    });
  });

  it("空命中 → 空档（面板回落既有索引行为）", () => {
    expect(buildMatchQuickOpenItems("b1", [])).toEqual([]);
  });
});
