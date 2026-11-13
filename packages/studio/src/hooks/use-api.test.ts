// @vitest-environment jsdom
//! 1008 号：useApi 响应竞态守卫——中心杠杆轮。useApi.refetch 是全部消费方
//! （Dashboard / DaemonControl / BookDetail SSE refetch 环 / API 失效事件 /
//! 消费者手动调用）的共同漏斗，此处加 latest-wins 守卫一次护全族。
//! 竞态形态：旧请求（慢）在途时新请求（快）先返回——旧响应晚到不得覆盖
//! 新终态（data / error / loading 三面）；含 path 变更触发形态。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useApi } from "./use-api";

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

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

describe("useApi 响应竞态守卫（1008）", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("手动 refetch 竞态：#2 先返回新数据，#1 晚到旧响应不得覆盖", async () => {
    const stale = deferredResponse();
    const fresh = deferredResponse();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise),
    );

    const { result } = renderHook(() => useApi<{ v: string }>("/race"));
    // 挂载 effect 已发起 #1（stale，在途）
    await act(async () => {
      const p = result.current.refetch();
      fresh.resolve(jsonResponse({ v: "new" }));
      await p;
    });
    await waitFor(() => expect(result.current.data).toEqual({ v: "new" }));

    // #1 晚到返回旧数据——latest-wins 守卫下不得覆盖
    await act(async () => {
      stale.resolve(jsonResponse({ v: "stale" }));
    });
    expect(result.current.data).toEqual({ v: "new" });
    expect(result.current.loading).toBe(false);
  });

  it("过期失败不清空成功态：#1 拒绝晚于 #2 成功 → error 保持 null、data 保持新值", async () => {
    const stale = deferredResponse();
    const fresh = deferredResponse();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise),
    );

    const { result } = renderHook(() => useApi<{ v: string }>("/race-fail"));
    await act(async () => {
      const p = result.current.refetch();
      fresh.resolve(jsonResponse({ v: "new" }));
      await p;
    });
    await waitFor(() => expect(result.current.data).toEqual({ v: "new" }));

    await act(async () => {
      stale.reject(new Error("late boom"));
    });
    expect(result.current.error).toBeNull();
    expect(result.current.data).toEqual({ v: "new" });
  });

  it("path 变更竞态：旧 path 慢响应晚到不覆盖新 path 数据", async () => {
    const slow = deferredResponse();
    const fast = deferredResponse();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockReturnValueOnce(slow.promise).mockReturnValueOnce(fast.promise),
    );

    const { result, rerender } = renderHook(
      ({ p }: { readonly p: string }) => useApi<{ v: string }>(p),
      { initialProps: { p: "/slow" } },
    );
    // path 变更 → refetch 身份变化 → effect 重发 #2（fast）
    rerender({ p: "/fast" });
    await act(async () => {
      fast.resolve(jsonResponse({ v: "fast" }));
    });
    await waitFor(() => expect(result.current.data).toEqual({ v: "fast" }));

    await act(async () => {
      slow.resolve(jsonResponse({ v: "slow" }));
    });
    expect(result.current.data).toEqual({ v: "fast" });
    expect(result.current.loading).toBe(false);
  });
});
