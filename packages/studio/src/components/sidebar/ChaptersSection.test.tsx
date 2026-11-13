// @vitest-environment jsdom
//! 1008 号：ChaptersSection 响应竞态守卫。deps [bookId, bookDataVersion] 在
//! dock 挂载中 in-place 变化（Sidebar 换书不卸载 ContextDock + SSE 事件 bump
//! bookDataVersion），旧响应晚到会覆盖新章节列表；更糟：旧请求失败晚到
//! `.catch(() => setChapters([]))` 会清空已到手的好数据。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { ChaptersSection } from "./ChaptersSection";
import { useChatStore } from "../../store/chat";

interface DeferredResponse {
  promise: Promise<Response>;
  resolve: (r: Response) => void;
  reject: (e: unknown) => void;
}

function deferredResponse(): DeferredResponse {
  let resolve!: DeferredResponse["resolve"];
  let reject!: DeferredResponse["reject"];
  const promise = new Promise<Response>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function chaptersResponse(chapters: ReadonlyArray<{ number: number; title: string; wordCount: number }>): Response {
  return new Response(JSON.stringify({ chapters }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

describe("ChaptersSection 响应竞态守卫（1008）", () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  // store 跨例持久：相对 bump（同值不触发 effect 重取）
  const bumpVersion = () =>
    useChatStore.setState({ bookDataVersion: useChatStore.getState().bookDataVersion + 1 });

  it("bookDataVersion 连续 bump：旧版本响应晚到不得覆盖新版本章节列表", async () => {
    const stale = deferredResponse();
    const fresh = deferredResponse();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise),
    );

    const { container } = render(<ChaptersSection bookId="b1" isZh />);
    // SSE bump → effect 重发 #2（fresh）
    await act(async () => {
      bumpVersion();
    });
    await act(async () => {
      fresh.resolve(chaptersResponse([{ number: 2, title: "新章", wordCount: 3000 }]));
    });
    await waitFor(() => expect(container.textContent).toContain("新章"));

    // #1（旧版本数据）晚到——不得覆盖
    await act(async () => {
      stale.resolve(chaptersResponse([{ number: 1, title: "旧章", wordCount: 100 }]));
    });
    expect(container.textContent).toContain("新章");
    expect(container.textContent).not.toContain("旧章");
  });

  it("过期失败不清空好数据：旧版本请求失败晚于新版本成功 → 章节列表保持", async () => {
    const stale = deferredResponse();
    const fresh = deferredResponse();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise),
    );

    const { container } = render(<ChaptersSection bookId="b1" isZh />);
    await act(async () => {
      bumpVersion();
    });
    await act(async () => {
      fresh.resolve(chaptersResponse([{ number: 2, title: "新章", wordCount: 3000 }]));
    });
    await waitFor(() => expect(container.textContent).toContain("新章"));

    // 旧版本请求失败晚到——catch 不得清空新数据为空态
    await act(async () => {
      stale.reject(new Error("late boom"));
    });
    expect(container.textContent).toContain("新章");
    expect(container.textContent).not.toContain("暂无章节");
  });
});
