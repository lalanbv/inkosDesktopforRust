// @vitest-environment jsdom
//! 648 号：QuickActions「写下一章」chip 的就地停止翻转——644 走查实证对话
//! 创作页的生产任务执行中 chip 保持 disabled「写下一章」，用户无可点停止
//! （BookDetail 的停止在设置页）。断言：执行中翻「停止写作」且点击走停止
//! 回调（复用聊天中止链），默认态与其它 chip 行为不变。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QuickActions } from "./QuickActions";

function renderActions(overrides: { writeNextRunning?: boolean; onStopWriteNext?: () => void; disabled?: boolean } = {}) {
  const onAction = vi.fn();
  const onStopWriteNext = overrides.onStopWriteNext ?? vi.fn();
  const utils = render(
    <QuickActions
      onAction={onAction}
      disabled={overrides.disabled ?? false}
      isZh
      writeNextRunning={overrides.writeNextRunning ?? false}
      onStopWriteNext={onStopWriteNext}
    />,
  );
  return { onAction, onStopWriteNext, ...utils };
}

describe("QuickActions write-next 就地停止（648 号）", () => {
  afterEach(() => cleanup());

  it("默认态：渲染「写下一章」，点击走 onAction（旧行为不变）", async () => {
    const user = userEvent.setup();
    const { onAction, onStopWriteNext, getByRole } = renderActions();
    const chip = getByRole("button", { name: "写下一章" });
    await user.click(chip);
    expect(onAction).toHaveBeenCalledWith("写下一章", "write_next");
    expect(onStopWriteNext).not.toHaveBeenCalled();
  });

  it("执行中：chip 翻「停止写作」destructive 形态，点击走停止回调", async () => {
    const user = userEvent.setup();
    const { onAction, onStopWriteNext, getByRole, queryByRole } = renderActions({ writeNextRunning: true });
    expect(queryByRole("button", { name: "写下一章" })).toBeNull();
    const stop = getByRole("button", { name: "停止写作" });
    expect(stop.getAttribute("data-slot")).toBe("quick-action-stop");
    expect(stop.className).toContain("bg-destructive");
    await user.click(stop);
    expect(onStopWriteNext).toHaveBeenCalledTimes(1);
    expect(onAction).not.toHaveBeenCalled();
  });

  it("执行中且全局面 disabled：停止 chip 仍可点（停止不许被 disabled 吞掉）", async () => {
    const user = userEvent.setup();
    const { onStopWriteNext, getByRole } = renderActions({ writeNextRunning: true, disabled: true });
    const stop = getByRole("button", { name: "停止写作" }) as HTMLButtonElement;
    expect(stop.disabled).toBe(false);
    await user.click(stop);
    expect(onStopWriteNext).toHaveBeenCalledTimes(1);
  });

  it("执行中只影响 write_next chip：审计 chip 照常 onAction", async () => {
    const user = userEvent.setup();
    const { onAction, getByRole } = renderActions({ writeNextRunning: true });
    await user.click(getByRole("button", { name: "审计" }));
    expect(onAction).toHaveBeenCalledWith("审计", undefined);
  });
});
