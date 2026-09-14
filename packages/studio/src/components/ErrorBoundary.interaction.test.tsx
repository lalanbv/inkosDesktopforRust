// @vitest-environment jsdom
//! 463 号：ErrorBoundary 交互测试——渲染错误降级为受限错误卡 + 重试清零恢复。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ErrorBoundary } from "./ErrorBoundary";

/** 可切换的崩溃子组件：boom=true 时渲染抛错。 */
function Boom({ boom }: { boom: boolean }) {
  if (boom) throw new Error("模拟渲染崩溃");
  return <div data-testid="fine">正常内容</div>;
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("ErrorBoundary（463 号）", () => {
  it("子组件渲染错误降级为受限错误卡（不白屏）", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary>
        <Boom boom />
      </ErrorBoundary>,
    );
    await vi.waitFor(() => {
      expect(document.querySelector('[data-testid="error-boundary"]')).not.toBeNull();
    });
    expect(document.body.textContent).toContain("界面遇到了问题");
    expect(document.body.textContent).toContain("模拟渲染崩溃");
    expect(errorSpy).toHaveBeenCalled();
  });

  it("重试清零错误态——子组件恢复后重新渲染正常内容", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    let boom = true;
    // 用可变标志子组件：重试后 boom 已被外部翻转为 false → 恢复正常内容
    const child = <Boom boom={boom} />;
    function recoverableChild() {
      return boom ? child : <div data-testid="fine">正常内容</div>;
    }
    function Host() {
      return recoverableChild();
    }
    render(
      <ErrorBoundary>
        <Host />
      </ErrorBoundary>,
    );
    await vi.waitFor(() => {
      expect(document.querySelector('[data-testid="error-boundary"]')).not.toBeNull();
    });
    boom = false;
    await userEvent.click(screen.getByTestId("error-boundary-retry"));
    expect(screen.getByTestId("fine")).toBeTruthy();
    expect(errorSpy).toHaveBeenCalled();
  });
});
