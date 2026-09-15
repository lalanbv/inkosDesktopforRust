// @vitest-environment jsdom
//! 479 号：RunLogPanel 交互测试——遥测行渲染（倒序/耗时格式化/三态结果 chip/
//! 接管徽标）、瞬态 chip 与环形缓冲逐出提示、空缓冲与拉取失败整卡不渲染。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { RunLogPanel } from "./RunLogPanel";

const entries = [
  { ts: "2026-09-15T10:00:05.000Z", agent: "planner", model: "glm-5", durationMs: 500, ok: true, attemptIndex: 0, round: 1, tookOver: false, errorKind: null },
  { ts: "2026-09-15T10:01:10.000Z", agent: "writer", model: "glm-5-air", durationMs: 1500, ok: false, attemptIndex: 2, round: 2, tookOver: true, errorKind: "fatal" },
  { ts: "2026-09-15T10:02:20.000Z", agent: "reviser", model: "glm-5-flash", durationMs: 300, ok: false, attemptIndex: 0, round: 1, tookOver: false, errorKind: "transient" },
];

const fetchJsonMock = vi.fn(async (path: string) => {
  if (path === "/run-log?limit=50") {
    return { total: 200, kept: 3, tookOverCount: 1, failureCount: 2, entries };
  }
  throw new Error(`unexpected ${path}`);
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string) => fetchJsonMock(path),
}));

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
});

describe("RunLogPanel 交互（479 号）", () => {
  it("遥测行倒序渲染，耗时格式化、三态 chip 与接管徽标齐备", async () => {
    const { container } = render(<RunLogPanel />);
    await vi.waitFor(() => {
      expect(screen.getByText("reviser")).toBeTruthy();
    });
    // 倒序：最新（reviser）在最前
    const list = container.querySelector("ul");
    const firstRow = list?.querySelector("li");
    expect(firstRow?.textContent).toContain("reviser");
    // 汇总计数
    expect(screen.getByText("200")).toBeTruthy();
    expect(screen.getByText("2")).toBeTruthy(); // 失败
    expect(screen.getByText("1")).toBeTruthy(); // 接管
    // 耗时格式化：500ms 与 1.5s
    expect(screen.getByText("500ms")).toBeTruthy();
    expect(screen.getByText("1.5s")).toBeTruthy();
    // 三态 chip：成功 / 失败（fatal）；接管徽标带 attempt/round
    expect(screen.getByText("成功")).toBeTruthy();
    expect(screen.getByText("失败")).toBeTruthy();
    expect(screen.getByText("接管 · #2/R2")).toBeTruthy();
  });

  it("瞬态失败显示重试 chip，环形缓冲逐出提示在 total 超出时出现", async () => {
    render(<RunLogPanel />);
    await vi.waitFor(() => {
      expect(screen.getByText("瞬态")).toBeTruthy();
    });
    expect(screen.getByText(/仅显示最近记录/)).toBeTruthy();
  });

  it("空缓冲与拉取失败均整卡不渲染", async () => {
    fetchJsonMock.mockResolvedValueOnce({ total: 0, kept: 0, tookOverCount: 0, failureCount: 0, entries: [] });
    const { container: emptyContainer } = render(<RunLogPanel />);
    await vi.waitFor(() => {
      expect(fetchJsonMock).toHaveBeenCalled();
    });
    expect(emptyContainer.textContent).toBe("");
    cleanup();
    fetchJsonMock.mockRejectedValueOnce(new Error("HTTP 500"));
    const { container: failContainer } = render(<RunLogPanel />);
    await vi.waitFor(() => {
      expect(fetchJsonMock).toHaveBeenCalledTimes(2);
    });
    expect(failContainer.textContent).toBe("");
  });
});
