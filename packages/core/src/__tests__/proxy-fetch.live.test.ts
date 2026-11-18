import { afterAll, beforeAll, describe, expect, it } from "vitest";
import * as http from "node:http";
import type { AddressInfo } from "node:net";

// 1573 号红路活体（真实 undici，不 mock ProxyAgent）：
// 1. 回环目标 + 死代理 env → 必须旁路直连成功（200 来自本 mock app）；
// 2. 非回环目标 + 标记代理 → 必须真过代理（599 标记响应）——证明本进程
//    的 undici 真实采纳了 dispatcher（红路前提活体），而非静默直连。
// 独立测试文件 = 独立模块注册表，不受 proxy-fetch.test.ts 的 undici mock 污染。

describe("fetchWithProxy live behavior (real undici)", () => {
  let app: http.Server;
  let appPort: number;
  let marker: http.Server;
  let markerUrl: string;

  beforeAll(async () => {
    // 本地回环 app：GET → 200 JSON（模拟 Ollama /v1/models 线格式）。
    app = http.createServer((_req, res) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ data: [{ id: "m1" }] }));
    });
    await new Promise<void>((resolve) => app.listen(0, "127.0.0.1", resolve));
    appPort = (app.address() as AddressInfo).port;

    // 标记代理：undici ProxyAgent 对 http 目标发绝对地址形态请求
    // （req.url = 完整 URL）；对任何经代理的请求回 599 标记——比「转发失败」
    // 更确定，无外部网络依赖。
    marker = http.createServer((req, res) => {
      res.writeHead(599, { "content-type": "text/plain" });
      res.end(`via-proxy ${req.url ?? ""}`);
    });
    await new Promise<void>((resolve) => marker.listen(0, "127.0.0.1", resolve));
    markerUrl = `http://127.0.0.1:${(marker.address() as AddressInfo).port}`;
  });

  afterAll(async () => {
    await new Promise<void>((resolve) => app.close(() => resolve()));
    await new Promise<void>((resolve) => marker.close(() => resolve()));
  });

  const deadProxyEnv = () => ({
    HTTPS_PROXY: markerUrl,
    HTTP_PROXY: markerUrl,
  });

  it("loopback target bypasses proxy and reaches the local app directly", async () => {
    const { fetchWithProxy } = await import("../utils/proxy-fetch.js");
    const res = await fetchWithProxy(
      `http://127.0.0.1:${appPort}/v1/models`,
      { signal: AbortSignal.timeout(3000) },
      undefined,
      deadProxyEnv(),
    );
    expect(res.status).toBe(200);
    const body = (await res.json()) as { data: Array<{ id: string }> };
    expect(body.data[0].id).toBe("m1");
  });

  it("localhost target bypasses proxy as well", async () => {
    const { fetchWithProxy } = await import("../utils/proxy-fetch.js");
    const res = await fetchWithProxy(
      `http://localhost:${appPort}/v1/models`,
      { signal: AbortSignal.timeout(3000) },
      undefined,
      deadProxyEnv(),
    );
    expect(res.status).toBe(200);
  });

  it("control: non-loopback target really transits the proxy (marker response proves dispatcher honored)", async () => {
    const { fetchWithProxy } = await import("../utils/proxy-fetch.js");
    // 192.168.1.10：非回环字面量（旁路判定=false）→ 走标记代理 → 599。
    // 若 undici 忽略 dispatcher，直连 192.168.1.10 将在 3s abort 红掉。
    // 端口须避开 undici bad-port 清单（9/22/25… 会被客户端侧拒发，到不了代理）。
    const res = await fetchWithProxy(
      "http://192.168.1.10:8080/v1/models",
      { signal: AbortSignal.timeout(3000) },
      undefined,
      deadProxyEnv(),
    );
    expect(res.status).toBe(599);
    expect(await res.text()).toContain("via-proxy");
  });
});
