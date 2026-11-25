import { createServer } from "node:http";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";
import type { Model } from "@mariozechner/pi-ai";
import { abortAgentSession, runAgentSession } from "../agent/agent-session.js";
import { createWorkManifest, saveWorkManifest } from "../harness/work-store.js";
import { readTranscriptEvents } from "../interaction/session-transcript.js";
// 1015 号：测试凭据惯用形——fixture 值走 env 注入 + 非凭据回退。
const TEST_KEY = process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";

// 两腿共用对照表：产品引擎 agent_loop.rs 的单测读同一文件（模型调用硬上限只在 Node 腿）。
const guards: {
  maxConsecutiveUnknownToolRounds: number;
  maxConsecutiveFailedToolRounds: number;
  maxRepeatedRounds: number;
  repeatLookbackRounds: number;
  maxModelRoundsPerTurn: number;
} = JSON.parse(readFileSync(
  new URL("../../../studio/src/__tests__/golden/agent-loop-guards.json", import.meta.url), "utf-8"));
const UNKNOWN_LIMIT = guards.maxConsecutiveUnknownToolRounds;
const FAILED_LIMIT = guards.maxConsecutiveFailedToolRounds;
const REPEAT_LIMIT = guards.maxRepeatedRounds;

/** 一轮模型回复里的工具调用；一轮 = 一次带工具的上游请求。 */
type Call = { name: string; args?: Record<string, unknown> };
/** 脚本的一步：这一轮回哪些调用；`before` 在应答前执行（用来在轮与轮之间改磁盘）。 */
type Step = Call[] | { calls: Call[]; before?: (root: string) => Promise<void> };
const UNKNOWN: Call = { name: "no_such_tool" };
const LIST: Call = { name: "workspace__list_works" };
const LIST_SCRIPT: Call = { name: "workspace__list_works", args: { profileId: "script" } };
// profileId 传空串等价于不过滤：调用参数不同、返回文本相同。
const LIST_BLANK: Call = { name: "workspace__list_works", args: { profileId: "" } };
const FINISH: Call = { name: "finish_turn", args: { status: "answered", message: "A scene is a unit of dramatic action." } };
const repeat = (call: Call, count: number): Call[][] => Array.from({ length: count }, () => [call]);
// 每轮读一个不存在的文件：已知工具、每次都失败；路径各不相同，所以不是「原地重复」。
const failReads = (count: number, batch = "a"): Call[][] => Array.from({ length: count }, (_, index) => [{ name: "workspace__read", args: { path: `missing-${batch}-${index}.md` } }]);

/**
 * 回环上游：按脚本逐轮回工具调用。脚本用完还来请求就回 500——守卫一旦被拿掉，
 * 用例会很快以「请求数对不上」变红，而不是空转到测试超时。
 */
async function startUpstream(script: Step[], root: string) {
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
    const step = script[toolRequests];
    toolRequests += 1;
    if (!step) { response.writeHead(500); response.end(); return; }
    const calls = Array.isArray(step) ? step : step.calls;
    if (!Array.isArray(step)) await step.before?.(root);
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

const sessionConfig = (root: string, model: Model<"openai-completions">) => ({ projectRoot: root, sessionId: "agent-turn-guard", bookId: null, workId: null, profileId: "workspace-default",
  sessionKind: "chat" as const, language: "en" as const, model, apiKey: TEST_KEY, stream: false, pipeline: {} as never });
type SessionConfig = ReturnType<typeof sessionConfig>;

async function withSession(script: Step[], run: (config: SessionConfig, requests: () => number) => Promise<void>) {
  const root = await mkdtemp(join(tmpdir(), "inkos-agent-turn-guard-"));
  const upstream = await startUpstream(script, root);
  try {
    await run(sessionConfig(root, upstream.model), upstream.requests);
  } finally {
    abortAgentSession(root, "agent-turn-guard");
    await upstream.close();
    await rm(root, { recursive: true, force: true });
  }
}

const ask = (config: SessionConfig) => runAgentSession(config, "给我讲一段少年冒险的故事");
/** 在环境变量改动下跑一段；跑完恢复原值。 */
async function withRoundCeiling(value: string | undefined, run: () => Promise<void>) {
  const prior = process.env.INKOS_AGENT_MAX_ROUNDS_PER_TURN;
  if (value === undefined) delete process.env.INKOS_AGENT_MAX_ROUNDS_PER_TURN; else process.env.INKOS_AGENT_MAX_ROUNDS_PER_TURN = value;
  try { await run(); } finally {
    if (prior === undefined) delete process.env.INKOS_AGENT_MAX_ROUNDS_PER_TURN; else process.env.INKOS_AGENT_MAX_ROUNDS_PER_TURN = prior;
  }
}

// ── ① 未知工具 ──

it("stops the turn once the model has called only unknown tools for the guard's round limit", async () => {
  await withSession(repeat(UNKNOWN, UNKNOWN_LIMIT + 20), async (config, requests) => {
    const result = await ask(config);
    expect(requests()).toBe(UNKNOWN_LIMIT);
    expect(result.errorMessage).toBe(`Agent stopped: the model called unknown tools ("no_such_tool") in ${UNKNOWN_LIMIT} consecutive turns.`);
    expect(result.completion).toBeUndefined();
    expect(result.responseText).toBe("");
    const events = await readTranscriptEvents(config.projectRoot, config.sessionId);
    expect(events.at(-1)).toMatchObject({ type: "request_failed", error: result.errorMessage });
  });
}, 20000);

it("lets the model recover when it finds a real tool before the limit", async () => {
  await withSession([...repeat(UNKNOWN, UNKNOWN_LIMIT - 1), [FINISH]], async (config, requests) => {
    const result = await ask(config);
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(UNKNOWN_LIMIT);
  });
}, 20000);

it("resets the count on a round that includes a real tool, and counts a same-name repeat as one unknown round", async () => {
  // 第 1 轮同名未知工具调两次 = 1 个未知轮；第 2 轮夹着已知工具 → 归零；之后连续 UNKNOWN_LIMIT-1 轮未知，不到上限。
  const script: Step[] = [[UNKNOWN, UNKNOWN], [UNKNOWN, LIST], ...repeat(UNKNOWN, UNKNOWN_LIMIT - 1), [FINISH]];
  await withSession(script, async (config, requests) => {
    const result = await ask(config);
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(UNKNOWN_LIMIT + 2);
  });
}, 20000);

it("counts a round that repeats one unknown name as a single unknown round on the way to the limit", async () => {
  // 归零之后：[同名 ×2] 算 1 个未知轮，再 4 个未知轮 → 恰好在上限停。若把「同名重复」
  // 误当成夹了已知工具而归零，会多打一轮才停。
  const script: Step[] = [[UNKNOWN, LIST], [UNKNOWN, UNKNOWN], ...repeat(UNKNOWN, 20)];
  await withSession(script, async (config, requests) => {
    const result = await ask(config);
    expect(requests()).toBe(UNKNOWN_LIMIT + 1);
    expect(result.errorMessage).toBe(`Agent stopped: the model called unknown tools ("no_such_tool") in ${UNKNOWN_LIMIT} consecutive turns.`);
    expect(result.completion).toBeUndefined();
  });
}, 20000);

it("does not carry an earlier request's unknown rounds into the next request", async () => {
  // 上一个请求留下 3 个未知轮后正常收尾；这一个请求再来 2 个，不该凑满上限。
  // 成立靠的是收尾那一轮（finish_turn）本身把计数归零，不靠每次请求重置。
  const script: Step[] = [...repeat(UNKNOWN, 3), [FINISH], ...repeat(UNKNOWN, 2), [FINISH]];
  await withSession(script, async (config, requests) => {
    const first = await ask(config);
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

// ── ② 整轮的工具调用全部失败 ──

it("stops the turn once every tool call has failed for the failed-tool limit", async () => {
  await withSession(failReads(FAILED_LIMIT + 10), async (config, requests) => {
    const result = await ask(config);
    expect(requests()).toBe(FAILED_LIMIT);
    expect(result.errorMessage).toBe(`Agent stopped: every tool call failed in ${FAILED_LIMIT} consecutive turns (last failures: "workspace__read").`);
    expect(result.completion).toBeUndefined();
    const events = await readTranscriptEvents(config.projectRoot, config.sessionId);
    expect(events.at(-1)).toMatchObject({ type: "request_failed", error: result.errorMessage });
  });
}, 20000);

it("lets the model recover from failed calls before the failed-tool limit", async () => {
  await withSession([...failReads(FAILED_LIMIT - 1), [FINISH]], async (config, requests) => {
    const result = await ask(config);
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(FAILED_LIMIT);
  });
}, 20000);

it("resets the failed-tool count on a round with a successful call", async () => {
  // 第二批换一组路径：同样的失败读取再来一遍会被第 ③ 层（原地重复）拦下，考察不到这一层的归零。
  const script: Step[] = [...failReads(FAILED_LIMIT - 1, "a"), [LIST], ...failReads(FAILED_LIMIT - 1, "b"), [FINISH]];
  await withSession(script, async (config, requests) => {
    const result = await ask(config);
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(2 * FAILED_LIMIT);
  });
}, 20000);

it("does not count a round as failed while at least one of its calls succeeds", async () => {
  // 每轮都是「一次失败的读取 + 一次成功的列举」，路径各不相同（不是原地重复）：
  // 只要有一笔成功，这一轮就不算「全部失败」，连续数永远凑不满。
  const script: Step[] = Array.from({ length: 10 }, (_, index) => [{ name: "workspace__read", args: { path: `missing-${index}.md` } }, LIST]);
  script.push([FINISH]);
  await withSession(script, async (config, requests) => {
    const result = await ask(config);
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(11);
  });
}, 20000);

// ── ③ 原地重复：调用与结果都相同 ──

it("stops the turn once the model has repeated an identical round for the repeat limit", async () => {
  await withSession(repeat(LIST, REPEAT_LIMIT + 10), async (config, requests) => {
    const result = await ask(config);
    // 第 1 轮是原样，之后每轮都是重复：重复满 REPEAT_LIMIT 轮时停。
    expect(requests()).toBe(REPEAT_LIMIT + 1);
    expect(result.errorMessage).toBe(`Agent stopped: the model repeated an earlier step of this request ${REPEAT_LIMIT} times in a row with identical calls and results.`);
    expect(result.completion).toBeUndefined();
    const events = await readTranscriptEvents(config.projectRoot, config.sessionId);
    expect(events.at(-1)).toMatchObject({ type: "request_failed", error: result.errorMessage });
  });
}, 20000);

it("catches a two-step loop that alternates between two rounds", async () => {
  // A、B 交替：第 3 轮起每一轮都与本回合前面某一轮相同。
  const loop: Call[][] = [];
  for (let index = 0; index < REPEAT_LIMIT + 4; index += 1) loop.push([index % 2 ? LIST_SCRIPT : LIST]);
  await withSession(loop, async (config, requests) => {
    const result = await ask(config);
    expect(requests()).toBe(REPEAT_LIMIT + 2);
    expect(result.errorMessage).toMatch(/^Agent stopped: the model repeated an earlier step/);
  });
}, 20000);

it("does not count identical calls as repeats while their results keep changing", async () => {
  // 同一个调用、每轮结果都在变（应答前新建一个作品）：模型每轮都拿到新信息，不是原地重复。
  const script: Step[] = Array.from({ length: 10 }, (_, index): Step => (index === 0 ? [LIST] : {
    calls: [LIST],
    before: async (root) => {
      await saveWorkManifest(root, createWorkManifest({ id: `work-${index}`, title: `Work ${index}`, profileId: "script", language: "en" }));
    },
  }));
  script.push([FINISH]);
  await withSession(script, async (config, requests) => {
    const result = await ask(config);
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(11);
  });
}, 20000);

it("treats different arguments as a different round even when the result is the same", async () => {
  // {} 与 {profileId: ""} 返回文本相同、参数不同：按两步循环处理（第 REPEAT_LIMIT+2 轮停）。
  // 若参数不进指纹，两轮会被当成同一轮，提前一轮停。
  const loop: Call[][] = [];
  for (let index = 0; index < REPEAT_LIMIT + 4; index += 1) loop.push([index % 2 ? LIST_BLANK : LIST]);
  await withSession(loop, async (config, requests) => {
    const result = await ask(config);
    expect(requests()).toBe(REPEAT_LIMIT + 2);
    expect(result.errorMessage).toMatch(/^Agent stopped: the model repeated an earlier step/);
  });
}, 20000);

it("lets the model redo an identical round up to one below the repeat limit", async () => {
  await withSession([...repeat(LIST, REPEAT_LIMIT - 1), [FINISH]], async (config, requests) => {
    const result = await ask(config);
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(REPEAT_LIMIT);
  });
}, 20000);

it("resets the repeat count on a round that has not happened before", async () => {
  const script: Step[] = [...repeat(LIST, REPEAT_LIMIT - 1), [LIST_SCRIPT], ...repeat(LIST, REPEAT_LIMIT - 1), [FINISH]];
  await withSession(script, async (config, requests) => {
    const result = await ask(config);
    expect(result.errorMessage).toBeUndefined();
    expect(result.completion?.status).toBe("answered");
    expect(requests()).toBe(2 * REPEAT_LIMIT);
  });
}, 20000);

it("does not let the previous request's rounds make an identical re-run look like a loop", async () => {
  // 上一个请求：A ×(REPEAT_LIMIT-1) 后收尾；这一个请求：照原样再来 A ×REPEAT_LIMIT 后收尾。
  // 重复检测的窗口每个请求开头清空——否则这一个请求的每一轮都「见过」，会被判成原地重复。
  const script: Step[] = [...repeat(LIST, REPEAT_LIMIT - 1), [FINISH], ...repeat(LIST, REPEAT_LIMIT), [FINISH]];
  await withSession(script, async (config, requests) => {
    const first = await ask(config);
    const afterFirst = requests();
    const second = await runAgentSession(config, "再检查一遍");
    expect(first.errorMessage).toBeUndefined();
    expect(afterFirst).toBe(REPEAT_LIMIT);
    expect(second.errorMessage).toBeUndefined();
    expect(second.completion?.status).toBe("answered");
    expect(requests()).toBe(2 * REPEAT_LIMIT + 1);
  });
}, 20000);

// ── ④ 每回合模型调用次数的硬上限 ──

it("caps the model calls one turn may make and starts the next turn from zero", async () => {
  // 上限调成 4（远低于各快停阈值，单独考察硬上限）：
  //   第 1、2 个请求各 4 次调用后收尾（调用数每个请求从零起算，否则第 2 个请求第一次调用就会超）；
  //   第 3 个请求不收尾：恰好 4 次调用后停，并说明是上限。
  const script: Step[] = [...repeat(LIST, 3), [FINISH], ...repeat(LIST, 3), [FINISH], ...repeat(LIST, 12)];
  await withSession(script, async (config, requests) => {
    await withRoundCeiling("4", async () => {
      const first = await ask(config);
      const second = await runAgentSession(config, "再讲一段");
      const third = await runAgentSession(config, "再讲一段");
      expect(first.errorMessage).toBeUndefined();
      expect(second.errorMessage).toBeUndefined();
      expect(requests()).toBe(12);
      expect(third.errorMessage).toBe("Agent stopped: the turn exceeded 4 model calls (INKOS_AGENT_MAX_ROUNDS_PER_TURN).");
      expect(third.completion).toBeUndefined();
    });
  });
}, 20000);

it("ignores a round ceiling that is not a positive integer", async () => {
  // 乱填的上限一律回落到默认值（远高于这段流程的轮数）。
  await withSession([...repeat(LIST, 3), [FINISH], ...repeat(LIST, 3), [FINISH]], async (config, requests) => {
    await withRoundCeiling("0", async () => {
      const result = await ask(config);
      expect(result.errorMessage).toBeUndefined();
      expect(result.completion?.status).toBe("answered");
    });
    await withRoundCeiling("abc", async () => {
      const result = await runAgentSession(config, "再讲一段");
      expect(result.errorMessage).toBeUndefined();
      expect(result.completion?.status).toBe("answered");
    });
    expect(requests()).toBe(8);
  });
}, 20000);
