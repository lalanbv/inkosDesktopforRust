// @vitest-environment jsdom
//! 471 号：QuickOpenPalette 交互测试——书籍/章节索引拉取、查询过滤、
//! 选中导航（onNavigate 载荷 + 弹层关闭）与无匹配空态。
import { afterEach, describe, expect, it, vi } from "vitest";

// jsdom 无 ResizeObserver——cmdk/radix 依赖，桩化（461 基建惯例）。
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = ResizeObserverStub;
// cmdk 选中项自动滚动依赖 Element.scrollIntoView，jsdom 同样缺失
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QuickOpenPalette } from "./QuickOpenPalette";

const fetchJsonMock = vi.fn(async (path: string) => {
  if (path === "/interactive-films") {
    return { films: [{ projectId: "p1", title: "悬疑影游" }] };
  }
  if (path === "/books/b1") {
    return { chapters: [{ number: 1, title: "镜中醒来" }] };
  }
  return {};
});

vi.mock("@/hooks/use-api", () => ({
  fetchJson: (path: string) => fetchJsonMock(path),
}));

const onOpenChange = vi.fn();
const onNavigate = vi.fn();

function renderPalette() {
  return render(
    <QuickOpenPalette
      open
      onOpenChange={onOpenChange}
      books={[{ id: "b1", title: "镜花水月" }]}
      sessions={[{ sessionId: "s1", title: "新会话", bookId: "b1" }]}
      onNavigate={onNavigate}
    />,
  );
}

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
  onOpenChange.mockClear();
  onNavigate.mockClear();
});

describe("QuickOpenPalette 交互（471 号）", () => {
  it("开启即渲染书籍/会话分组与影游项", async () => {
    renderPalette();
    await vi.waitFor(() => {
      expect(screen.getByText("悬疑影游")).toBeTruthy();
    });
    // 书名同时出现在书籍项与会话 detail——按出现次数断言
    expect(screen.getAllByText("镜花水月").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("新会话")).toBeTruthy();
  });

  it("输入章节标题过滤出章节项，选中后导航并关闭弹层", async () => {
    const user = userEvent.setup();
    renderPalette();
    const input = screen.getByPlaceholderText("跳转到…") as HTMLInputElement;
    await user.type(input, "镜中醒来");
    await vi.waitFor(() => {
      expect(screen.getByText("镜中醒来")).toBeTruthy();
    });
    await user.click(screen.getByText("镜中醒来"));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onNavigate).toHaveBeenCalledWith({ page: "chapter", bookId: "b1", chapterNumber: 1 });
  });

  it("无匹配查询渲染空态文案", async () => {
    const user = userEvent.setup();
    renderPalette();
    const input = screen.getByPlaceholderText("跳转到…") as HTMLInputElement;
    await user.type(input, "zzz-不存在");
    await vi.waitFor(() => {
      expect(screen.getByText("没有匹配的内容")).toBeTruthy();
    });
  });
});
