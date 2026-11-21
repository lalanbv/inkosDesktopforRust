// @vitest-environment jsdom
//! 1655 号：DaemonControl 通知条交互测试——start/stop 失败 alert→页内
//! role=status 条（wry 壳内 alert 静默族批三）。清零面由
//! no-native-dialogs.test.ts 全仓守卫锁定。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import type { SSEMessage } from "../hooks/use-sse";
import { DaemonControl } from "./DaemonControl";

const postApiMock = vi.hoisted(() => vi.fn());
const useApiMock = vi.hoisted(() => vi.fn());

vi.mock("../hooks/use-api", () => ({
  postApi: (path: string) => postApiMock(path),
  useApi: () => useApiMock(),
}));

const baseProps = {
  nav: { toDashboard: () => {} },
  theme: "light" as const,
  t: (key: string) => key,
  sse: { messages: [] as ReadonlyArray<SSEMessage> },
};

describe("DaemonControl 通知条（1655 号）", () => {
  beforeEach(() => {
    postApiMock.mockReset();
    useApiMock.mockReset();
    useApiMock.mockReturnValue({ data: { running: false }, refetch: () => Promise.resolve() });
  });
  afterEach(() => cleanup());

  it("start 失败 → 页内通知条可见（原 alert 壳内静默）", async () => {
    postApiMock.mockRejectedValue(new Error("engine unreachable"));
    render(<DaemonControl {...baseProps} />);
    const startButton = screen.getAllByRole("button").find((button) => button.textContent?.toLowerCase().includes("start"));
    expect(startButton).toBeTruthy();
    startButton!.click();
    await waitFor(() => {
      const notice = document.querySelector('[data-testid="daemon-notice"]');
      expect(notice?.getAttribute("role")).toBe("status");
      expect(notice?.textContent).toContain("engine unreachable");
    });
  });

  it("stop 失败同款通知条", async () => {
    useApiMock.mockReturnValue({ data: { running: true }, refetch: () => Promise.resolve() });
    postApiMock.mockRejectedValue(new Error("stop refused"));
    render(<DaemonControl {...baseProps} />);
    const stopButton = screen.getAllByRole("button").find((button) => button.textContent?.toLowerCase().includes("stop"));
    stopButton!.click();
    await waitFor(() => {
      expect(document.querySelector('[data-testid="daemon-notice"]')?.textContent).toContain("stop refused");
    });
  });
});
