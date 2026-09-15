// @vitest-environment jsdom
//! 476 号：ContextLensPanel 交互测试——默认选中末章并渲染装配条目（层级/保护/
//! 编译徽标/压缩留痕）、章切换重拉（预算内未压缩分支）、章数据失败错误透出、
//! 无留痕空态。440 号已真机走查零漂移，此处锁组件四态接线。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ContextLensPanel } from "./ContextLensPanel";

const ch3Lens = {
  version: 1,
  chapter: 3,
  entries: [
    { order: 1, source: "story_bible#世界观", tier: "book-fact", tierPrecedence: 100, protected: true, tokens: 1200, compiled: false, rank: null },
    { order: 2, source: "chapter_index", tier: "book-planning", tierPrecedence: 80, protected: false, tokens: 800, compiled: false, rank: { recency: 1 } },
    { order: 3, source: "compiled:memory", tier: "ephemeral", tierPrecedence: 10, protected: false, tokens: 2200, compiled: true, rank: null },
  ],
  compression: {
    compiledSource: "compiled:memory",
    budgetTokens: 8192,
    protectedTokens: 1200,
    compressibleTokens: 3000,
    preCompressionSources: [
      { source: "memory_0002", tokens: 1500 },
      { source: "memory_0001", tokens: 1500 },
    ],
  },
  notes: ["受保护来源跳过压缩"],
  totals: { entries: 3, protectedEntries: 1, compiledEntries: 1, tokens: 4200 },
};

const ch1Lens = {
  version: 1,
  chapter: 1,
  entries: [
    { order: 1, source: "story_bible#世界观", tier: "book-fact", tierPrecedence: 100, protected: true, tokens: 600, compiled: false, rank: null },
  ],
  compression: null,
  notes: [],
  totals: { entries: 1, protectedEntries: 1, compiledEntries: 0, tokens: 600 },
};

const fetchJsonMock = vi.fn(async (path: string) => {
  if (path === "/books/b1/context-lens") return { chapters: [1, 2, 3] };
  if (path === "/books/b1/context-lens/3") return ch3Lens;
  if (path === "/books/b1/context-lens/1") return ch1Lens;
  if (path === "/books/b1/context-lens/2") throw new Error("HTTP 404：无该章留痕");
  if (path === "/books/b2/context-lens") return { chapters: [] };
  throw new Error(`unexpected ${path}`);
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string) => fetchJsonMock(path),
}));

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
});

describe("ContextLensPanel 交互（476 号）", () => {
  it("加载后默认选中末章，渲染条目层级/徽标/压缩留痕与汇总行", async () => {
    render(<ContextLensPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByText("story_bible#世界观")).toBeTruthy();
    });
    expect(findCall("/books/b1/context-lens/3")).toBeTruthy();
    const select = screen.getByRole("combobox") as HTMLSelectElement;
    expect(select.value).toBe("3");
    // 层级中英标签与双徽标
    expect(screen.getByText("本书事实")).toBeTruthy();
    expect(screen.getByText("临时/未注册")).toBeTruthy();
    expect(screen.getByText("保护")).toBeTruthy();
    expect(screen.getByText("编译产物")).toBeTruthy();
    expect(screen.getByText("~1200")).toBeTruthy();
    // 汇总行（已编译压缩分支）
    expect(screen.getByText(/3 条来源 · 受保护 1 · 约 4200 tokens/)).toBeTruthy();
    expect(screen.getByText(/预算 8192（已编译压缩）/)).toBeTruthy();
    // 压缩留痕与备注
    expect(screen.getByText("压缩留痕：2 条可压缩来源被编译为单条摘要")).toBeTruthy();
    expect(screen.getByText("memory_0002（~1500）")).toBeTruthy();
    expect(screen.getByText(/留痕备注：/)).toBeTruthy();
    // 备注为「留痕备注：」+ join 的拼接段，逐条断言用正则
    expect(screen.getByText(/受保护来源跳过压缩/)).toBeTruthy();
  });

  it("切换章节重拉对应留痕，渲染预算内未压缩分支", async () => {
    const user = userEvent.setup();
    render(<ContextLensPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByRole("combobox")).toBeTruthy();
    });
    await user.selectOptions(screen.getByRole("combobox"), "1");
    await vi.waitFor(() => {
      expect(findCall("/books/b1/context-lens/1")).toBeTruthy();
    });
    expect(screen.getByText(/预算内未压缩/)).toBeTruthy();
    expect(screen.queryByText(/压缩留痕/)).toBeNull();
  });

  it("章数据拉取失败时错误信息透出", async () => {
    const user = userEvent.setup();
    render(<ContextLensPanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByRole("combobox")).toBeTruthy();
    });
    await user.selectOptions(screen.getByRole("combobox"), "2");
    await vi.waitFor(() => {
      expect(screen.getByText("HTTP 404：无该章留痕")).toBeTruthy();
    });
  });

  it("无装配留痕时渲染空态文案", async () => {
    render(<ContextLensPanel bookId="b2" />);
    await vi.waitFor(() => {
      expect(screen.getByText(/暂无装配留痕/)).toBeTruthy();
    });
  });
});

function findCall(path: string) {
  return fetchJsonMock.mock.calls.find(([p]) => p === path);
}
