import { describe, it, expect } from "vitest";
import { fetchJson, ApiResponseError } from "../use-api";

/**
 * 1079 号（1057 链路消费点锚定补全）：use-api 错误响应的 zh 承接 hooks 面活体。
 * 1057 改造后映射函数有 error-copy.test.ts 9 例锚定，但 use-api 消费点
 * （readErrorMessage→localizeKnownRuntimeMessage）无直接锚——消费点回归
 * （误删调用改直通）会静默漏网。本测试经 fetchJson 公开导出+deps.fetchImpl
 * 注入（use-api 测试友好设计）锚定三形态。样例字面与映射表条目同源
 * （server.ts 409 生产字面 1004 号/provider not-set 块 1006 号——均生产派生锚定在案）。
 */
function jsonError(body: unknown, status = 500): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

describe("use-api error zh bridge (1079 hooks consumer anchoring)", () => {
  it("error:string 形态（409 生产字面）→ ApiResponseError.message=zh 映射", async () => {
    const fetchImpl = (async () => jsonError({
      error: "This session is still running. Wait for it to finish or stop it first.",
    })) as typeof fetch;
    const err = await fetchJson("/books", {}, { fetchImpl }).then(
      () => { throw new Error("expected rejection"); },
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(ApiResponseError);
    expect((err as ApiResponseError).message).toContain("当前会话仍在运行，请等待完成或先停止。");
  });

  it("error:{message} 对象形态（provider not-set 块）→ 同款映射", async () => {
    const fetchImpl = (async () => jsonError({
      error: { message: "INKOS_LLM_API_KEY not set. Run 'inkos config set-global' or add it to project .env file." },
    })) as typeof fetch;
    const err = await fetchJson("/books", {}, { fetchImpl }).then(
      () => { throw new Error("expected rejection"); },
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(ApiResponseError);
    expect((err as ApiResponseError).message).toContain("INKOS_LLM_API_KEY 未设置。");
  });

  it("非 json 错误 → status/statusText 直通（映射不命中原样，不误伤）", async () => {
    const fetchImpl = (async () => new Response("boom", { status: 502, statusText: "Bad Gateway" })) as typeof fetch;
    const err = await fetchJson("/books", {}, { fetchImpl }).then(
      () => { throw new Error("expected rejection"); },
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(ApiResponseError);
    expect((err as ApiResponseError).message).toBe("502 Bad Gateway");
  });
});
