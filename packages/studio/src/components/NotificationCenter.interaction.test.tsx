// @vitest-environment jsdom
//! 459 号：NotificationCenter 交互测试——未读徽标计数、打开即全部已读、
//! 清空空态、Esc 关闭。真 store 驱动（zustand 全局态直接 push）。
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NotificationCenter } from "./NotificationCenter";
import { useNotificationsStore } from "@/store/notifications";

afterEach(() => cleanup());

beforeEach(() => {
  useNotificationsStore.setState({ notifications: [] });
});

describe("NotificationCenter 交互（459 号）", () => {
  it("未读徽标计数与通知入列渲染", async () => {
    render(<NotificationCenter />);
    const store = useNotificationsStore.getState();
    // 渲染后推送须包 act——否则 zustand 订阅的 DOM 刷新不落地
    act(() => {
      store.pushNotification({ title: "第 3 章完成", level: "info" });
      store.pushNotification({ title: "引擎连接中断", level: "error", detail: "SSE 断开" });
    });

    const badge = document.querySelector('[data-testid="notification-unread"]');
    expect(badge?.textContent).toBe("2");

    const bell = document.querySelector('[data-testid="notification-bell"]') as HTMLElement;
    await userEvent.click(bell);

    expect(document.querySelector('[data-testid="notification-panel"]')).not.toBeNull();
    expect(document.body.textContent).toContain("第 3 章完成");
    expect(document.body.textContent).toContain("SSE 断开");
    // 打开即全部已读 → 徽标消失
    expect(document.querySelector('[data-testid="notification-unread"]')).toBeNull();
  });

  it("清空通知后渲染空态", async () => {
    useNotificationsStore.getState().pushNotification({ title: "唯一通知", level: "warn" });
    render(<NotificationCenter />);
    await userEvent.click(document.querySelector('[data-testid="notification-bell"]') as HTMLElement);

    await userEvent.click(screen.getByLabelText("清空通知"));
    expect(useNotificationsStore.getState().notifications).toHaveLength(0);
    expect(document.body.textContent).toContain("暂无通知");
  });

  it("Esc 关闭面板", async () => {
    render(<NotificationCenter />);
    await userEvent.click(document.querySelector('[data-testid="notification-bell"]') as HTMLElement);
    expect(document.querySelector('[data-testid="notification-panel"]')).not.toBeNull();

    await userEvent.keyboard("{Escape}");
    expect(document.querySelector('[data-testid="notification-panel"]')).toBeNull();
  });
});
