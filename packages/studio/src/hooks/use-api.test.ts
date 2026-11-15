// @vitest-environment jsdom
//! 1008 号：useApi 响应竞态守卫——中心杠杆轮。useApi.refetch 是全部消费方
//! （Dashboard / DaemonControl / BookDetail SSE refetch 环 / API 失效事件 /
//! 消费者手动调用）的共同漏斗，此处加 latest-wins 守卫一次护全族。
//! 竞态形态：旧请求（慢）在途时新请求（快）先返回——旧响应晚到不得覆盖
//! 新终态（data / error / loading 三面）；含 path 变更触发形态。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { isBackendUnreachableMessage, useApi } from "./use-api";

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

// 1426 号：后端不可达分类器。正例=四大引擎网络层失败的固定文案（引擎常量，
// 仓内无生产函数可派生——按 1007 号「不可派生才手打并注明理由」契约纪律手打）；
// 反例含 18801 实采的服务端真实形态（PROJECT_CONFIG_INVALID 500 的人类可读串）
// 与 readErrorMessage 回退形态——服务端可达的错误恰恰不该归入「不可达」，
// 否则启动门会把两类指引（重启引擎 vs 检查 inkos.json）发反。
describe("isBackendUnreachableMessage（1426）", () => {
  it("四大引擎网络层失败文案均判为不可达", () => {
    // Blink（Edge/Chrome，1426 号用户截图原文）
    expect(isBackendUnreachableMessage("Failed to fetch")).toBe(true);
    // WebKit（Safari / 桌面壳 WKWebView 同故障的真实措辞）
    expect(isBackendUnreachableMessage("Load failed")).toBe(true);
    // undici（Node 侧 fetch）
    expect(isBackendUnreachableMessage("fetch failed")).toBe(true);
    // Firefox 完整形态
    expect(isBackendUnreachableMessage("NetworkError when attempting to fetch resource.")).toBe(true);
  });

  it("服务端可达的错误不判为不可达（分流边界）", () => {
    // 18801 实采：PROJECT_CONFIG_INVALID 500 的 message（生产派生样例）
    expect(
      isBackendUnreachableMessage(
        "Failed to load inkos.json: inkos.json not found in /tmp/inkos1087.\nMake sure you are inside an InkOS project directory (cd into the project created by 'inkos init').",
      ),
    ).toBe(false);
    // readErrorMessage 回退形态："<status> <statusText>"
    expect(isBackendUnreachableMessage("500 Internal Server Error")).toBe(false);
    expect(isBackendUnreachableMessage("404 Not Found")).toBe(false);
  });

  it("空值安全", () => {
    expect(isBackendUnreachableMessage(null)).toBe(false);
    expect(isBackendUnreachableMessage(undefined)).toBe(false);
    expect(isBackendUnreachableMessage("")).toBe(false);
  });
});

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
