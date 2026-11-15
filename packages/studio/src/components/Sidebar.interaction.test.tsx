// @vitest-environment jsdom
//! 1428 号：Sidebar 会话改名 IME 组合回车守卫逐功能回归（1169 修复面站 2
//! 专属自动化——此前仅有活体走查证据无单测兜底）。
//! 双腿断言：①组合期回车（isComposing，中文会话名选词上屏）吞没——renameSession
//! 0 调用+改名 Dialog 不误关；②普通回车——恰 1 次重命名（sessionId+新标题逐参
//! 断言）+Dialog 关闭（renameTarget 复位）。
//! 可观测面 = useChatStore.renameSession 注入 spy（Sidebar :110 经 selector 读取）。
//! radix DropdownMenu 首例 jsdom 交互：scrollIntoView / hasPointerCapture 需桩。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Sidebar } from "./Sidebar";
import { useChatStore } from "../store/chat";

const useApiMock = vi.fn();
const fetchJsonMock = vi.fn();
const postApiMock = vi.fn();

vi.mock("../hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: (...args: unknown[]) => fetchJsonMock(...(args as [])),
  postApi: (...args: unknown[]) => postApiMock(...(args as [])),
  buildApiUrl: (path: string) => path,
}));

describe("Sidebar 改名 IME 组合回车守卫（1169 修复面·1428 逐功能补测）", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("组合回车吞没 0 重命名+Dialog 不误关；普通回车恰 1 重命名+Dialog 关", async () => {
    // jsdom 缺 radix 菜单依赖的 DOM API（首例交互先例备案）。
    Element.prototype.scrollIntoView = vi.fn();
    Element.prototype.hasPointerCapture = vi.fn(() => false);
    Element.prototype.setPointerCapture = vi.fn();
    Element.prototype.releasePointerCapture = vi.fn();

    // 形参显式声明（调用面逐参断言需要非零长元组类型；运行时为 spy 换装）。
    const renameSessionSpy = vi.fn(async (_sessionId: string, _title: string) => {});
    useChatStore.setState({
      activeSessionId: "s1",
      sessions: {
        s1: { sessionId: "s1", title: "原会话名" },
      },
      // "__null__" = 项目会话区键（Sidebar projectChatKey），默认展开（:117）。
      sessionIdsByBook: { __null__: ["s1"] },
      messagesBySession: {},
      renameSession: renameSessionSpy,
    } as never);
    useApiMock.mockImplementation(() => ({ data: [], refetch: () => undefined }));
    fetchJsonMock.mockResolvedValue({});
    postApiMock.mockResolvedValue({});

    const nav = { toChat: () => undefined, toDashboard: () => undefined } as never;
    const t = (key: string) => key;
    const sse = { messages: [], connected: true };
    render(<Sidebar nav={nav} activePage="chat" sse={sse} t={t} />);

    // 会话行在位（项目会话区）→ 打开 会话操作 菜单 → 改名。
    const user = userEvent.setup();
    const opsButton = await screen.findByLabelText("会话操作");
    await user.click(opsButton);
    const renameItem = await screen.findByText("改名");
    await user.click(renameItem);

    // 改名 Dialog 开启，预填现标题（活体走查同款形态）。
    const input = screen.getByPlaceholderText("输入新标题") as HTMLInputElement;
    expect(input.value).toBe("原会话名");
    fireEvent.change(input, { target: { value: "新会话名一四二六" } });

    // 腿①：IME 组合期回车（中文会话名选词上屏）——守卫必须吞没。
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(renameSessionSpy).not.toHaveBeenCalled();
    expect((screen.getByPlaceholderText("输入新标题") as HTMLInputElement).value).toBe("新会话名一四二六");

    // 腿②：普通回车——恰 1 次重命名 + Dialog 关闭（确认 promise resolve 后复位）。
    fireEvent.keyDown(input, { key: "Enter" });
    expect(renameSessionSpy).toHaveBeenCalledTimes(1);
    expect(renameSessionSpy.mock.calls[0]?.[0]).toBe("s1");
    expect(renameSessionSpy.mock.calls[0]?.[1]).toBe("新会话名一四二六");
    await waitFor(() => {
      expect(screen.queryByPlaceholderText("输入新标题")).toBeNull();
    });
  });
});
