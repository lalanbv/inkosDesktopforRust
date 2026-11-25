import { createServer, type IncomingMessage } from "node:http";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { evictAgentCache } from "@actalk/inkos-core";
import { createStudioServer } from "../api/server.js";

/**
 * 交互聊天发给上游的 temperature。两腿共用对照表
 * `golden/chat-temperature-parity.json`（产品引擎 `tests/chat_temperature.rs`
 * 读同一文件）：所选服务项配置了就发、没配置就不发，模型卡标了固定温度的按卡钳制。
 *
 * 取证（修复前）：Node 腿交互聊天从不带 temperature——界面「随机性（可选）」对
 * 聊天不生效（只有管线工作调用读它）；产品引擎恒发 0.7。
 */
const here = dirname(fileURLToPath(import.meta.url));
interface Case {
  readonly name: string;
  readonly services: Array<Record<string, unknown>>;
  readonly defaultModel: string;
  readonly topLevelTemperature?: number;
  readonly request: Record<string, string>;
  /** 本轮每个上游请求的 temperature；null = 不带该键。 */
  readonly expect: number | null;
}
const table = JSON.parse(
  readFileSync(join(here, "golden/chat-temperature-parity.json"), "utf-8"),
) as { cases: Case[] };

/** 测试凭据一律 env 注入 + 非凭据回退。 */
const testKey = () => process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";

async function readBody(request: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(Buffer.from(chunk));
  return JSON.parse(Buffer.concat(chunks).toString("utf-8")) as Record<string, unknown>;
}

describe("interactive chat temperature (shared parity table)", () => {
  const roots: string[] = [];
  const sessions: string[] = [];
  afterEach(async () => {
    for (const session of sessions.splice(0)) evictAgentCache(session);
    for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true });
  });

  async function upstream() {
    const captured: Array<Record<string, unknown>> = [];
    const server = createServer(async (request, response) => {
      captured.push(await readBody(request));
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ choices: [{ finish_reason: "tool_calls", message: { role: "assistant", tool_calls: [{
        id: "finish", type: "function", function: { name: "finish_turn", arguments: JSON.stringify({ status: "answered", message: "ok" }) },
      }] } }] }));
    });
    server.listen(0, "127.0.0.1"); await once(server, "listening");
    return {
      captured,
      baseUrl: `http://127.0.0.1:${(server.address() as { port: number }).port}/v1`,
      close: async () => {
        server.closeAllConnections();
        await new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())));
      },
    };
  }

  async function project(baseUrl: string, services: Array<Record<string, unknown>>, defaultModel: string, topLevelTemperature?: number) {
    const root = await mkdtemp(join(tmpdir(), "inkos-chat-temperature-"));
    roots.push(root);
    await mkdir(join(root, ".inkos"));
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "fixture", version: "0.1.0", language: "en",
      llm: { defaultModel, services, ...(topLevelTemperature !== undefined ? { temperature: topLevelTemperature } : {}) },
    }));
    await writeFile(join(root, ".inkos", "secrets.json"), JSON.stringify({
      services: { "custom:fixture": { apiKey: testKey() } },
    }));
    return root;
  }

  const post = (body: unknown) => ({ method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });

  async function chat(root: string, request: Record<string, string>): Promise<{ sessionId: string; status: number }> {
    const app = createStudioServer({} as never, root);
    const created = await (await app.request("/api/v1/sessions", post({ sessionKind: "chat" }))).json() as { session: { sessionId: string } };
    sessions.push(created.session.sessionId);
    const response = await app.request("/api/v1/agent", post({ sessionId: created.session.sessionId, instruction: "Explain a scene.", ...request }));
    return { sessionId: created.session.sessionId, status: response.status };
  }

  it("对照表：配置了就发、没配置就不发、固定温度模型按卡钳制", async () => {
    expect(table.cases).toHaveLength(7);
    for (const testCase of table.cases) {
      const server = await upstream();
      try {
        const services = testCase.services.map((entry) => ({ ...entry, baseUrl: server.baseUrl }));
        const root = await project(server.baseUrl, services, testCase.defaultModel, testCase.topLevelTemperature);
        const { status } = await chat(root, testCase.request);
        expect(status, testCase.name).toBe(200);
        expect(server.captured.length, testCase.name).toBeGreaterThan(0);
        for (const body of server.captured) {
          expect("temperature" in body ? body.temperature ?? null : null, testCase.name).toBe(testCase.expect);
          expect(body.model, testCase.name).toBe(testCase.defaultModel);
        }
      } finally {
        await server.close();
      }
    }
  }, 30000);

  it("同一会话里改了「随机性」：下一个请求用新值，缓存里的 Agent 不带旧值", async () => {
    const server = await upstream();
    try {
      const entry = (temperature?: number) => [{ service: "custom", name: "fixture", baseUrl: server.baseUrl, apiFormat: "chat", stream: false, models: ["fixture-model"], ...(temperature !== undefined ? { temperature } : {}) }];
      const root = await project(server.baseUrl, entry(0.2), "fixture-model");
      const app = createStudioServer({} as never, root);
      const created = await (await app.request("/api/v1/sessions", post({ sessionKind: "chat" }))).json() as { session: { sessionId: string } };
      sessions.push(created.session.sessionId);
      const send = async () => {
        const before = server.captured.length;
        const response = await app.request("/api/v1/agent", post({ sessionId: created.session.sessionId, instruction: "Explain a scene.", model: "fixture-model", service: "custom:fixture" }));
        expect(response.status).toBe(200);
        return server.captured.slice(before).map((body) => ("temperature" in body ? (body.temperature as number | null) : null));
      };
      expect(await send()).toEqual([0.2]);

      const rewrite = async (temperature?: number) => {
        const config = JSON.parse(await readFile(join(root, "inkos.json"), "utf-8")) as { llm: Record<string, unknown> };
        config.llm.services = entry(temperature);
        await writeFile(join(root, "inkos.json"), JSON.stringify(config));
      };
      await rewrite(0.9);
      expect(await send()).toEqual([0.9]);
      await rewrite(undefined);
      expect(await send()).toEqual([null]);
    } finally {
      await server.close();
    }
  }, 30000);

  // `saveServiceConfig`（pages/service-detail-state.ts）保存服务项时发给
  // `PUT /services/config` 的请求体，逐字段同形（数组形态；temperature 为数值，
  // 「使用默认」为 null）。captureBaseUrl 是本用例的回环采集服务地址。
  const uiSaveBody = (captureBaseUrl: string, temperature: number | null) => ({
    service: "custom:fixture",
    defaultModel: "fixture-model",
    services: [{ service: "custom", temperature, apiFormat: "chat", stream: false, models: ["fixture-model"], name: "fixture", baseUrl: captureBaseUrl }],
  });

  it("按界面的保存形态走一遍：服务详情页填随机性 → 聊天带上；点「使用默认」→ 聊天不带", async () => {
    const server = await upstream();
    try {
      const root = await project(server.baseUrl, [], "fixture-model");
      const app = createStudioServer({} as never, root);
      const saveAsUiDoes = async (temperature: number | null) => {
        const saved = await app.request("/api/v1/services/config", { ...post(uiSaveBody(server.baseUrl, temperature)), method: "PUT" });
        expect(saved.status, await saved.clone().text()).toBe(200);
      };
      const created = await (await app.request("/api/v1/sessions", post({ sessionKind: "chat" }))).json() as { session: { sessionId: string } };
      sessions.push(created.session.sessionId);
      const send = async () => {
        const before = server.captured.length;
        const response = await app.request("/api/v1/agent", post({ sessionId: created.session.sessionId, instruction: "Explain a scene.", model: "fixture-model", service: "custom:fixture" }));
        expect(response.status).toBe(200);
        return server.captured.slice(before).map((body) => ("temperature" in body ? (body.temperature as number | null) : null));
      };

      // 还没设置过：不带。
      await saveAsUiDoes(null);
      expect(await send()).toEqual([null]);
      // 界面填 0.3 并保存。
      await saveAsUiDoes(0.3);
      expect(await send()).toEqual([0.3]);
      // 点「使用默认」保存 → 不再带。
      await saveAsUiDoes(null);
      expect(await send()).toEqual([null]);
    } finally {
      await server.close();
    }
  }, 30000);
});
