import { describe, expect, it } from "vitest";
import { buildBookTree, deriveExpandedBookIds, filterTree } from "./tree-filter";

const tree = [
  {
    id: "b1",
    title: "山河志",
    children: [
      { id: "s1", title: "世界观设定" },
      { id: "s2", title: "夜袭章讨论" },
    ],
  },
  {
    id: "b2",
    title: "The Long Night",
    children: [{ id: "s3", title: "Outline" }],
  },
  { id: "b3", title: "番外集", children: [] },
];

describe("filterTree", () => {
  it("returns the tree as-is for a blank query", () => {
    expect(filterTree(tree, "")).toEqual(tree);
    expect(filterTree(tree, "   ")).toEqual(tree);
  });

  it("keeps the whole subtree when the parent title hits", () => {
    const result = filterTree(tree, "山河");
    expect(result.map((node) => node.id)).toEqual(["b1"]);
    expect(result[0].children?.map((child) => child.id)).toEqual(["s1", "s2"]);
  });

  it("keeps the ancestor chain when only a child hits, pruning siblings", () => {
    const result = filterTree(tree, "夜袭");
    expect(result.map((node) => node.id)).toEqual(["b1"]);
    expect(result[0].children?.map((child) => child.id)).toEqual(["s2"]);
  });

  it("matches case-insensitively across books and sessions", () => {
    expect(filterTree(tree, "outline").map((node) => node.id)).toEqual(["b2"]);
    expect(filterTree(tree, "LONG").map((node) => node.id)).toEqual(["b2"]);
  });

  it("drops books with no hit in title or children", () => {
    expect(filterTree(tree, "不存在的词")).toEqual([]);
  });
});

describe("deriveExpandedBookIds", () => {
  it("expands books whose title or child hit during filtering", () => {
    expect(deriveExpandedBookIds(tree, "夜袭")).toEqual(new Set(["b1"]));
    expect(deriveExpandedBookIds(tree, "long")).toEqual(new Set(["b2"]));
  });

  it("returns an empty set when not filtering", () => {
    expect(deriveExpandedBookIds(tree, "")).toEqual(new Set());
  });
});

describe("buildBookTree", () => {
  it("assembles the book→session view tree", () => {
    const built = buildBookTree(
      [{ id: "b1", title: "山河志" }],
      { b1: [{ id: "s1", title: "会话一" }] },
    );
    expect(built[0].children?.map((child) => child.title)).toEqual(["会话一"]);
  });
});

describe("50 books × 10 sessions fuzz", () => {
  it("filters a large tree correctly and fast enough for one frame", () => {
    const big = Array.from({ length: 50 }, (_, b) => ({
      id: `b${b}`,
      title: `书 ${b} 号`,
      children: Array.from({ length: 10 }, (_, s) => ({
        id: `s${b}-${s}`,
        title: `会话 ${b}-${s}`,
      })),
    }));
    const start = performance.now();
    const result = filterTree(big, "会话 49-");
    const elapsed = performance.now() - start;
    expect(result.map((node) => node.id)).toEqual(["b49"]);
    expect(result[0].children).toHaveLength(10);
    // 16ms 一帧预算;CI 抖动留裕量到 50ms(超此值视为实现退化)
    expect(elapsed).toBeLessThan(50);
  });
});
