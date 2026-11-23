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

/**
 * 1666 号：R55 预算确认门的交互边界（1661 落地时零 UI 测试——只确认了既有
 * interaction 测复跑兼容）。三处边界：
 * - 判据=**当前所用服务**的预算行，而非「任意服务需确认」（服务 A 超支不该
 *   拦住在服务 B 上的对话）；
 * - 确认旁路一次性：确认后的 onSend 若早退，旁路不得残留到下一条消息；
 * - 弹层打开期间换会话 = 待确认发送作废（不把旧会话文本发进新会话）。
 */
describe("ChatPage 预算确认门（1666 交互边界）", () => {
  type BudgetRow = { serviceKey: string; ratio: number; level: string; confirmRequired: boolean };
  const over = (serviceKey: string): BudgetRow => ({ serviceKey, ratio: 1.2, level: "over", confirmRequired: true });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  function mount(selectedService: string | null, statuses: BudgetRow[]) {
    // 形参显式声明（调用面逐参断言需要非零长元组类型；运行时为 spy 换装）。
    const sendMessageSpy = vi.fn(
      async (_sessionId: string, _text: string, _opts?: unknown) => {},
    );
    // 会话对象须是真实形态（bookId:null + isDraft）——挂载时的会话引导只沿用
    // 这种草稿会话，否则新建草稿顶替 activeSessionId（门拦截的 await 期间发生）。
    const draft = (sessionId: string, title: string) => ({ sessionId, title, bookId: null, isDraft: true, messages: [] });
    useChatStore.setState({
      input: "",
      activeSessionId: "s1",
      sessions: { s1: draft("s1", "预算门"), s2: draft("s2", "另一会话") },
      messagesBySession: {},
      sendMessage: sendMessageSpy,
      creatingWorkSession: false,
      chatStreaming: false,
      loading: false,
      selectedService,
    } as never);
    useServiceStore.setState({ services: [], servicesLoading: false });
    usePreferencesStore.setState({ focusMode: false, toolDetailsDefaultOpen: false } as never);
    useApiMock.mockImplementation(() => ({ data: undefined, refetch: () => undefined }));
    let budgetFetches = 0;
    fetchJsonMock.mockImplementation((path: string) => {
      if (path === "/usage/budget-status") {
        budgetFetches += 1;
        return Promise.resolve({ statuses });
      }
      return Promise.resolve({});
    });
    const view = render(<ChatPage mode="project-chat" nav={nav} theme={theme} t={t} sse={sse} />);
    // onSend：await fetchJson → await 附件序列化 → sendMessage；多冲几轮微任务。
    const flush = async () => {
      for (let round = 0; round < 6; round += 1) await act(async () => {});
    };
    const textarea = () => screen.getByPlaceholderText("Enter command...");
    const type = (text: string) => fireEvent.change(textarea(), { target: { value: text } });
    const send = async (text: string) => {
      type(text);
      fireEvent.keyDown(textarea(), { key: "Enter" });
      await flush();
    };
    const confirmButton = () => screen.queryByText("Send anyway");
    return { sendMessageSpy, flush, send, confirmButton, budgetFetches: () => budgetFetches, textarea,
      // 同一组件实例换 props（state 保留）：用于在弹层打开期间改变 onSend 的早退条件。
      rerenderWith: (activeBookId: string | undefined) =>
        view.rerender(<ChatPage activeBookId={activeBookId} mode="project-chat" nav={nav} theme={theme} t={t} sse={sse} />) };
  }

  it("其他服务超预算不拦——只看当前所用服务的预算行", async () => {
    const { sendMessageSpy, confirmButton, send } = mount("svc-a", [over("svc-b")]);
    await send("用服务 A 正常发送");
    expect(confirmButton()).toBeNull();
    expect(sendMessageSpy).toHaveBeenCalledTimes(1);
    expect(sendMessageSpy.mock.calls[0]?.[0]).toBe("s1");
    expect(sendMessageSpy.mock.calls[0]?.[1]).toBe("用服务 A 正常发送");
  });

  it("当前服务超预算 → 弹层点名该服务、不发送；『仍要发送』后恰发一次，下一条仍过门", async () => {
    const { sendMessageSpy, confirmButton, send, flush, textarea } = mount("svc-b", [over("svc-b"), { serviceKey: "svc-a", ratio: 0.1, level: "ok", confirmRequired: false }]);
    await send("第一条");
    expect(sendMessageSpy).not.toHaveBeenCalled();
    expect(confirmButton()).not.toBeNull();
    expect(screen.getByText(/The service in use \(svc-b\) has reached/)).toBeTruthy();
    // 取消 = 不发送、输入保留。
    fireEvent.click(screen.getByText("Cancel"));
    await flush();
    expect(confirmButton()).toBeNull();
    expect(sendMessageSpy).not.toHaveBeenCalled();
    expect(useChatStore.getState().input).toBe("第一条");
    // 再按回车 → 再次弹层；「仍要发送」→ 恰发一次（原文本）。
    fireEvent.keyDown(textarea(), { key: "Enter" });
    await flush();
    expect(confirmButton()).not.toBeNull();
    fireEvent.click(confirmButton()!);
    await flush();
    expect(confirmButton()).toBeNull();
    expect(sendMessageSpy).toHaveBeenCalledTimes(1);
    expect(sendMessageSpy.mock.calls[0]?.[1]).toBe("第一条");
    // 旁路一次性：下一条消息照样过门（服务仍超预算 → 再次弹层，不直发）。
    await send("第二条");
    expect(sendMessageSpy).toHaveBeenCalledTimes(1);
    expect(confirmButton()).not.toBeNull();
  });

  it("弹层打开期间路由切走（会话随之更换）→ 弹层作废、不发；回来后仍过门", async () => {
    const { sendMessageSpy, confirmButton, send, flush, rerenderWith } = mount("svc-b", [over("svc-b")]);
    await send("发给会话一");
    expect(confirmButton()).not.toBeNull();
    // 路由改指向一本书 → 会话引导为该书另起会话 → activeSessionId 变化。
    rerenderWith("book-early-return");
    await flush();
    expect(confirmButton()).toBeNull();
    expect(sendMessageSpy).not.toHaveBeenCalled();
    // 切回原路由：仍超预算 → 照样过门（旁路从未被点亮，也无从残留）。
    rerenderWith(undefined);
    await flush();
    await send("不该绕过确认门");
    expect(sendMessageSpy).not.toHaveBeenCalled();
    expect(confirmButton()).not.toBeNull();
  });

  it("弹层打开期间切换会话 → 弹层作废，旧文本不会发进新会话", async () => {
    const { sendMessageSpy, confirmButton, send, flush } = mount("svc-b", [over("svc-b")]);
    await send("发给会话一");
    expect(confirmButton()).not.toBeNull();
    act(() => { useChatStore.setState({ activeSessionId: "s2" } as never); });
    await flush();
    expect(confirmButton()).toBeNull();
    expect(sendMessageSpy).not.toHaveBeenCalled();
  });

  it("服务未知（选择未就绪）→ 退回『任意服务需确认』；预算面不可达 → 不拦发送", async () => {
    const first = mount(null, [over("svc-b")]);
    await first.send("服务未知");
    expect(first.confirmButton()).not.toBeNull();
    cleanup();
    const sendMessageSpy = vi.fn(async () => {});
    useChatStore.setState({ input: "", activeSessionId: "s1", messagesBySession: {}, sendMessage: sendMessageSpy } as never);
    fetchJsonMock.mockImplementation((path: string) =>
      path === "/usage/budget-status" ? Promise.reject(new Error("budget face down")) : Promise.resolve({}));
    render(<ChatPage mode="project-chat" nav={nav} theme={theme} t={t} sse={sse} />);
    const textarea = screen.getByPlaceholderText("Enter command...");
    fireEvent.change(textarea, { target: { value: "预算面挂了也照发" } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    for (let round = 0; round < 6; round += 1) await act(async () => {});
    expect(sendMessageSpy).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Send anyway")).toBeNull();
  });
});
