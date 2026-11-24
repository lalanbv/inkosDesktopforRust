import { createServer, type Server } from "node:http";
import { once } from "node:events";
import type { AddressInfo } from "node:net";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { globalRunLog } from "@actalk/inkos-core";

/**
 * 1664 号：路由覆盖盲区首批清偿（218 条 TS 路由中 34 条零测试/探针引用差集
 * 里的两条——全清单与棘轮见 route-coverage-guard.test）。
 *
 * - POST /services/:service/test：连通性探测路由的两条路径。实测契约（首版
 *   测试按「成功探测必经一次 chatCompletion」写，被实跑推翻）：/models 可达
 *   即返回，**零 LLM 调用**；模型列表不可达才逐模型 chat 探测——该路径经
 *   chatCompletion（1663 号写入点标签 service-probe），是「写入点 →
 *   GET /run-log」在 studio 层的端到端证据（R26 死面复发即红）。
 * - POST .../workspace/inspiration：书/章不存在的状态码（Rust 同径 404；
 *   此前 readdir ENOENT → 500 裸串）。
 */
describe("route coverage blind spots (1664)", () => {
  let app: ReturnType<typeof createStudioServer>;
  let root = "";
  let upstream: Server;
  let upstreamBase = "";
  let modelsReachable = true;
  let chatCalls = 0;

  beforeAll(async () => {
    root = mkdtempSync(join(tmpdir(), "route-blind-spots-"));
    app = createStudioServer({} as never, root);
    upstream = createServer((req, res) => {
      req.resume();
      req.on("end", () => {
        if (req.url?.endsWith("/models")) {
          if (!modelsReachable) {
            res.writeHead(404, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ error: { message: "no models endpoint" } }));
            return;
          }
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ object: "list", data: [{ id: "probe-model", object: "model" }] }));
          return;
        }
        if (req.url?.endsWith("/chat/completions") && req.method === "POST") {
          chatCalls += 1;
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ choices: [{ index: 0, message: { role: "assistant", content: "OK" }, finish_reason: "stop" }] }));
          return;
        }
        res.writeHead(404, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: { message: "unexpected probe path" } }));
      });
    });
    upstream.listen(0, "127.0.0.1");
    await once(upstream, "listening");
    upstreamBase = `http://127.0.0.1:${(upstream.address() as AddressInfo).port}/v1`;
  });

  afterAll(async () => {
    await new Promise<void>((resolveClose) => upstream.close(() => resolveClose()));
    // 1669 号：此前项目根不清理——每跑一次在系统 tmp 留一个 route-blind-spots-* 根。
    if (root) rmSync(root, { recursive: true, force: true });
  });

  async function testService(body: Record<string, unknown>): Promise<{ status: number; body: Record<string, any> }> {
    const res = await app.request("/api/v1/services/custom/test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      // 1015/855 号惯用形：客户端凭据走 env 注入 + 非凭据回退。
      body: JSON.stringify({ apiKey: process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder", ...body }),
    });
    return { status: res.status, body: (await res.json()) as Record<string, any> };
  }

  it("reachable /models answers from the model list with zero LLM calls", async () => {
    modelsReachable = true;
    const callsBefore = chatCalls;
    const appendedBefore = globalRunLog.snapshot().totalAppended;
    const { status, body } = await testService({ baseUrl: upstreamBase });
    expect(status).toBe(200);
    expect(body).toMatchObject({ ok: true, selectedModel: "probe-model", detected: { modelsSource: "api" } });
    expect(body.models.map((model: { id: string }) => model.id)).toContain("probe-model");
    // 模型列表可达 = 不花一次调用（成本契约）。
    expect(chatCalls).toBe(callsBefore);
    expect(globalRunLog.snapshot().totalAppended).toBe(appendedBefore);
  });

  it("unreachable /models falls back to the chat probe, which lands in the run log", async () => {
    modelsReachable = false;
    const { status, body } = await testService({ baseUrl: upstreamBase, preferredModel: "probe-model" });
    expect(status).toBe(200);
    expect(body).toMatchObject({ ok: true, selectedModel: "probe-model" });

    const runLog = await app.request("/api/v1/run-log?limit=200");
    expect(runLog.status).toBe(200);
    const entries = ((await runLog.json()) as { entries: Array<Record<string, unknown>> }).entries;
    const probe = entries.find((entry) => entry.agent === "service-probe" && entry.model === "probe-model");
    expect(probe, "chat 探测成功后 run-log 必有 service-probe 条目（R26 写入点）").toBeDefined();
    expect(probe).toMatchObject({ ok: true, errorKind: null, round: 1, tookOver: false });
    // v5 隐私红线：只含九个元数据键。
    expect(Object.keys(probe!).sort()).toEqual([
      "agent", "attemptIndex", "durationMs", "errorKind", "model", "ok", "round", "tookOver", "ts",
    ]);
  });

  it("service connectivity test rejects an unknown service", async () => {
    const res = await app.request("/api/v1/services/no-such-service/test", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ apiKey: process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder" }),
    });
    expect(res.status).toBe(400);
    expect(((await res.json()) as { ok: boolean }).ok).toBe(false);
  });

  it("workspace inspiration on a missing book answers 404, not a 500 ENOENT", async () => {
    const res = await app.request("/api/v1/books/ghost-book/chapters/1/workspace/inspiration", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(res.status).toBe(404);
    expect(((await res.json()) as { error: string }).error).toBe("Chapter not found");
  });
});
