// @vitest-environment jsdom
//! 1008 号：useApi 响应竞态守卫——中心杠杆轮。useApi.refetch 是全部消费方
//! （Dashboard / DaemonControl / BookDetail SSE refetch 环 / API 失效事件 /
//! 消费者手动调用）的共同漏斗，此处加 latest-wins 守卫一次护全族。
//! 竞态形态：旧请求（慢）在途时新请求（快）先返回——旧响应晚到不得覆盖
//! 新终态（data / error / loading 三面）；含 path 变更触发形态。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiResponseError, fetchJson, isBackendUnreachableMessage, useApi } from "./use-api";

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
// 1427 号补正例=裸网关类 5xx（1427 活体 curl 实采：vite dev 代理 ECONNREFUSED
// 返回 HTTP 500 + 空 text/plain 体 → readErrorMessage 回退裸串，生产派生）。
// 反例含 18801 实采的服务端真实形态（PROJECT_CONFIG_INVALID 500 带详情人话串）
// 与非网关类回退形态——服务端可达的错误恰恰不该归入「不可达」，
// 否则启动门会把两类指引（重启引擎 vs 检查 inkos.json）发反。
describe("isBackendUnreachableMessage（1426/1427/1429）", () => {
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

  it("裸网关类 5xx（代理签名）判为不可达（1427 活体样例）", () => {
    // vite dev 代理 ECONNREFUSED 实采（1427 浏览器活体 curl 同形）
    expect(isBackendUnreachableMessage("500 Internal Server Error")).toBe(true);
    // 同族网关态（代理产生；双端引擎自身从不返回）
    expect(isBackendUnreachableMessage("502 Bad Gateway")).toBe(true);
    expect(isBackendUnreachableMessage("503 Service Unavailable")).toBe(true);
    expect(isBackendUnreachableMessage("504 Gateway Timeout")).toBe(true);
    // 整串锚定：只匹配裸回退串，带详情/前后缀不误判
    expect(isBackendUnreachableMessage("HTTP/1.1 500 Internal Server Error")).toBe(false);
    expect(isBackendUnreachableMessage("500 Internal Server Error (details)")).toBe(false);
  });

  it("裸状态码（空 statusText）判为不可达——fetchJson 真链路生产派生（1429）", async () => {
    // 1429 号（逐功能审查发现）：readErrorMessage 回退串 `${status} ${statusText}`.trim()
    // 在 statusText 为空（HTTP/2 无 reason phrase / 裸代理不回 reason phrase）
    // 时恰为裸状态码——样例经 fetchJson→readErrorMessage→localizeKnownRuntimeMessage
    // 真链路取样（1007 契约纪律：样例生产派生，防手打与实现双漂假绿）。
    for (const status of [500, 502, 503, 504]) {
      let thrown: unknown;
      try {
        await fetchJson(`/bare-${status}`, {}, {
          fetchImpl: (async () => new Response(null, { status })) as typeof fetch,
        });
      } catch (e) {
        thrown = e;
      }
      expect(thrown, `${status} 应抛 ApiResponseError`).toBeInstanceOf(ApiResponseError);
      const message = (thrown as ApiResponseError).message;
      expect(message, `${status} 空 statusText 应回退裸状态码`).toBe(String(status));
      expect(isBackendUnreachableMessage(message), `${message} 应判不可达`).toBe(true);
    }
    // 放宽不破坏锚定边界：裸码收尾形态不误吞带前后缀/非网关串
    expect(isBackendUnreachableMessage("5001")).toBe(false);
    expect(isBackendUnreachableMessage("500 Internal Server Error (details)")).toBe(false);
    expect(isBackendUnreachableMessage("HTTP/1.1 500 Internal Server Error")).toBe(false);
    expect(isBackendUnreachableMessage("404")).toBe(false);
    expect(isBackendUnreachableMessage("403")).toBe(false);
  });

  it("服务端可达的错误不判为不可达（分流边界）", () => {
    // 18801 实采：PROJECT_CONFIG_INVALID 500 的 message（生产派生样例——
    // 引擎自身 500 恒带 JSON 详情，落进 inkos.json 指引分支）
    expect(
      isBackendUnreachableMessage(
        "Failed to load inkos.json: inkos.json not found in /tmp/inkos1087.\nMake sure you are inside an InkOS project directory (cd into the project created by 'inkos init').",
      ),
    ).toBe(false);
    // 非网关类回退形态：路由缺失面（引擎契约恒有 /project，404=非引擎面）
    expect(isBackendUnreachableMessage("404 Not Found")).toBe(false);
    expect(isBackendUnreachableMessage("403 Forbidden")).toBe(false);
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
