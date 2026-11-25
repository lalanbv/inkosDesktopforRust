import { createServer } from "node:http";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";
import type { Model } from "@mariozechner/pi-ai";
import { abortAgentSession, runAgentSession } from "../agent/agent-session.js";
import { readTranscriptEvents } from "../interaction/session-transcript.js";
// 1015 号：测试凭据惯用形——fixture 值走 env 注入 + 非凭据回退。
const TEST_KEY = process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";

// 两腿共用对照表：产品引擎 agent_loop.rs 的单测读同一文件。
const guards: { maxConsecutiveUnknownToolRounds: number } = JSON.parse(readFileSync(
  new URL("../../../studio/src/__tests__/golden/agent-loop-guards.json", import.meta.url), "utf-8"));
const LIMIT = guards.maxConsecutiveUnknownToolRounds;

/** 一轮模型回复里的工具调用；一轮 = 一次带工具的上游请求。 */
type Call = { name: string; args?: Record<string, unknown> };
const UNKNOWN: Call = { name: "no_such_tool" };
const LIST_WORKS: Call = { name: "workspace__list_works" };
const FINISH: Call = { name: "finish_turn", args: { status: "answered", message: "A scene is a unit of dramatic action." } };
const unknownRounds = (count: number): Call[][] => Array.from({ length: count }, () => [UNKNOWN]);

/**
 * 回环上游：按脚本逐轮回工具调用。脚本用完还来请求就回 500——守卫一旦被拿掉，
 * 用例会很快以「请求数对不上」变红，而不是空转到测试超时。
 */
async function startUpstream(script: Call[][]) {
  let toolRequests = 0;
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) chunks.push(chunk as Buffer);
    const body = JSON.parse(Buffer.concat(chunks).toString("utf8")) as { tools?: Array<{ function: { name: string } }> };
    // 送达口径的独立小请求（不带主工具表）：照常放行，不计轮。
    if (body.tools?.[0]?.function.name === "submit_requested_operations") {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ choices: [{ finish_reason: "tool_calls", message: { tool_calls: [{
        id: "scope", type: "function", function: { name: "submit_requested_operations", arguments: JSON.stringify({ coverQuote: "", preDraftReviewQuote: "", contentReviewQuote: "", exportQuote: "" }) },
      }] } }] }));
      return;
    }
    const calls = script[toolRequests];
    toolRequests += 1;
    if (!calls) { response.writeHead(500); response.end(); return; }
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ choices: [{ finish_reason: "tool_calls", message: { role: "assistant", tool_calls: calls.map((call, index) => ({
      id: `call-${toolRequests}-${index}`, type: "function", function: { name: call.name, arguments: JSON.stringify(call.args ?? {}) },
    })) } }] }));
  });
  server.listen(0, "127.0.0.1"); await once(server, "listening");
  const model: Model<"openai-completions"> = { id: "fixture", name: "Fixture", provider: "openai", api: "openai-completions",
    baseUrl: `http://127.0.0.1:${(server.address() as { port: number }).port}/v1`, input: ["text"], reasoning: false,
    contextWindow: 128000, maxTokens: 8192, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 } };
  return {
    model,
    requests: () => toolRequests,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())));
    },
  };
}

const sessionConfig = (root: string, model: Model<"openai-completions">) => ({ projectRoot: root, sessionId: "unknown-tool-loop", bookId: null, workId: null, profileId: "workspace-default",
  sessionKind: "chat" as const, language: "en" as const, model, apiKey: TEST_KEY, stream: false, pipeline: {} as never });
type SessionConfig = ReturnType<typeof sessionConfig>;

async function withSession(script: Call[][], run: (config: SessionConfig, requests: () => number) => Promise<void>) {
  const root = await mkdtemp(join(tmpdir(), "inkos-unknown-tool-loop-"));
  const upstream = await startUpstream(script);
  try {
    await run(sessionConfig(root, upstream.model), upstream.requests);
  } finally {
    abortAgentSession(root, "unknown-tool-loop");
    await upstream.close();
    await rm(root, { recursive: true, force: true });
  }
}

it("stops the turn once the model has called only unknown tools for the guard's round limit", async () => {
  await withSession(unknownRounds(LIMIT + 20), async (config, requests) => {
    const result = await runAgentSession(config, "给我讲一段少年冒险的故事");
    expect(requests()).toBe(LIMIT);
    expect(result.errorMessage).toBe(`Agent stopped: the model called unknown tools ("no_such_tool") in ${LIMIT} consecutive turns.`);
    expect(result.completion).toBeUndefined();
    expect(result.responseText).toBe("");
    const events = await readTranscriptEvents(config.projectRoot, config.sessionId);
    expect(events.at(-1)).toMatchObject({ type: "request_failed", error: result.errorMessage });
  });
}, 20000);

it("lets the model recover when it finds a real tool before the limit", async () => {
  await withSession([...unknownRounds(LIMIT - 1), [FINISH]], async (config, requests) => {
    const result = await runAgentSession(config, "给我讲一段少年冒险的故事");
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(LIMIT);
  });
}, 20000);

it("resets the count on a round that includes a real tool, and counts a same-name repeat as one unknown round", async () => {
  // 第 1 轮同名未知工具调两次 = 1 个未知轮；第 2 轮夹着已知工具 → 归零；之后连续 LIMIT-1 轮未知，不到上限。
  const script: Call[][] = [[UNKNOWN, UNKNOWN], [UNKNOWN, LIST_WORKS], ...unknownRounds(LIMIT - 1), [FINISH]];
  await withSession(script, async (config, requests) => {
    const result = await runAgentSession(config, "给我讲一段少年冒险的故事");
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(LIMIT + 2);
  });
}, 20000);

it("counts a round that repeats one unknown name as a single unknown round on the way to the limit", async () => {
  // 归零之后：[同名 ×2] 算 1 个未知轮，再 4 个未知轮 → 恰好在上限停。若把「同名重复」
  // 误当成夹了已知工具而归零，会多打一轮才停。
  const script: Call[][] = [[UNKNOWN, LIST_WORKS], [UNKNOWN, UNKNOWN], ...unknownRounds(20)];
  await withSession(script, async (config, requests) => {
    const result = await runAgentSession(config, "给我讲一段少年冒险的故事");
    expect(requests()).toBe(LIMIT + 1);
    expect(result.errorMessage).toBe(`Agent stopped: the model called unknown tools ("no_such_tool") in ${LIMIT} consecutive turns.`);
    expect(result.completion).toBeUndefined();
  });
}, 20000);

it("does not carry an earlier request's unknown rounds into the next request", async () => {
  // 上一个请求留下 3 个未知轮后正常收尾；这一个请求再来 2 个，不该凑满上限。
  // 成立靠的是收尾那一轮（finish_turn）本身把计数归零，不靠每次请求重置。
  const script: Call[][] = [...unknownRounds(3), [FINISH], ...unknownRounds(2), [FINISH]];
  await withSession(script, async (config, requests) => {
    const first = await runAgentSession(config, "给我讲一段少年冒险的故事");
    const afterFirst = requests();
    const second = await runAgentSession(config, "再讲一段");
    expect(first.errorMessage).toBeUndefined();
    expect(first.completion?.status).toBe("answered");
    expect(afterFirst).toBe(4);
    expect(second.errorMessage).toBeUndefined();
    expect(second.completion?.status).toBe("answered");
    expect(requests()).toBe(7);
  });
}, 20000);
