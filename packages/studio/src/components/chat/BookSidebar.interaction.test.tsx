// @vitest-environment jsdom
//! 998 号：ArtifactView 编辑草稿卸载恢复——996 同族第二面（数据丢失级）。
//! ArtifactView 挂在 ContextDock 的 visible 条件渲染下：dock 开合/面板切换/
//! 页面切换都卸载重建组件，编辑中未保存输入随本地 useState 重置而丢失。
//! 修复 = sessionStorage 草稿（lib/artifact-edit-draft.ts）。
//! 断言：①进入编辑+输入后卸载重挂载，编辑态与草稿内容恢复；
//! ②保存成功清草稿（会话内回到非编辑态）；③取消清草稿。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ArtifactView } from "./BookSidebar";
import { useChatStore } from "../../store/chat";
import { artifactEditDraftKey } from "../../lib/artifact-edit-draft";

const TRUTH_CONTENT = "原文内容ABC";
const FILE = "truth/characters.md";

function mockTruthFetch() {
  return vi.fn(async () =>
    new Response(JSON.stringify({ content: TRUTH_CONTENT }), {
      status: 200,
      headers: { "content-type": "application/json" },
    }),
  );
}

function iconButton(container: HTMLElement, accessibleName: string): HTMLElement {
  // 1002 号：按可访问名称定位（aria-label 注册后升级）——本测试即 a11y 回归
  // 守卫：按钮若退化为无 aria-label，getByRole 查询失败即红。
  const btn = document.querySelector(`button[aria-label="${accessibleName}"]`);
  if (!btn) throw new Error(`button with aria-label "${accessibleName}" not found`);
  return btn as HTMLElement;
}

describe("ArtifactView 编辑草稿卸载恢复（998）", () => {
  beforeEach(() => {
    sessionStorage.clear();
    useChatStore.getState().openArtifact(FILE);
    vi.stubGlobal("fetch", mockTruthFetch());
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    sessionStorage.clear();
  });

  it("编辑→输入→卸载→重挂载：编辑态与未保存内容恢复", async () => {
    const user = userEvent.setup();
    const first = render(<ArtifactView bookId="b1" />);
    await waitFor(() => expect(first.container.textContent).toContain(TRUTH_CONTENT));

    // 进入编辑态（铅笔钮——按可访问名称定位，a11y 回归守卫）
    await user.click(iconButton(first.container, "编辑文件内容"));
    const textarea = (await waitFor(() => {
      const el = first.container.querySelector("textarea");
      if (!el) throw new Error("textarea not yet rendered");
      return el;
    })) as HTMLTextAreaElement;
    await user.type(textarea, "草稿新增");
    expect(textarea.value).toContain("草稿新增");

    // 模拟 dock 开合：卸载（未保存）
    first.unmount();

    // 重挂载：草稿恢复——textarea 直接出现且含未保存内容
    const second = render(<ArtifactView bookId="b1" />);
    const restored = (await waitFor(() => {
      const el = second.container.querySelector("textarea");
      if (!el) throw new Error("textarea not yet restored");
      return el;
    })) as HTMLTextAreaElement;
    expect(restored.value).toContain("草稿新增");
    expect(restored.value).toContain(TRUTH_CONTENT);
    expect(second.container.textContent).toContain("草稿新增");
  });

  it("保存成功：PUT 提交草稿内容并清 sessionStorage 草稿", async () => {
    const user = userEvent.setup();
    const first = render(<ArtifactView bookId="b1" />);
    await waitFor(() => expect(first.container.textContent).toContain(TRUTH_CONTENT));
    await user.click(iconButton(first.container, "编辑文件内容"));
    const textarea = (await waitFor(() => {
      const el = first.container.querySelector("textarea");
      if (!el) throw new Error("textarea not yet rendered");
      return el;
    })) as HTMLTextAreaElement;
    await user.type(textarea, "已保存新增");

    await user.click(iconButton(first.container, "保存修改"));
    // PUT 已发生（第二次 fetch 调用），body 含草稿全文
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.length).toBeGreaterThanOrEqual(2));
    const putCall = vi.mocked(fetch).mock.calls.find(([, init]) => (init as RequestInit)?.method === "PUT");
    expect(putCall).toBeDefined();
    expect((putCall![1] as RequestInit).body).toContain("已保存新增");

    // 草稿已清：重挂载回到非编辑态。mock GET 恒返回原文（真实服务会返回已保存
    // 新内容）——草稿清除的证据 = sessionStorage null + 无 textarea（非编辑态）。
    first.unmount();
    const second = render(<ArtifactView bookId="b1" />);
    await waitFor(() => expect(second.container.textContent).toContain(TRUTH_CONTENT));
    expect(second.container.querySelector("textarea")).toBeNull();
    expect(sessionStorage.getItem(artifactEditDraftKey("b1", false, null, FILE))).toBeNull();
  });

  it("取消编辑：清草稿，重挂载回到非编辑态原文", async () => {
    const user = userEvent.setup();
    const first = render(<ArtifactView bookId="b1" />);
    await waitFor(() => expect(first.container.textContent).toContain(TRUTH_CONTENT));
    await user.click(iconButton(first.container, "编辑文件内容"));
    const textarea = (await waitFor(() => {
      const el = first.container.querySelector("textarea");
      if (!el) throw new Error("textarea not yet rendered");
      return el;
    })) as HTMLTextAreaElement;
    await user.type(textarea, "将被丢弃");
    await user.click(iconButton(first.container, "取消编辑"));
    expect(sessionStorage.getItem(artifactEditDraftKey("b1", false, null, FILE))).toBeNull();

    first.unmount();
    const second = render(<ArtifactView bookId="b1" />);
    await waitFor(() => expect(second.container.textContent).toContain(TRUTH_CONTENT));
    expect(second.container.querySelector("textarea")).toBeNull();
    expect(second.container.textContent).not.toContain("将被丢弃");
  });
});

// ---------------------------------------------------------------------------
// 1008 号：ArtifactView 响应竞态守卫。目标切换（artifactFile / 章节号）在
// dock 挂载中 in-place 发生，旧目标响应晚到会覆盖新目标内容；旧目标失败晚到
// 会把新目标的正文清成空态。ArtifactView 是 1007 号备案「stale-response 防护」
// 主缺陷面。
// ---------------------------------------------------------------------------
describe("ArtifactView 响应竞态守卫（1008）", () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    sessionStorage.clear();
  });

  function deferredJson(): {
    promise: Promise<Response>;
    resolve: (r: Response) => void;
    reject: (e: unknown) => void;
  } {
    let resolve!: (r: Response) => void;
    let reject!: (e: unknown) => void;
    const promise = new Promise<Response>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    return { promise, resolve, reject };
  }

  function jsonResponse(body: unknown): Response {
    return new Response(JSON.stringify(body), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }

  function routeFetch(routes: ReadonlyArray<{ match: string; promise: Promise<Response> }>) {
    return vi.fn((input: string | URL | Request) => {
      const url = typeof input === "string" ? input : input instanceof Request ? input.url : input.href;
      const hit = routes.find((r) => url.includes(r.match));
      if (!hit) {
        return Promise.resolve(new Response(JSON.stringify({}), { status: 404 }));
      }
      return hit.promise;
    });
  }

  it("切换目标竞态：旧文件慢响应晚到不得覆盖新文件内容", async () => {
    const fileA = deferredJson();
    const fileB = deferredJson();
    vi.stubGlobal(
      "fetch",
      routeFetch([
        { match: "/truth/a.md", promise: fileA.promise },
        { match: "/truth/b.md", promise: fileB.promise },
      ]),
    );

    useChatStore.getState().openArtifact("a.md");
    const view = render(<ArtifactView bookId="b1" />);
    // a.md 请求在途（慢）——此时切换目标到 b.md
    await act(async () => {
      useChatStore.getState().openArtifact("b.md");
      fileB.resolve(jsonResponse({ content: "B目标内容" }));
    });
    await waitFor(() => expect(view.container.textContent).toContain("B目标内容"));

    // a.md 旧响应晚到——不得覆盖
    await act(async () => {
      fileA.resolve(jsonResponse({ content: "A旧内容" }));
    });
    expect(view.container.textContent).toContain("B目标内容");
    expect(view.container.textContent).not.toContain("A旧内容");
  });

  it("过期失败不清空新目标正文：旧文件请求失败晚于新文件成功 → 内容保持", async () => {
    const fileA = deferredJson();
    const fileB = deferredJson();
    vi.stubGlobal(
      "fetch",
      routeFetch([
        { match: "/truth/a.md", promise: fileA.promise },
        { match: "/truth/b.md", promise: fileB.promise },
      ]),
    );

    useChatStore.getState().openArtifact("a.md");
    const view = render(<ArtifactView bookId="b1" />);
    await act(async () => {
      useChatStore.getState().openArtifact("b.md");
      fileB.resolve(jsonResponse({ content: "B目标内容" }));
    });
    await waitFor(() => expect(view.container.textContent).toContain("B目标内容"));

    // 旧目标失败晚到——catch 不得把正文清成 null/空态
    await act(async () => {
      fileA.reject(new Error("late boom"));
    });
    expect(view.container.textContent).toContain("B目标内容");
  });
});
