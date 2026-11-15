// @vitest-environment jsdom
//! 1428 号：ChatPage 聊天主输入 IME 组合回车守卫逐功能回归（1169 修复面站 1
//! 专属自动化——此前仅有活体走查证据，无单测兜底）。
//! 三腿断言（守卫面 `!e.shiftKey && !e.nativeEvent.isComposing` 全覆盖）：
//! ①组合期回车（isComposing，中文选词上屏）吞没——sendMessage 0 调用+输入保留；
//! ②普通回车——恰 1 次发送（activeSessionId+文本逐参断言）；
//! ③shift+Enter（换行路径）——0 次发送（!e.shiftKey 半区回归）。
//! 可观测面 = useChatStore.sendMessage 注入 spy（ChatPage :333 经 selector 读取，
//! setState 先于 render 即可换装）。
//! 时序注意：onSend 内 `await serializeChatAttachments([])` 把 sendMessage 推迟
//! 到微任务——断言前必须 `await act(async () => {})` flush，否则 spy 计数恒 0 假红。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ChatPage } from "./ChatPage";
import { useChatStore } from "@/store/chat";
import { useServiceStore } from "@/store/service";
import { usePreferencesStore } from "@/store/preferences";

const useApiMock = vi.fn();
const fetchJsonMock = vi.fn();
const postApiMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: (...args: unknown[]) => fetchJsonMock(...(args as [])),
  postApi: (...args: unknown[]) => postApiMock(...(args as [])),
  buildApiUrl: (path: string) => path,
}));

const nav = {
  toDashboard: () => undefined,
  toBook: () => undefined,
  toServices: () => undefined,
} as never;
const theme = "light" as const;
const t = (key: string) => key;
const sse = { messages: [], connected: true };

describe("ChatPage IME 组合回车守卫（1169 修复面·1428 逐功能补测）", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("组合回车吞没 0 发送+输入保留；普通回车恰 1 发送；shift+Enter 换行 0 发送", async () => {
    // 形参显式声明（调用面逐参断言需要非零长元组类型；运行时为 spy 换装）。
    const sendMessageSpy = vi.fn(
      async (_sessionId: string, _text: string, _opts?: unknown) => {},
    );
    useChatStore.setState({
      input: "",
      activeSessionId: "s1",
      sessions: { s1: { sessionId: "s1", title: "守卫走查" } },
      messagesBySession: {},
      sendMessage: sendMessageSpy,
      creatingWorkSession: false,
      chatStreaming: false,
      loading: false,
    } as never);
    useServiceStore.setState({ services: [], servicesLoading: false });
    usePreferencesStore.setState({ focusMode: false, toolDetailsDefaultOpen: false } as never);
    useApiMock.mockImplementation(() => ({ data: undefined, refetch: () => undefined }));
    fetchJsonMock.mockResolvedValue({});
    postApiMock.mockResolvedValue({});

    render(<ChatPage mode="project-chat" nav={nav} theme={theme} t={t} sse={sse} />);
    // t mock 直返键名 → isZh 判定 false → 英文 placeholder（741 号 mount 冒烟同款）。
    const textarea = screen.getByPlaceholderText("Enter command...");
    expect(textarea.tagName).toBe("TEXTAREA");

    fireEvent.change(textarea, { target: { value: "守卫走查文本一四二六" } });
    expect(useChatStore.getState().input).toBe("守卫走查文本一四二六");

    // 腿①：IME 组合期回车（选词上屏）——守卫必须整句吞没（无 await 需求：守卫
    // 直接 return，不进异步路径，同步断言即可）。
    fireEvent.keyDown(textarea, { key: "Enter", isComposing: true });
    expect(sendMessageSpy).not.toHaveBeenCalled();
    expect(useChatStore.getState().input).toBe("守卫走查文本一四二六");

    // 腿②：普通回车——恰 1 次发送，入参逐项断言（先 flush onSend 的附件序列化微任务）。
    fireEvent.keyDown(textarea, { key: "Enter" });
    await act(async () => {});
    expect(sendMessageSpy).toHaveBeenCalledTimes(1);
    expect(sendMessageSpy.mock.calls[0]?.[0]).toBe("s1");
    expect(sendMessageSpy.mock.calls[0]?.[1]).toBe("守卫走查文本一四二六");

    // 腿③：shift+Enter（换行路径）——不发送。
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true });
    await act(async () => {});
    expect(sendMessageSpy).toHaveBeenCalledTimes(1);
  });
});
