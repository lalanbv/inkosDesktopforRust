// @vitest-environment jsdom
//! 1003 号：移动端抽屉 a11y/i18n 守卫——873/1002 同族补位。1002 备案「SR 用户
//! 走关闭钮路径」的前提此前不成立：抽屉关闭钮为纯图标且无可访问名称，抽屉
//! 标题「书籍信息」为硬编码中文（非 t() 键）。修复后本测试即守卫，t 桩用键名
//! 哨兵形态（t(key) → "[key]"）一次拦三种退化：①按钮退化无 aria-label（查询
//! 失败红）；②渲染回退硬编码中文（哨兵缺席红）；③t 输出未接线回落键名字面量
//! （同形缺席红——哨兵即键名包裹，接线正确必在位）。
//! 分支选择：sidebarView 走 artifact（复用 interaction 测试已实证的 truth
//! mock 形态）——抽屉头/关闭钮在 panel/artifact 两分支同构，本测目标是抽屉
//! chrome 不涉面板内容；panel 分支的派生逻辑由 BookSidebar.activeOp.test.ts
//! 纯函数覆盖。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { BookSidebarToggle } from "./BookSidebar";
import { useChatStore } from "../../store/chat";
import type { TFunction } from "../../hooks/use-i18n";

// 键名哨兵 t：任何经 t() 的文案渲染为 "[键名]"——接线证明与退化拦截同形
const t = ((key: string) => `[${key}]`) as TFunction;

const FILE = "truth/characters.md";

const props = {
  bookId: "b1",
  theme: "light" as const,
  t,
  sse: { messages: [], connected: false },
};

function mockTruthFetch() {
  return vi.fn(async () =>
    new Response(JSON.stringify({ content: "原文内容" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    }),
  );
}

describe("移动端抽屉 a11y/i18n 守卫（1003）", () => {
  beforeEach(() => {
    useChatStore.getState().openArtifact(FILE);
    vi.stubGlobal("fetch", mockTruthFetch());
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    useChatStore.getState().closeArtifact();
  });

  it("抽屉开启后：关闭钮经 t() 有可访问名称，标题经 t() 接线", async () => {
    const user = userEvent.setup();
    render(<BookSidebarToggle {...props} />);
    await user.click(screen.getByRole("button", { name: "[nav.openContextPanel]" }));

    // a11y 守卫：关闭钮按 t() 可访问名称定位——退化无名/硬编码即红
    const close = screen.getByRole("button", { name: "[nav.closeContextPanel]" });
    expect(close).toBeTruthy();
    // i18n 接线守卫：标题经 t("nav.mobilePanelTitle") 渲染（哨兵在位）；
    // 硬编码「书籍信息」回退时哨兵缺席即红
    expect(document.body.textContent).toContain("[nav.mobilePanelTitle]");
    expect(document.body.textContent).not.toContain("书籍信息");
  });

  it("点关闭钮抽屉收起（SR 用户路径可达）", async () => {
    const user = userEvent.setup();
    render(<BookSidebarToggle {...props} />);
    await user.click(screen.getByRole("button", { name: "[nav.openContextPanel]" }));
    await user.click(screen.getByRole("button", { name: "[nav.closeContextPanel]" }));
    expect(screen.queryByRole("button", { name: "[nav.closeContextPanel]" })).toBeNull();
  });
});
