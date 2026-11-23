//! 1663 号：R26 运行遥测写入点锁定（403 号 runAndRecord 的 2.0 复座守卫）。
//!
//! 688 号上游合并退役 model-chain.ts 时带走唯一写入点，GET /run-log 端点
//! （701 号回收）此后恒空——端点在位掩盖了写入面失踪近一个合并周期。
//! 本文件直锁 2.0 两条 LLM 传输边界的写入：非 agent 路径 chatCompletion
//! （每个重试轮一条）、agent 路径 guardedPiStream/guardedPiNonStreaming
//! （observe 层每次传输尝试一条）。上游=真实回环 http 服务脚本化应答；
//! 断言只看 globalRunLog 的增量，不依赖缓冲既有内容。
import { createServer, type Server } from "node:http";
import { once } from "node:events";
import type { AddressInfo } from "node:net";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import type { Model } from "@mariozechner/pi-ai";
import { chatCompletion, createLLMClient } from "../llm/provider.js";
import { guardedPiNonStreaming, guardedPiStream } from "../agent/pi-stream.js";
import { runWithAgentTrajectory } from "../llm/agent-trajectory.js";
import { globalRunLog, recordRunLogCall, type RunLogEntry } from "../utils/run-log.js";

// 代理层直通全局 fetch——本用例上游是回环 http 服务，不进系统代理
// （本机系统代理窗口性拦截回环探测，本地压测记忆族）。
vi.mock("../utils/proxy-fetch.js", () => ({
  fetchWithProxy: (url: string, init: RequestInit) => fetch(url, init),
}));

/** v5 隐私红线：条目只含九个元数据键，绝不含 prompt/正文/错误原文。 */
const RUN_LOG_ENTRY_KEYS = [
  "agent", "attemptIndex", "durationMs", "errorKind", "model", "ok", "round", "tookOver", "ts",
];

type Scripted = { status: number; contentType: string; body: string };

const jsonOk = (content = "pong"): Scripted => ({
  status: 200,
  contentType: "application/json",
  body: JSON.stringify({
    choices: [{ finish_reason: "stop", message: { content } }],
    usage: { prompt_tokens: 3, completion_tokens: 1, total_tokens: 4 },
  }),
});
const jsonEmpty = (): Scripted => jsonOk("");
const sseOk = (): Scripted => ({
  status: 200,
  contentType: "text/event-stream",
  body: `data: ${JSON.stringify({
    id: "run-log-writer",
    choices: [{ index: 0, delta: { role: "assistant", content: "pong" }, finish_reason: "stop" }],
  })}\n\ndata: [DONE]\n\n`,
});
const httpError = (status: number): Scripted => ({
  status,
  contentType: "application/json",
  body: JSON.stringify({ error: { message: `upstream status ${status}` } }),
});

let server: Server;
let baseUrl = "";
let queue: Scripted[] = [];
let requests = 0;

const client = () => createLLMClient({
  provider: "openai",
  service: "custom",
  configSource: "studio",
  baseUrl,
  model: "test-model",
  apiKey: "fixture",
  apiFormat: "chat",
  stream: false,
  thinkingBudget: 0,
});

let model: Model<"openai-completions">;
const context = () => ({
  systemPrompt: "Reply briefly.",
  messages: [{ role: "user", content: "ping", timestamp: 1 }],
}) as never;

/** 跑一段调用并取出期间新增的遥测条目（旧 → 新）。 */
async function captured<T>(run: () => Promise<T>): Promise<{ result: T; entries: RunLogEntry[] }> {
  const before = globalRunLog.snapshot().totalAppended;
  const result = await run();
  const snapshot = globalRunLog.snapshot();
  const added = snapshot.totalAppended - before;
  return { result, entries: snapshot.entries.slice(snapshot.entries.length - added) };
}

function expectMetadataOnly(entries: RunLogEntry[]): void {
  for (const entry of entries) {
    expect(Object.keys(entry).sort()).toEqual(RUN_LOG_ENTRY_KEYS);
    expect(Number.isNaN(Date.parse(entry.ts))).toBe(false);
    expect(entry.durationMs).toBeGreaterThanOrEqual(0);
  }
}

beforeAll(async () => {
  server = createServer((req, res) => {
    req.resume();
    req.on("end", () => {
      requests += 1;
      const next = queue.shift() ?? jsonOk();
      res.writeHead(next.status, { "Content-Type": next.contentType });
      res.end(next.body);
    });
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/v1`;
  model = {
    id: "test-model",
    name: "test-model",
    api: "openai-completions",
    provider: "openai",
    baseUrl,
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 128_000,
    maxTokens: 1024,
  };
});

afterAll(async () => {
  await new Promise<void>((resolveClose) => server.close(() => resolveClose()));
});

describe("recordRunLogCall（记录器单源）", () => {
  it("成功且 round>1 记接管；失败即使 round>1 也不记接管（与 Rust 同口径）", async () => {
    const { entries } = await captured(async () => {
      recordRunLogCall({ agent: "a", model: "m", startedAt: Date.now() - 5, ok: true, round: 3 });
      recordRunLogCall({ agent: "a", model: "m", startedAt: Date.now(), ok: false, round: 3 });
      recordRunLogCall({ agent: "a", model: "m", startedAt: Date.now(), ok: false });
    });
    expectMetadataOnly(entries);
    expect(entries.map((entry) => [entry.ok, entry.round, entry.tookOver, entry.errorKind, entry.attemptIndex])).toEqual([
      [true, 3, true, null, 0],
      [false, 3, false, "fatal", 0],
      [false, 1, false, "fatal", 0],
    ]);
    expect(entries[0]!.durationMs).toBeGreaterThanOrEqual(5);
  });
});

describe("chatCompletion 写入点（非 agent 路径）", () => {
  it("成功调用记一条：缺省标签 chat、primary 轮、不接管", async () => {
    queue = [jsonOk()];
    const { result, entries } = await captured(() => chatCompletion(
      client(), model.id, [{ role: "user", content: "ping" }], { maxTokens: 8 },
    ));
    expect(result.content).toBe("pong");
    expectMetadataOnly(entries);
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      agent: "chat", model: "test-model", ok: true, attemptIndex: 0, round: 1, tookOver: false, errorKind: null,
    });
  });

  it("agentLabel 选项覆盖标签", async () => {
    queue = [jsonOk()];
    const { entries } = await captured(() => chatCompletion(
      client(), model.id, [{ role: "user", content: "ping" }], { maxTokens: 8, agentLabel: "service-probe" },
    ));
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ agent: "service-probe", ok: true });
  });

  it("轨迹作用域内缺省取角色标签", async () => {
    queue = [jsonOk()];
    const { entries } = await captured(() => runWithAgentTrajectory(
      { conversationId: "c", runId: "r", agentRole: "workflow" },
      () => chatCompletion(client(), model.id, [{ role: "user", content: "ping" }], { maxTokens: 8 }),
    ));
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ agent: "workflow", ok: true });
  });

  it("瞬态失败后重试成功：逐轮各记一条，命中轮 round=2 记接管", async () => {
    queue = [httpError(503), jsonOk()];
    const { result, entries } = await captured(() => chatCompletion(
      client(), model.id, [{ role: "user", content: "ping" }], { maxTokens: 8 },
    ));
    expect(result.content).toBe("pong");
    expectMetadataOnly(entries);
    expect(entries.map((entry) => [entry.ok, entry.errorKind, entry.round, entry.tookOver])).toEqual([
      [false, "transient", 1, false],
      [true, null, 2, true],
    ]);
  });

  it("致命失败只记一条 fatal 且不重试", async () => {
    queue = [httpError(400)];
    const before = requests;
    const { entries } = await captured(() => expect(chatCompletion(
      client(), model.id, [{ role: "user", content: "ping" }], { maxTokens: 8 },
    )).rejects.toThrow());
    expect(requests - before).toBe(1);
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ ok: false, errorKind: "fatal", round: 1, tookOver: false });
  });

  it("retry:false 关闭重试时瞬态失败仍按性质记瞬态", async () => {
    queue = [httpError(503)];
    const before = requests;
    const { entries } = await captured(() => expect(chatCompletion(
      client(), model.id, [{ role: "user", content: "ping" }], { maxTokens: 8, retry: false },
    )).rejects.toThrow());
    expect(requests - before).toBe(1);
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ ok: false, errorKind: "transient", round: 1 });
  });

  it("预先中止的轮次未上线，不记", async () => {
    const controller = new AbortController();
    controller.abort();
    const { entries } = await captured(() => expect(chatCompletion(
      client(), model.id, [{ role: "user", content: "ping" }], { maxTokens: 8, signal: controller.signal },
    )).rejects.toThrow());
    expect(entries).toHaveLength(0);
  });
});

describe("guardedPi* 写入点（agent 路径）", () => {
  it("非流式成功记一条：无轨迹作用域=agent 标签", async () => {
    queue = [jsonOk()];
    const { result, entries } = await captured(() => guardedPiNonStreaming(model, context(), { apiKey: "fixture" }).result());
    expect(result.stopReason).toBe("stop");
    expectMetadataOnly(entries);
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      agent: "agent", model: "test-model", ok: true, attemptIndex: 0, round: 1, tookOver: false, errorKind: null,
    });
  });

  it("流式成功记一条", async () => {
    queue = [sseOk()];
    const { result, entries } = await captured(() => guardedPiStream(model, context(), { apiKey: "fixture" }).result());
    expect(result.stopReason).toBe("stop");
    expectMetadataOnly(entries);
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ agent: "agent", ok: true, round: 1, tookOver: false });
  });

  it("轨迹作用域内取角色标签", async () => {
    queue = [jsonOk()];
    const { entries } = await captured(() => runWithAgentTrajectory(
      { conversationId: "c", runId: "r", agentRole: "main" },
      () => guardedPiNonStreaming(model, context(), { apiKey: "fixture" }).result(),
    ));
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ agent: "main", ok: true });
  });

  it("空响应耗尽 observe 层重试：两次尝试各一条瞬态，无重复记账", async () => {
    queue = [jsonEmpty(), jsonEmpty()];
    const before = requests;
    const { result, entries } = await captured(() => guardedPiNonStreaming(model, context(), { apiKey: "fixture" }).result());
    expect(requests - before).toBe(2);
    expect(result.stopReason).toBe("error");
    expectMetadataOnly(entries);
    expect(entries.map((entry) => [entry.ok, entry.errorKind, entry.round, entry.tookOver])).toEqual([
      [false, "transient", 1, false],
      [false, "transient", 2, false],
    ]);
  });

  it("空响应重试命中：首条瞬态、次条 round=2 记接管", async () => {
    queue = [jsonEmpty(), jsonOk("recovered")];
    const { result, entries } = await captured(() => guardedPiNonStreaming(model, context(), { apiKey: "fixture" }).result());
    expect(result.stopReason).toBe("stop");
    expect(entries.map((entry) => [entry.ok, entry.errorKind, entry.round, entry.tookOver])).toEqual([
      [false, "transient", 1, false],
      [true, null, 2, true],
    ]);
  });

  it("上游致命错误记 fatal，不重试", async () => {
    queue = [httpError(401)];
    const before = requests;
    const { result, entries } = await captured(() => guardedPiNonStreaming(model, context(), { apiKey: "fixture" }).result());
    expect(requests - before).toBe(1);
    expect(result.stopReason).toBe("error");
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ ok: false, errorKind: "fatal", round: 1 });
  });

  it("流式上游致命错误同样记 fatal", async () => {
    queue = [httpError(401)];
    const { result, entries } = await captured(() => guardedPiStream(model, context(), { apiKey: "fixture" }).result());
    expect(result.stopReason).toBe("error");
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ ok: false, errorKind: "fatal", round: 1 });
  });
});
