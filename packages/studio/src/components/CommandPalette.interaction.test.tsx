// @vitest-environment jsdom
//! 474 号：CommandPalette 交互测试——空查询首屏（最近访问+推荐）、
//! 查询过滤命中导航/动作命令并执行、无匹配兜底（创建新书）四条链。
//! lib 层纯函数已由 lib/commands.test.ts 覆盖，此处只锁组件接线。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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
import { CommandPalette } from "./CommandPalette";
import { useRecentsStore } from "@/store/recents";
import type { RecentEntry } from "@/store/recents";
import type { CommandContext } from "@/lib/commands";

const onOpenChange = vi.fn();
const routes: unknown[] = [];
const calls: string[] = [];

// 与 App.commandCtx 同构的最小桩：按调用序记录（lib/commands.test.ts 同款）。
const ctx: CommandContext = {
  setRoute: (route) => routes.push(route),
  setThemeMode: (mode) => calls.push(`theme:${mode}`),
  setDensity: (density) => calls.push(`density:${density}`),
  setProjectLanguage: (lang) => calls.push(`lang:${lang}`),
  refetchProject: () => calls.push("refresh"),
  openBookCreate: () => calls.push("bookCreate"),
  createProjectChatDraft: () => calls.push("draft"),
  launchProjectMode: (kind, playMode) => calls.push(`launch:${kind}:${playMode ?? ""}`),
};

const seedRecents: RecentEntry[] = [
  { page: "book", label: "镜花水月", bookId: "b1" },
  { page: "logs", label: "日志页" },
];

function renderPalette() {
  render(<CommandPalette open onOpenChange={onOpenChange} ctx={ctx} lang="zh" />);
}

beforeEach(() => {
  useRecentsStore.setState({ recents: seedRecents });
});

afterEach(() => {
  cleanup();
  useRecentsStore.setState({ recents: [] });
  onOpenChange.mockClear();
  routes.length = 0;
  calls.length = 0;
});

describe("CommandPalette 交互（474 号）", () => {
  it("空查询首屏渲染最近访问组与推荐组，不显示全量导航命令", () => {
    renderPalette();
    // 书名同时出现在最近访问项与推荐第二位——按出现次数断言
    expect(screen.getAllByText("镜花水月")).toHaveLength(2);
    expect(screen.getByText("日志页")).toBeTruthy();
    expect(screen.getByText("新建长篇小说")).toBeTruthy();
    expect(screen.getByText("项目设置")).toBeTruthy();
    // 全量导航/动作命令不在空查询首屏
    expect(screen.queryByText("题材管理")).toBeNull();
    expect(screen.queryByText("切换到深色主题")).toBeNull();
  });

  it("输入中文查询过滤出导航命令，选中后导航并关闭弹层", async () => {
    const user = userEvent.setup();
    renderPalette();
    const input = screen.getByPlaceholderText("输入命令或搜索…") as HTMLInputElement;
    await user.type(input, "题材");
    await vi.waitFor(() => {
      expect(screen.getByText("题材管理")).toBeTruthy();
    });
    await user.click(screen.getByText("题材管理"));
    expect(routes).toContainEqual({ page: "genres" });
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("输入查询过滤出动作命令，选中后经 ctx 执行且不动路由", async () => {
    const user = userEvent.setup();
    renderPalette();
    const input = screen.getByPlaceholderText("输入命令或搜索…") as HTMLInputElement;
    await user.type(input, "深色");
    await vi.waitFor(() => {
      expect(screen.getByText("切换到深色主题")).toBeTruthy();
    });
    await user.click(screen.getByText("切换到深色主题"));
    expect(calls).toContain("theme:dark");
    expect(routes).toHaveLength(0);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("无匹配查询渲染空态兜底，执行创建新书并关闭弹层", async () => {
    const user = userEvent.setup();
    renderPalette();
    const input = screen.getByPlaceholderText("输入命令或搜索…") as HTMLInputElement;
    await user.type(input, "zzz-不存在");
    await vi.waitFor(() => {
      expect(screen.getByText("没有匹配的命令")).toBeTruthy();
    });
    await user.click(screen.getByText("创建新书"));
    expect(calls).toContain("bookCreate");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
