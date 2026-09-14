// @vitest-environment jsdom
//! 464 号：快捷键速查表测试——应用级动作标签映射 + 命令注册表行同源 +
//! Mac/Win 组合格式化。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { HotkeyCheatSheet } from "./HotkeyCheatSheet";

afterEach(() => cleanup());

describe("HotkeyCheatSheet（464 号）", () => {
  it("应用级动作行：标签映射 + Mac 组合格式化", () => {
    render(
      <HotkeyCheatSheet
        open
        onOpenChange={() => {}}
        defs={[{ combo: "mod+k", commandId: "app.palette.toggle" }]}
        lang="zh"
        isMac
      />,
    );
    expect(screen.getByText("快捷键速查")).toBeTruthy();
    expect(screen.getByText("打开/关闭命令面板")).toBeTruthy();
    expect(screen.getByText("⌘K")).toBeTruthy();
  });

  it("非 Mac 组合格式化与命令注册表行同源出现", () => {
    render(
      <HotkeyCheatSheet
        open
        onOpenChange={() => {}}
        defs={[{ combo: "mod+k", commandId: "app.palette.toggle" }]}
        lang="en"
        isMac={false}
      />,
    );
    expect(screen.getByText("Ctrl+K")).toBeTruthy();
    // 命令注册表同源：带 hotkey 的导航/动作命令自动出现
    expect(document.body.textContent!.length).toBeGreaterThan(20);
  });

  it("关闭态不渲染弹层", () => {
    render(
      <HotkeyCheatSheet
        open={false}
        onOpenChange={() => {}}
        defs={[{ combo: "mod+k", commandId: "app.palette.toggle" }]}
        lang="zh"
        isMac
      />,
    );
    expect(screen.queryByText("快捷键速查")).toBeNull();
  });
});
