import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

// 445 号 TS 对称：API 响应统一 Cache-Control: no-store——与 Rust 腿
// api_no_store 中间件（engine-rs server/mod.rs）行为级对称。锁三形态：
// ① happy-path 响应带头；② 未注册 API 路径 404 同样带头（双腿一致：
// Rust axum Router::layer 同盖未匹配 fallback 的 404——1455 活体实证；
// 双腿差异在非 API 404 面：Rust 盖得更宽（router fallback 全局带，
// 方向=更严），TS use 只挂 /api/v1/*——404 无缓存价值，无害差）；
// ③ 非 API 路径不带（中间件只挂 /api/v1/*，assets/SPA 面不盖——SPA
// 入口 no-store 是 469 号另一形态，活体面验证）。
describe("API no-store unified layer (445 TS parity)", () => {
  let root: string;

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "inkos-api-no-store-"));
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "no-store-project",
      version: "2.0.0",
      language: "zh",
      llm: { model: "test-model", provider: "openai" },
      notify: [],
    }));
  });

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("carries Cache-Control: no-store on a read endpoint response", async () => {
    const app = createStudioServer({} as never, root);
    const response = await app.request("/api/v1/profiles");
    expect(response.status).toBe(200);
    expect(response.headers.get("cache-control")).toBe("no-store");
  });

  it("covers unregistered /api/v1 paths (404) — the layer is path-wide, not per-route", async () => {
    const app = createStudioServer({} as never, root);
    const response = await app.request("/api/v1/does-not-exist");
    expect(response.status).toBe(404);
    expect(response.headers.get("cache-control")).toBe("no-store");
  });

  // 1456 锁测：streamSSE helper 自设 `Cache-Control: no-cache`，Hono `set res`
  // 用旧响应头逐键回写新响应——重建形若不先摘键，旧值 no-cache 恰把
  // no-store 盖回（1455 四案例矩阵漏此形态，活体暴露）。本测在 dispatch 层
  // 锁定摘键序；node-server Response2 叠加面由活体探针矩阵覆盖（1456）。
  it("overrides streamSSE's self-set no-cache on the events endpoint", async () => {
    const app = createStudioServer({} as never, root);
    const response = await app.request("/api/v1/events");
    expect(response.headers.get("content-type")).toContain("text/event-stream");
    expect(response.headers.get("cache-control")).toBe("no-store");
  });

  it("leaves non-API paths without the header (assets/SPA face stays cache-eligible)", async () => {
    const app = createStudioServer({} as never, root);
    const response = await app.request("/non-api-path");
    expect(response.headers.get("cache-control")).toBeNull();
  });
});
