// @vitest-environment jsdom
//! R54/1659 号：prompt pack 管理面交互测试——列表/选择/保存覆盖（hint 评测
//! 提示）/版本历史/diff 投影/一键回滚/端点不可达降级。端点真源=Rust 引擎
//! （1658）；纯函数语义由 core text-diff golden 锁定。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { PromptPacksPanel } from "./PromptPacksPanel";

const fetchJsonMock = vi.hoisted(() => vi.fn());

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

const packsPayload = () => ({
  packs: [
    { id: "longform", title: "Longform Writing", description: "Core prompts.", prompts: ["longform.writer"] },
  ],
  prompts: [
    { id: "longform.writer", title: "Writer", source: "builtin", overridden: false, content: "# 内置正文", defaultContent: "# 内置正文" },
  ],
});

function seedHappy() {
  fetchJsonMock.mockImplementation((path?: string, init?: { method?: string; body?: string }) => {
    // 环境噪声容错：偶发零参调用（React 19 双 effect/mockReset 时序）吞掉。
    if (typeof path !== "string") return Promise.resolve({});
    if (path === "/prompt-packs" && !init) return Promise.resolve(packsPayload());
    if (path === "/prompt-packs/longform.writer" && init?.method === "PUT") {
      return Promise.resolve({ regressionHint: "prompt-changed" });
    }
    if (path === "/prompt-packs/longform.writer" && init?.method === "DELETE") {
      return Promise.resolve({});
    }
    if (path.includes("/versions") && !path.includes("/restore")) {
      return Promise.resolve({
        versions: [
          { ts: "2026-11-23T02:00:00.000Z", action: "put", preview: "# 版本二", content: "# 版本二" },
          { ts: "2026-11-23T01:00:00.000Z", action: "put", preview: "# 版本一", content: "# 版本一" },
        ],
      });
    }
    if (path.includes("/versions/restore")) return Promise.resolve({ regressionHint: "prompt-changed" });
    return Promise.reject(new Error("unexpected path: " + path));
  });
}

describe("PromptPacksPanel（R54/1659 号）", () => {
  beforeEach(() => fetchJsonMock.mockReset());
  afterEach(() => cleanup());

  it("列表渲染+选择 prompt 出编辑器与版本历史", async () => {
    seedHappy();
    render(<PromptPacksPanel />);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-packs="ready"]')).toBeTruthy();
    });
    fireEvent.click(document.querySelector('[data-prompt-item="longform.writer"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-editor="longform.writer"]')).toBeTruthy();
    });
    expect(document.querySelector('[data-prompt-versions]')).toBeTruthy();
    expect(document.querySelectorAll('[data-prompt-version-restore]').length).toBe(2);
  });

  it("保存覆盖——PUT 载荷+regressionHint 评测提示", async () => {
    seedHappy();
    render(<PromptPacksPanel />);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-item="longform.writer"]')).toBeTruthy();
    });
    fireEvent.click(document.querySelector('[data-prompt-item="longform.writer"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-editor="longform.writer"]')).toBeTruthy();
    });
    fireEvent.change(document.querySelector('[data-prompt-editor="longform.writer"]')!, {
      target: { value: "# 新覆盖内容" },
    });
    fireEvent.click(document.querySelector('[data-prompt-action="save"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-notice]')?.textContent).toContain("行为评测");
    });
    const put = fetchJsonMock.mock.calls.find(
      ([path, init]) => path === "/prompt-packs/longform.writer" && init?.method === "PUT",
    );
    expect(put).toBeTruthy();
    expect(JSON.parse(put![1]!.body!)).toEqual({ content: "# 新覆盖内容" });
  });

  it("diff——版本 vs 当前红绿投影", async () => {
    seedHappy();
    render(<PromptPacksPanel />);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-item="longform.writer"]')).toBeTruthy();
    });
    fireEvent.click(document.querySelector('[data-prompt-item="longform.writer"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-versions]')).toBeTruthy();
    });
    // 版本「# 版本一」 vs 当前「# 内置正文」：kept(# ) 行内差异 → removed+added。
    fireEvent.click(document.querySelector('[data-prompt-version-diff="2026-11-23T01:00:00.000Z"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-diff="2026-11-23T01:00:00.000Z"]')).toBeTruthy();
    });
    const kinds = Array.from(document.querySelectorAll('[data-prompt-diff] [data-diff-kind]')).map(
      (node) => node.getAttribute("data-diff-kind"),
    );
    expect(kinds).toContain("removed");
    expect(kinds).toContain("added");
  });

  it("回滚——restore 载荷 {ts}+确认弹层", async () => {
    seedHappy();
    render(<PromptPacksPanel />);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-item="longform.writer"]')).toBeTruthy();
    });
    fireEvent.click(document.querySelector('[data-prompt-item="longform.writer"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-versions]')).toBeTruthy();
    });
    fireEvent.click(document.querySelector('[data-prompt-version-restore="2026-11-23T01:00:00.000Z"]')!);
    const confirmBtn = await waitFor(() => {
      const dialog = Array.from(document.querySelectorAll("button")).find(
        (button) => button.textContent?.trim() === "回滚" && button.closest(".fixed.inset-0"),
      );
      expect(dialog).toBeDefined();
      return dialog!;
    });
    fireEvent.click(confirmBtn);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-notice]')?.textContent).toContain("回滚到所选版本");
    });
    const restore = fetchJsonMock.mock.calls.find(([path, init]) => path.includes("/versions/restore") && init?.method === "POST");
    expect(restore).toBeTruthy();
    expect(JSON.parse(restore![1]!.body!)).toEqual({ ts: "2026-11-23T01:00:00.000Z" });
    // 成功提示=status 形态（与失败的 alert 区分）。
    expect(document.querySelector('[data-prompt-notice]')?.getAttribute("data-prompt-notice-kind")).toBe("ok");
    expect(document.querySelector('[data-prompt-notice]')?.getAttribute("role")).toBe("status");
  });

  // 1666 号：操作进行中锁定 prompt 选择与编辑——保存/回滚的收尾按发起时的
  // prompt 刷新版本历史与草稿，中途切走会把旧 prompt 的历史挂到新选择底下。
  it("保存进行中锁定 prompt 列表与编辑器，结束后解锁", async () => {
    let releasePut: (() => void) | undefined;
    const putGate = new Promise<void>((resolve) => { releasePut = resolve; });
    fetchJsonMock.mockImplementation((path?: string, init?: { method?: string }) => {
      if (typeof path !== "string") return Promise.resolve({});
      if (path === "/prompt-packs" && !init) return Promise.resolve(packsPayload());
      if (path === "/prompt-packs/longform.writer" && init?.method === "PUT") {
        return putGate.then(() => ({ regressionHint: "prompt-changed" }));
      }
      if (path.includes("/versions") && !path.includes("/restore")) return Promise.resolve({ versions: [] });
      return Promise.reject(new Error("unexpected path: " + path));
    });
    render(<PromptPacksPanel />);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-item="longform.writer"]')).toBeTruthy();
    });
    fireEvent.click(document.querySelector('[data-prompt-item="longform.writer"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-editor="longform.writer"]')).toBeTruthy();
    });
    expect(document.querySelector('[data-prompt-item="longform.writer"]')?.hasAttribute("disabled")).toBe(false);
    fireEvent.change(document.querySelector('[data-prompt-editor="longform.writer"]')!, {
      target: { value: "# 新覆盖内容" },
    });
    fireEvent.click(document.querySelector('[data-prompt-action="save"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-editor="longform.writer"]')?.hasAttribute("disabled")).toBe(true);
    });
    expect(document.querySelector('[data-prompt-item="longform.writer"]')?.hasAttribute("disabled")).toBe(true);
    releasePut!();
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-notice]')?.textContent).toContain("行为评测");
    });
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-editor="longform.writer"]')?.hasAttribute("disabled")).toBe(false);
    });
    expect(document.querySelector('[data-prompt-item="longform.writer"]')?.hasAttribute("disabled")).toBe(false);
  });

  // 1666 号：失败提示此前与成功同形（同样的灰色 status）——改为 alert 形态并
  // 透出服务端消息（fetchJson 取 error.message 信封）。
  it("保存失败——提示为 alert 形态并带服务端消息", async () => {
    fetchJsonMock.mockImplementation((path?: string, init?: { method?: string }) => {
      if (typeof path !== "string") return Promise.resolve({});
      if (path === "/prompt-packs" && !init) return Promise.resolve(packsPayload());
      if (path === "/prompt-packs/longform.writer" && init?.method === "PUT") {
        return Promise.reject(new Error("content must be a string"));
      }
      if (path.includes("/versions") && !path.includes("/restore")) return Promise.resolve({ versions: [] });
      return Promise.reject(new Error("unexpected path: " + path));
    });
    render(<PromptPacksPanel />);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-item="longform.writer"]')).toBeTruthy();
    });
    fireEvent.click(document.querySelector('[data-prompt-item="longform.writer"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-action="save"]')).toBeTruthy();
    });
    fireEvent.click(document.querySelector('[data-prompt-action="save"]')!);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-notice]')?.textContent).toContain("content must be a string");
    });
    expect(document.querySelector('[data-prompt-notice]')?.getAttribute("data-prompt-notice-kind")).toBe("error");
    expect(document.querySelector('[data-prompt-notice]')?.getAttribute("role")).toBe("alert");
  });

  it("端点不可达——降级空态（Node 回退腿缺该域）", async () => {
    fetchJsonMock.mockImplementation((path?: string) => {
      if (typeof path !== "string") return Promise.resolve({});
      return Promise.reject(new Error("404"));
    });
    render(<PromptPacksPanel />);
    await waitFor(() => {
      expect(document.querySelector('[data-prompt-packs="unavailable"]')).toBeTruthy();
    });
  });
});
