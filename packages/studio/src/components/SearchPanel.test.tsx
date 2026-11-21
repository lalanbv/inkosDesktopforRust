// @vitest-environment jsdom
//! R47/1640 号：全局搜索面板冒烟测试——四轴 scope 切换、防抖检索 hit 渲染、
//! <mark> 高亮定位、章节命中点击走 chapter-focus + toChapter。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { SearchPanel } from "./SearchPanel";

const fetchJsonMock = vi.fn();
vi.mock("@/hooks/use-api", () => ({
  fetchJson: (...args: unknown[]) => fetchJsonMock(...(args as [])),
}));
// use-i18n 整层 mock（zh 直映）：隔离其内部 useApi("/project") 网络面，
// 且键面断言与词典同源（缺键直接暴露为裸键文本）。t 必须引用稳定——
// 不稳定引用会经组件 effect 重触发形成无限循环（worker 挂死实证）。
const MOCK_T = (key: string): string => {
  const zh: Record<string, string> = {
    "search.title": "搜索",
    "search.placeholder": "搜索正文、实体、伏笔与真相…",
    "search.scope.chapters": "正文",
    "search.scope.entity": "实体",
    "search.scope.hook": "伏笔",
    "search.scope.truth": "真相",
    "search.scopeAria": "搜索范围",
    "search.searching": "搜索中…",
    "search.noHits": "无命中",
    "search.failed": "搜索失败",
  };
  return zh[key] ?? key;
};
vi.mock("@/hooks/use-i18n", () => ({
  useI18n: () => ({ t: MOCK_T, lang: "zh" as const }),
}));

const nav = {
  toChapter: vi.fn(),
  toTruth: vi.fn(),
};

describe("SearchPanel（R47/1640 号）", () => {
  afterEach(() => {
    cleanup();
    fetchJsonMock.mockReset();
    nav.toChapter.mockClear();
    nav.toTruth.mockClear();
  });

  it("输入防抖后渲染命中列表，章节命中点击携带一次性焦点跳章", async () => {
    fetchJsonMock.mockResolvedValueOnce({
      query: "碎镜",
      scope: "chapters",
      hits: [
        {
          kind: "chapter",
          id: "chapter:2",
          title: "灵脉",
          source: "0002_灵脉.md",
          score: 1.5,
          snippet: "灵气紊乱，碎镜重现。",
          matchStart: 5,
          matchLength: 2,
          chapterNumber: 2,
        },
      ],
    });
    render(<SearchPanel bookId="b1" nav={nav} />);
    const input = screen.getByPlaceholderText("搜索正文、实体、伏笔与真相…");
    await act(async () => {
      fireEvent.change(input, { target: { value: "碎镜" } });
    });
    // 防抖 250ms 窗口 + 微任务 flush。
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 300));
    });
    expect(fetchJsonMock).toHaveBeenCalledWith(
      "/books/b1/search?q=%E7%A2%8E%E9%95%9C&scope=chapters&limit=20",
    );
    expect(screen.getByText("灵脉")).toBeTruthy();
    const mark = document.querySelector("mark");
    expect(mark?.textContent).toBe("碎镜");
    await act(async () => {
      fireEvent.click(screen.getByText("灵脉"));
    });
    expect(nav.toChapter).toHaveBeenCalledWith("b1", 2);
  });

  it("truth 命中点击跳真相页（无需一次性焦点）", async () => {
    fetchJsonMock.mockResolvedValueOnce({
      query: "之谜",
      scope: "truth",
      hits: [
        {
          kind: "truth",
          id: "truth:story_bible.md",
          title: "story_bible.md",
          source: "story_bible.md",
          score: 1,
          snippet: "碎镜崖之谜待解",
          matchStart: 3,
          matchLength: 2,
        },
      ],
    });
    render(<SearchPanel bookId="b1" nav={nav} />);
    const input = screen.getByPlaceholderText("搜索正文、实体、伏笔与真相…");
    await act(async () => {
      fireEvent.change(input, { target: { value: "之谜" } });
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 300));
    });
    await act(async () => {
      fireEvent.click(screen.getByText("story_bible.md"));
    });
    expect(nav.toTruth).toHaveBeenCalledWith("b1");
    expect(nav.toChapter).not.toHaveBeenCalled();
  });
});
