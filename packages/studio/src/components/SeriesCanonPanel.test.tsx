// @vitest-environment jsdom
//! 1092 号：SeriesCanonPanel canon 响应竞态守卫回归锁（1008 号同族补扫唯一真缺口）。
//! seriesId 是原位变化的内部 state：绑定保存→loadBinding→setSeriesId 重发
//! [seriesId] 效果→loadCanon 重取；守卫缺失时旧 series 的 canon 响应晚到会
//! 覆写新 series 条目（setEntries），其失败晚到也会 setEntries([]) 清空好数据。
//! 本文件锁定：canonSeqRef seq latest-wins（惯用形同 AssetLibraryPanel/PlayHud）。
//! 驱动形态：deferred 五响应链（挂载绑定→alpha canon[晚到]→PUT 改绑→重读绑定→
//! beta canon[新]）+ 真实 DOM 改绑点击，JSON 断言确定性文本。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { setAppLanguage } from "../lib/app-language";
import { SeriesCanonPanel } from "./SeriesCanonPanel";

setAppLanguage("zh");

interface DeferredResponse {
  promise: Promise<Response>;
  resolve: (r: Response) => void;
  reject: (e: unknown) => void;
}

function deferredResponse(): DeferredResponse {
  let resolve!: DeferredResponse["resolve"];
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

function canonBody(name: string, summary: string) {
  return {
    entries: [{ name, aliases: [], kind: "other", summary, facts: [], relationships: [] }],
  };
}

/** 挂载并推进到「seriesId=alpha 已绑定、alpha canon 在途（stale）」的公共前置。 */
async function setupStaleAlphaBinding() {
  const r1BindingAlpha = deferredResponse();
  const r2CanonAlpha = deferredResponse(); // stale：alpha 系 canon 慢响应
  const r3PutRebind = deferredResponse();
  const r4BindingBeta = deferredResponse();
  const r5CanonBeta = deferredResponse(); // fresh：beta 系 canon
  const fetchMock = vi
    .fn()
    .mockReturnValueOnce(r1BindingAlpha.promise)
    .mockReturnValueOnce(r2CanonAlpha.promise)
    .mockReturnValueOnce(r3PutRebind.promise)
    .mockReturnValueOnce(r4BindingBeta.promise)
    .mockReturnValueOnce(r5CanonBeta.promise);
  vi.stubGlobal("fetch", fetchMock);

  const { container } = render(<SeriesCanonPanel bookId="b1" />);
  // #1 绑定读回 → setSeriesId("alpha") → [seriesId] 效果 → #2 alpha canon（stale）
  await act(async () => {
    r1BindingAlpha.resolve(jsonResponse({ seriesId: "alpha" }));
  });
  expect(fetchMock.mock.calls[1]?.[0]).toContain("/series/alpha/canon");

  // 真实 DOM 驱动改绑 beta：PUT #3 → 绑定重读 #4 → setSeriesId("beta") → #5 beta canon
  fireEvent.change(container.querySelector('input[aria-label^="系列 ID"]')!, {
    target: { value: "beta" },
  });
  const rebindButton = Array.from(container.querySelectorAll("button")).find(
    (b) => b.textContent === "改绑",
  );
  expect(rebindButton).toBeDefined();
  fireEvent.click(rebindButton!);
  await act(async () => {
    r3PutRebind.resolve(jsonResponse({ ok: true }));
  });
  await act(async () => {
    r4BindingBeta.resolve(jsonResponse({ seriesId: "beta" }));
  });
  expect(fetchMock.mock.calls[4]?.[0]).toContain("/series/beta/canon");

  return { container, r2CanonAlpha, r5CanonBeta, fetchMock };
}

describe("SeriesCanonPanel canon 响应竞态守卫（1092）", () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("seriesId 连切：旧 series canon 响应晚到不得覆写新 series 条目", async () => {
    const { container, r2CanonAlpha, r5CanonBeta } = await setupStaleAlphaBinding();

    await act(async () => {
      r5CanonBeta.resolve(jsonResponse(canonBody("韩立", "beta 系共享条目")));
    });
    await waitFor(() => expect(container.textContent).toContain("韩立"));

    // alpha 系 canon 快照晚到——不得覆盖 beta 系条目
    await act(async () => {
      r2CanonAlpha.resolve(jsonResponse(canonBody("萧炎", "alpha 系共享条目")));
    });
    expect(container.textContent).toContain("韩立");
    expect(container.textContent).toContain("beta 系共享条目");
    expect(container.textContent).not.toContain("萧炎");
    expect(container.textContent).not.toContain("alpha 系共享条目");
  });

  it("过期失败不清空好数据：旧 series canon 失败晚于新 series 成功 → 条目保持", async () => {
    const { container, r2CanonAlpha, r5CanonBeta } = await setupStaleAlphaBinding();

    await act(async () => {
      r5CanonBeta.resolve(jsonResponse(canonBody("韩立", "beta 系共享条目")));
    });
    await waitFor(() => expect(container.textContent).toContain("韩立"));

    // alpha 系请求失败晚到——守卫不得 setEntries([]) 清空 beta 系好数据
    await act(async () => {
      r2CanonAlpha.reject(new Error("晚到失败 boom"));
    });
    expect(container.textContent).toContain("韩立");
    expect(container.textContent).not.toContain("该系列暂无共享条目");
    expect(container.textContent).not.toContain("晚到失败 boom");
  });
});
