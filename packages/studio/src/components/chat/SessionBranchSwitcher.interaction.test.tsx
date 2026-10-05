// @vitest-environment jsdom
//! 638 号：SessionBranchSwitcher 交互测试——draft/无会话不渲染、面板点位
//! 渲染（当前徽章/弃用分支标记/预览截断/轮号倒序）、确认弹窗→branchSession
//! 载荷、409 错误展示、拉取失败与空点位态。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { SessionBranchSwitcher } from "./SessionBranchSwitcher";
import type { SessionBranchPointsResponse } from "../../store/chat/types";

const now = Date.now();

const branchesPayload: SessionBranchPointsResponse = {
  sessionId: "s1",
  head: 12,
  branchCount: 1,
  points: [
    { seq: 12, requestId: "r2b", timestamp: now - 30_000, preview: "第二轮重写的问题", onActiveChain: true },
    { seq: 8, requestId: "r2", timestamp: now - 3 * 60 * 60_000, preview: "被弃用第二轮的问题".repeat(6), onActiveChain: false }, // 54 字 > 42 → JS 截断 + …
    { seq: 4, requestId: "r1", timestamp: now - 26 * 60 * 60_000, preview: "第一轮的问题", onActiveChain: true },
  ],
};

const fetchJsonMock = vi.fn(async (path: string) => {
  if (path === "/sessions/s1/branches") return branchesPayload;
  if (path === "/sessions/s-empty/branches") {
    return { sessionId: "s-empty", head: 0, branchCount: 0, points: [] };
  }
  throw new Error("not found");
});

const branchSessionMock = vi.fn(async (_sessionId: string, _toSeq: number) => ({
  head: _toSeq,
  branchCount: 1,
}));

vi.mock("../../hooks/use-api", () => ({
  fetchJson: (path: string) => fetchJsonMock(path),
}));

vi.mock("../../store/chat", () => ({
  useChatStore: (selector: (state: { branchSession: typeof branchSessionMock }) => unknown) =>
    selector({ branchSession: branchSessionMock }),
}));

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
  branchSessionMock.mockClear();
  branchSessionMock.mockImplementation(async (sessionId: string, toSeq: number) => ({ head: toSeq, branchCount: 1 }));
});

function renderSwitcher(props: Partial<Parameters<typeof SessionBranchSwitcher>[0]> = {}) {
  return render(
    <SessionBranchSwitcher
      sessionId="s1"
      isDraft={false}
      disabled={false}
      isZh={true}
      {...props}
    />,
  );
}

async function openPanel() {
  fireEvent.click(document.querySelector("[data-slot='session-branch-trigger']")!);
  await vi.waitFor(() => {
    expect(document.querySelector("[data-slot='session-branch-panel']")).not.toBeNull();
  });
}

describe("SessionBranchSwitcher 交互（638 号）", () => {
  it("无 sessionId 与 draft 会话不渲染", () => {
    const none = render(<SessionBranchSwitcher sessionId={null} isDraft={false} disabled={false} isZh />);
    expect(none.container.querySelector("[data-slot='session-branch-switcher']")).toBeNull();
    const draft = render(<SessionBranchSwitcher sessionId="s1" isDraft={true} disabled={false} isZh />);
    expect(draft.container.querySelector("[data-slot='session-branch-switcher']")).toBeNull();
  });

  it("打开面板：轮号倒序 + 当前徽章 + 弃用标记 + 长预览截断 + 相对时间", async () => {
    renderSwitcher();
    await openPanel();
    await vi.waitFor(() => {
      expect(document.querySelectorAll("[data-slot='session-branch-point']")).toHaveLength(3);
    });
    const points = document.querySelectorAll("[data-slot='session-branch-point']");
    expect(points).toHaveLength(3);
    // seq 降序：首轮点击位是 seq 12（第二轮重写）
    expect(points[0]!.textContent).toContain("第 3 轮");
    expect(points[0]!.textContent).toContain("第二轮重写的问题");
    expect(points[0]!.getAttribute("disabled")).not.toBeNull(); // 当前点禁点
    expect(document.querySelector("[data-slot='session-branch-current']")!.textContent).toBe("当前");
    expect(document.querySelector("[data-slot='session-branch-abandoned']")!.textContent).toBe("已弃用分支");
    // 长预览截断（>42 字 → 42 字 + …）
    expect(points[1]!.textContent).toContain("…");
    expect(points[2]!.textContent).toContain("第 1 轮");
    expect(points[2]!.textContent).toContain("1 天前");
    // branchCount>0 显示计数徽标
    expect(document.querySelector("[data-slot='session-branch-count']")!.textContent).toBe("1");
  });

  it("点击历史点 → 确认弹窗 → 确认后 branchSession(sessionId, seq) 且面板关闭", async () => {
    renderSwitcher();
    await openPanel();
    await vi.waitFor(() => {
      expect(document.querySelectorAll("[data-slot='session-branch-point']")).toHaveLength(3);
    });
    const points = document.querySelectorAll("[data-slot='session-branch-point']");
    fireEvent.click(points[2]!); // seq 4（第一轮）
    await vi.waitFor(() => {
      expect(screen.getByText("切换对话分支")).toBeTruthy();
    });
    expect(screen.getByText(/回到「第一轮的问题」之后继续对话/)).toBeTruthy();
    fireEvent.click(screen.getByText("切到此分支"));
    await vi.waitFor(() => {
      expect(branchSessionMock).toHaveBeenCalledWith("s1", 4);
    });
    await vi.waitFor(() => {
      expect(document.querySelector("[data-slot='session-branch-panel']")).toBeNull();
    });
  });

  it("branchSession 拒绝（409 忙）→ 错误消息留在面板", async () => {
    branchSessionMock.mockRejectedValue(new Error("Session has an in-flight request; abort it before branching"));
    renderSwitcher();
    await openPanel();
    await vi.waitFor(() => {
      expect(document.querySelectorAll("[data-slot='session-branch-point']")).toHaveLength(3);
    });
    const points = document.querySelectorAll("[data-slot='session-branch-point']");
    fireEvent.click(points[2]!);
    await vi.waitFor(() => {
      expect(screen.getByText("切换对话分支")).toBeTruthy();
    });
    fireEvent.click(screen.getByText("切到此分支"));
    await vi.waitFor(() => {
      const errorBox = document.querySelector("[data-slot='session-branch-error']");
      expect(errorBox).not.toBeNull();
      expect(errorBox!.textContent).toContain("in-flight");
    });
    // 面板保持打开，可重试
    expect(document.querySelector("[data-slot='session-branch-panel']")).not.toBeNull();
  });

  it("拉取失败 → 失败文案；空点位 → 空态文案", async () => {
    renderSwitcher({ sessionId: "s-unknown" });
    fireEvent.click(document.querySelector("[data-slot='session-branch-trigger']")!);
    await vi.waitFor(() => {
      expect(document.querySelector("[data-slot='session-branch-panel']")!.textContent).toContain("分支点加载失败");
    });

    cleanup();
    renderSwitcher({ sessionId: "s-empty" });
    fireEvent.click(document.querySelector("[data-slot='session-branch-trigger']")!);
    await vi.waitFor(() => {
      expect(document.querySelector("[data-slot='session-branch-panel']")!.textContent).toContain("还没有已完成的对话轮");
    });
  });
});
