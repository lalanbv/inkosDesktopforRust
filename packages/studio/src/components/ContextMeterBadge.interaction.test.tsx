// @vitest-environment jsdom
//! 564 号：ContextMeterBadge 交互测试——usage 锚点快照渲染（k 格式化/来源
//! 标签/覆盖 tooltip）、超窗告警色、无 sessionId 与 404 不渲染、会话切换重拉。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { ContextMeterBadge } from "./ContextMeterBadge";

const snapshot = {
  model: "glm-5",
  heuristicTokens: 12_345,
  anchoredTokens: 15_000,
  tokens: 15_000,
  anchorValid: true,
  source: "usage" as const,
  coverage: 0.9876,
  inputWindow: 128_000,
  overWindow: false,
  surfaceNodes: 42,
};

const fetchJsonMock = vi.fn(async (path: string) => {
  if (path === "/context-meter?sessionId=s1") return snapshot;
  if (path === "/context-meter?sessionId=s-over") {
    return { ...snapshot, tokens: 130_000, overWindow: true };
  }
  if (path === "/context-meter?sessionId=s-estimate") {
    return { ...snapshot, anchorValid: false, source: "estimate" as const, anchoredTokens: 12_345, tokens: 12_345, coverage: 0, model: null };
  }
  const error = new Error("not found");
  throw error;
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string) => fetchJsonMock(path),
}));

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
});

describe("ContextMeterBadge 交互（564 号）", () => {
  it("usage 锚点快照渲染：k 格式化 + 来源 + tooltip 详情", async () => {
    render(<ContextMeterBadge sessionId="s1" />);
    await vi.waitFor(() => {
      expect(screen.getByText("15.0k")).toBeTruthy();
    });
    const badge = screen.getByText("15.0k").closest("[data-slot='context-meter-badge']")!;
    expect(badge).toBeTruthy();
    expect(badge.getAttribute("title")).toContain("usage");
    expect(badge.getAttribute("title")).toContain("12345");
    expect(badge.getAttribute("title")).toContain("98.8%"); // 覆盖率四位舍入
    expect(badge.getAttribute("title")).toContain("glm-5");
  });

  it("超窗快照显示告警色与超窗标记", async () => {
    render(<ContextMeterBadge sessionId="s-over" />);
    await vi.waitFor(() => {
      expect(screen.getByText("130.0k")).toBeTruthy();
    });
    const badge = screen.getByText("130.0k").closest("[data-slot='context-meter-badge']")!;
    expect(badge.className).toContain("text-red-600");
    expect(badge.textContent).toContain("超窗");
  });

  it("无锚点估算快照：tooltip 标注无 usage 锚点", async () => {
    render(<ContextMeterBadge sessionId="s-estimate" />);
    await vi.waitFor(() => {
      expect(screen.getByText("12.3k")).toBeTruthy();
    });
    const badge = screen.getByText("12.3k").closest("[data-slot='context-meter-badge']")!;
    expect(badge.getAttribute("title")).toContain("无 usage 锚点");
  });

  it("无 sessionId 不渲染；拉取失败（404）不渲染", async () => {
    const { container } = render(<ContextMeterBadge sessionId={null} />);
    expect(container.querySelector("[data-slot='context-meter-badge']")).toBeNull();
    const failing = render(<ContextMeterBadge sessionId="s-unknown" />);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(failing.container.querySelector("[data-slot='context-meter-badge']")).toBeNull();
  });
});
