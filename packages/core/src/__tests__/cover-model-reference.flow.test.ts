import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { it, expect, vi } from "vitest";
import { saveSecrets } from "../llm/secrets.js";
import { resolveCoverGenerationRequest, generateImageFromPrompt } from "../pipeline/short-fiction-runner.js";
import { createServer } from "node:http";
import { once } from "node:events";

it("keeps provider credentials on its own origin when an image URL requires authentication", async () => {
  const remoteHeaders: Array<string | undefined> = [], localHeaders: Array<string | undefined> = [];
  const image = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=', 'base64');
  const remote = createServer((req, res) => {remoteHeaders.push(req.headers.authorization); res.writeHead(403); res.end();});
  remote.listen(0, "127.0.0.1"); await once(remote, "listening");
  const remoteUrl = `http://127.0.0.1:${(remote.address() as {port:number}).port}/image.png`;
  let generation = 0;
  const provider = createServer((req, res) => {
    if (req.method === "POST") {
      req.resume(); generation++;
      res.writeHead(200, {"Content-Type":"application/json"});
      res.end(JSON.stringify({data:[{url:generation === 1 ? remoteUrl : `http://127.0.0.1:${(provider.address() as {port:number}).port}/private.png`}]}));
    } else {
      localHeaders.push(req.headers.authorization);
      res.writeHead(req.headers.authorization ? 200 : 401, {"Content-Type":"image/png"});
      res.end(req.headers.authorization ? image : undefined);
    }
  });
  provider.listen(0, "127.0.0.1"); await once(provider, "listening");
  try {
    // 900 号 origin 策略：异 origin 环回目标默认发送前拒绝；allowlist 显式
    // 放行以保持原凭据卫生断言（fetch 必须真实发生才能断言无 Authorization）。
    vi.stubEnv("INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS", new URL(remoteUrl).origin);
    const request = {api:"images" as const,baseUrl:`http://127.0.0.1:${(provider.address() as {port:number}).port}/v1`,model:"image-fixture",apiKey:"fixture-key"};
    await expect(generateImageFromPrompt(request, "A lantern", "1024x1024")).rejects.toMatchObject({code:"IMAGE_DOWNLOAD_FAILED",status:403});
    expect(remoteHeaders).toEqual([undefined]);
    expect((await generateImageFromPrompt(request, "A lantern", "1024x1024")).buffer).toEqual(image);
    expect(localHeaders).toEqual([undefined,"Bearer fixture-key"]);
  } finally {
    vi.unstubAllEnvs();
    for (const server of [provider, remote]) {server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve()));}
  }
});

it("resolves configured service labels to model IDs while preserving other explicit models", async () => {
  const root = await mkdtemp(join(tmpdir(), "inkos-cover-model-"));
  vi.stubEnv("INKOS_COVER_ENDPOINT", "");
  vi.stubEnv("INKOS_COVER_BASE_URL", "");
  try {
    const config = { name: "fixture", version: "0.1.0", language: "zh", notify: [],
      llm: { provider: "openai", model: "fixture", baseUrl: "https://api.kkaiapi.com/v1", cover: { service: "kkaiapi", model: "gpt-image-2" } } };
    await writeFile(join(root, "inkos.json"), JSON.stringify(config));
    await saveSecrets(root, { services: { kkaiapi: { apiKey: "fixture-key" } } });
    const requests = [];
    for (const coverModel of [undefined, "gpt-image-2", "kkaiapi gpt-image-2", "kkaiapi/gpt-image-2", "kkaiapi · gpt-image-2", "vendor/custom-image"]) {
      requests.push(await resolveCoverGenerationRequest({ root, coverModel }));
    }
    expect(requests.map(request => request.model)).toEqual([
      "gpt-image-2", "gpt-image-2", "gpt-image-2", "gpt-image-2", "gpt-image-2", "vendor/custom-image",
    ]);
    expect(requests.every(request => request.baseUrl === "https://api.kkaiapi.com/v1" && request.apiKey === "fixture-key")).toBe(true);
    config.llm.cover.model = "vendor/custom-image";
    await writeFile(join(root, "inkos.json"), JSON.stringify(config));
    expect((await resolveCoverGenerationRequest({ root, coverModel: "kkaiapi/vendor/custom-image" })).model).toBe("vendor/custom-image");
    expect((await resolveCoverGenerationRequest({ root, coverModel: "kkaiapi/unknown-model" })).model).toBe("kkaiapi/unknown-model");
    vi.stubEnv("INKOS_COVER_BASE_URL", "https://api.kkaiapi.com/v1");
    vi.stubEnv("INKOS_COVER_API_KEY", "fixture-key");
    expect((await resolveCoverGenerationRequest({ root, coverModel: "kkaiapi/gpt-image-2" })).model).toBe("gpt-image-2");
    expect((await resolveCoverGenerationRequest({ root, coverBaseUrl: "https://custom.example/v1", coverModel: "kkaiapi/gpt-image-2" })).model).toBe("kkaiapi/gpt-image-2");
  } finally {
    vi.unstubAllEnvs();
    await rm(root, { recursive: true, force: true });
  }
});

it("ends a stalled cover generation at the fallback total window even without a caller signal (899)", async () => {
  // 停摆端点：接受连接后持有不响应（此前 caller signal 未传即裸 fetch 无限挂死）。
  const server = createServer((_req, _res) => { /* 故意不响应 */ });
  server.listen(0, "127.0.0.1"); await once(server, "listening");
  try {
    vi.stubEnv("INKOS_COVER_REQUEST_TIMEOUT_MS", "100");
    const request = { api: "images" as const, baseUrl: `http://127.0.0.1:${(server.address() as { port: number }).port}/v1`, model: "image-fixture", apiKey: "fixture-key" };
    const started = Date.now();
    // 关键回归锚：不传 caller signal——修复前这是 signal-less 裸 fetch。
    await expect(generateImageFromPrompt(request, "A lantern", "1024x1024")).rejects.toThrow();
    expect(Date.now() - started).toBeLessThan(5_000);
  } finally {
    vi.unstubAllEnvs();
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}, 20_000);

it("blocks a model-returned loopback URL before any request leaves the process (900)", async () => {
  // provider 返回异 origin 环回图片 URL：策略必须在发送前拒绝，目标服务器
  // 零请求（修复前该 URL 会被匿名 fetch 真实抓取——SSRF 面）。
  let extraProviderRequests = 0;
  const provider = createServer((req, res) => {
    if (req.method === "POST") {
      req.resume();
      res.writeHead(200, {"Content-Type":"application/json"});
      res.end(JSON.stringify({data:[{url:"http://127.0.0.1:1/private.png"}]}));
    } else {
      extraProviderRequests += 1; // 金丝雀：目标端口 1 无监听，provider 侧不应出现任何后续请求
      res.writeHead(200, {"Content-Type":"image/png"});
      res.end();
    }
  });
  provider.listen(0, "127.0.0.1"); await once(provider, "listening");
  try {
    const request = { api: "images" as const, baseUrl: `http://127.0.0.1:${(provider.address() as { port: number }).port}/v1`, model: "image-fixture", apiKey: "fixture-key" };
    await expect(generateImageFromPrompt(request, "A lantern", "1024x1024")).rejects.toMatchObject({
      code: "COVER_DOWNLOAD_ORIGIN_BLOCKED",
      reason: "private-or-reserved-address",
    });
    expect(extraProviderRequests).toBe(0);
  } finally {
    provider.closeAllConnections();
    await new Promise<void>((resolve) => provider.close(() => resolve()));
  }
});

it("rejects a redirect that lands on a private address and keeps its response out of the artifact (900)", async () => {
  // 同 origin URL 302 跳环回私网（可达的真实本地服务器）：匿名请求默认
  // follow，盲请求已发出（如实断言目标收到 1 次=残余面锚点），但终态
  // 复核拒绝——响应被丢弃不进入产物。
  const landedImage = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=', 'base64');
  let landedRequests = 0;
  const landedServer = createServer((_req, res) => {
    landedRequests += 1;
    res.writeHead(200, {"Content-Type":"image/png"});
    res.end(landedImage);
  });
  landedServer.listen(0, "127.0.0.1"); await once(landedServer, "listening");
  const landedUrl = `http://127.0.0.1:${(landedServer.address() as {port:number}).port}/secret.png`;
  const provider = createServer((req, res) => {
    if (req.method === "POST") {
      req.resume();
      res.writeHead(200, {"Content-Type":"application/json"});
      res.end(JSON.stringify({data:[{url:`http://127.0.0.1:${(provider.address() as {port:number}).port}/img.png`}]}));
    } else {
      res.writeHead(302, {Location: landedUrl});
      res.end();
    }
  });
  provider.listen(0, "127.0.0.1"); await once(provider, "listening");
  try {
    const request = { api: "images" as const, baseUrl: `http://127.0.0.1:${(provider.address() as { port: number }).port}/v1`, model: "image-fixture", apiKey: "fixture-key" };
    await expect(generateImageFromPrompt(request, "A lantern", "1024x1024")).rejects.toMatchObject({
      code: "COVER_DOWNLOAD_ORIGIN_BLOCKED",
      reason: "private-or-reserved-address",
    });
    expect(landedRequests).toBe(1);
  } finally {
    for (const server of [provider, landedServer]) {server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve()));}
  }
});
