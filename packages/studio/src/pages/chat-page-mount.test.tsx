import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";

/**
 * 741 号：ChatPage 挂载冒烟（大页另案转正第一批）——1423 行核心页
 * 空会话态挂载不崩 + 核心区块在位。store 全家桶经 zustand setState 预填。
 */

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

import { ChatPage } from "@/pages/ChatPage";
import { useChatStore } from "@/store/chat";
import { useServiceStore } from "@/store/service";
import { usePreferencesStore } from "@/store/preferences";

describe("ChatPage mount smoke (741)", () => {
  it("空会话态挂载不崩 + 输入指令区在位", () => {
    useChatStore.setState({
      input: "",
      activeSessionId: null,
      sessions: {},
      messagesBySession: {},
    } as never);
    useServiceStore.setState({ services: [], servicesLoading: false });
    usePreferencesStore.setState({ focusMode: false, toolDetailsDefaultOpen: false } as never);
    useApiMock.mockImplementation(() => ({ data: undefined, refetch: () => undefined }));
    fetchJsonMock.mockResolvedValue({});
    postApiMock.mockResolvedValue({});

    const html = renderToString(
      <ChatPage
        mode="project-chat"
        nav={nav}
        theme={theme}
        t={t}
        sse={sse}
      />,
    );
    // t mock 直返键名 → isZh 判定 false → 英文 placeholder 分支。
    expect(html).toContain("Enter command...");
    expect(html).toContain('aria-label="Send"');
    expect(html.length).toBeGreaterThan(500);
  });
});
