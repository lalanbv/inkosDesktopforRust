//! 1666 号：R55 chat 流水用量账本**写入点**活体锁（TS 腿）。
//!
//! 1660/1661 只在 Rust 腿（RouterLoopChat）落了写入点；TS 侧 `appendUsageRows`
//! 仅导出、零生产调用 = Node 回退腿账本恒空，GET /usage/budget-status 恒报 0，
//! 80% 告警与 100% 确认门永不触发。1661 的 studio 测试直接往盘上播种账本行
//! 再打端点，所以 9/9 全绿也照不见写入面缺席（1663 run-log 同族）。
//!
//! 本文件走真实 agent 会话（回环 http 上游脚本化应答）锁：
//! - 每个模型轮恰落一条，字段=usage 精确值（隐私红线）；
//! - serviceKey = baseUrl 的 host **不含端口**（Rust `host_str()` 同口径——本地
//!   模型/自定义网关都带端口，`URL.host` 会把写点与读点拆开）；
//! - 写点 → 投影 → 预算判定在 core 层端到端贯通；
//! - 上游不给 usage / 上游失败 = 无精确值 → 不落行（不以估算顶替）。
import { createServer } from "node:http";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Model } from "@mariozechner/pi-ai";
import { runAgentSession, abortAgentSession } from "../agent/agent-session.js";
import { loadUsageLedger } from "../utils/usage-ledger-store.js";
import { budgetStatus, serviceHostOf, usageProjection } from "../utils/usage-budget.js";

// 1015/855 号惯用形：测试凭据走 env 注入 + 非凭据回退。
const TEST_KEY = process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";

// 代理层直通全局 fetch——上游是回环 http 服务，不进系统代理。
vi.mock("../utils/proxy-fetch.js", () => ({
  fetchWithProxy: (url: string, init: RequestInit) => fetch(url, init),
}));

// 包一层 spy（真实实现照跑）：负路径要证「没写」，靠等一段时间的缺席断言
// 只能得到假绿——直接断言写点没被调用。
const appendSpy = vi.hoisted(() => ({ appendUsageRows: vi.fn() }));
vi.mock("../utils/usage-ledger-store.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../utils/usage-ledger-store.js")>();
  appendSpy.appendUsageRows.mockImplementation((...args: Parameters<typeof actual.appendUsageRows>) => actual.appendUsageRows(...args));
  return { ...actual, appendUsageRows: appendSpy.appendUsageRows };
});

type Reply = { usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number } } | { status: number };

function finishTurnReply(id: number, withUsage: boolean) {
  return {
    choices: [{
      finish_reason: "tool_calls",
      message: {
        role: "assistant",
        tool_calls: [{
          id: `completion-${id}`,
          type: "function",
          function: { name: "finish_turn", arguments: JSON.stringify({ status: "answered", message: `Answer ${id}.` }) },
        }],
      },
    }],
    ...(withUsage ? { usage: { prompt_tokens: 3, completion_tokens: 1, total_tokens: 4 } } : {}),
  };
}

/** 回环上游：按请求序号取脚本应答（越界=500）。 */
async function spawnUpstream(script: Reply[]) {
  let calls = 0;
  const server = createServer(async (request, response) => {
    for await (const _chunk of request) {}
    calls += 1;
    const reply = script[calls - 1];
    if (!reply || "status" in reply) {
      response.writeHead(reply && "status" in reply ? reply.status : 500, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ error: { message: "script exhausted" } }));
      return;
    }
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify(finishTurnReply(calls, reply.usage !== undefined)));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const baseUrl = `http://127.0.0.1:${(server.address() as { port: number }).port}/v1`;
  const model: Model<"openai-completions"> = {
    id: "fixture", name: "Fixture", provider: "openai", api: "openai-completions", baseUrl,
    input: ["text"], reasoning: false, contextWindow: 128000, maxTokens: 8192,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  };
  const close = async () => {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())));
  };
  return { baseUrl, model, close, calls: () => calls };
}

/** 写点是 best-effort 异步落盘——轮询到目标行数（自同步，不猜固定 sleep）。 */
async function waitForRows(root: string, count: number) {
  const deadline = Date.now() + 5_000;
  for (;;) {
    const rows = await loadUsageLedger(root);
    if (rows.length >= count) return rows;
    if (Date.now() > deadline) throw new Error(`账本未在 5s 内落满 ${count} 行（现 ${rows.length}）`);
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

/** 剥离易变字段 ts 后的账本行口径。 */
const gauge = (rows: Awaited<ReturnType<typeof loadUsageLedger>>) =>
  rows.map((row) => ({ serviceKey: row.serviceKey, model: row.model, promptTokens: row.promptTokens, completionTokens: row.completionTokens, totalTokens: row.totalTokens }));

let roots: string[] = [];
beforeEach(() => {
  appendSpy.appendUsageRows.mockClear();
  roots = [];
});
afterEach(async () => {
  for (const root of roots) await rm(root, { recursive: true, force: true });
});

it("serviceHostOf：host 不含端口（Rust host_str 镜像），不可解析回退原值", () => {
  expect(serviceHostOf("https://API.DeepSeek.com/v1")).toBe("api.deepseek.com");
  expect(serviceHostOf("http://127.0.0.1:1234/v1")).toBe("127.0.0.1");
  expect(serviceHostOf("http://localhost:11434")).toBe("localhost");
  expect(serviceHostOf("http://[::1]:8080/v1")).toBe("[::1]");
  expect(serviceHostOf("not a url")).toBe("not a url");
  expect(serviceHostOf("")).toBe("");
});

it("chat 流水每个模型轮落一条：usage 精确值、serviceKey=host、写点→投影→预算贯通", async () => {
  const root = await mkdtemp(join(tmpdir(), "inkos-usage-ledger-chat-"));
  roots.push(root);
  const upstream = await spawnUpstream([{ usage: { prompt_tokens: 3, completion_tokens: 1, total_tokens: 4 } }, { usage: { prompt_tokens: 3, completion_tokens: 1, total_tokens: 4 } }]);
  // 端口是非默认口：serviceKey 必须是不带端口的 host，否则 budget-status 读侧
  // （Rust host_str 口径）匹配不上。
  expect(upstream.baseUrl).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/v1$/);
  const config = {
    projectRoot: root, sessionId: "usage-ledger", bookId: null, workId: null, profileId: "workspace-default",
    sessionKind: "chat" as const, language: "en" as const, model: upstream.model, apiKey: TEST_KEY, stream: false, pipeline: {} as never,
  };
  try {
    const first = await runAgentSession(config, "First question.");
    expect(first.completion?.status).toBe("answered");
    expect(first.errorMessage).toBeUndefined();
    expect(upstream.calls()).toBe(1);
    const rows = await waitForRows(root, 1);
    expect(gauge(rows)).toEqual([
      { serviceKey: "127.0.0.1", model: "fixture", promptTokens: 3, completionTokens: 1, totalTokens: 4 },
    ]);
    expect(rows[0]!.ts).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/);

    // 第二轮（同会话续聊）再落一条——每个模型轮一条，不是每会话一条。
    const second = await runAgentSession(config, "Second question.");
    expect(second.completion?.status).toBe("answered");
    expect(upstream.calls()).toBe(2);
    const all = await waitForRows(root, 2);
    expect(gauge(all)).toHaveLength(2);
    expect(appendSpy.appendUsageRows).toHaveBeenCalledTimes(2);

    // 写点 → 投影 → 预算判定：以账本行自身的 dayKey 为「今日」
    // （跨 UTC 零点不受墙上时钟影响）。日顶 8 → 100% → over + 确认门。
    const today = all[0]!.ts.slice(0, 10);
    const projection = usageProjection(all, (row) => row.ts.slice(0, 10), today);
    expect(projection.get("127.0.0.1")).toEqual({ daily: 8, monthly: 8, total: 8 });
    expect(budgetStatus("127.0.0.1", projection.get("127.0.0.1")!, { dailyTokenBudget: 8 }))
      .toMatchObject({ daily: 8, ratio: 1, level: "over", confirmRequired: true });
  } finally {
    abortAgentSession(root, config.sessionId);
    await upstream.close();
  }
}, 20000);

it("上游不给 usage 精确值（缺 usage / 上游失败）→ 不落行，不以估算顶替", async () => {
  const root = await mkdtemp(join(tmpdir(), "inkos-usage-ledger-chat-none-"));
  roots.push(root);
  const upstream = await spawnUpstream([{}, { status: 500 }]);
  const config = {
    projectRoot: root, sessionId: "usage-ledger-none", bookId: null, language: "en" as const,
    model: upstream.model, apiKey: TEST_KEY, stream: false, pipeline: {} as never,
  };
  try {
    const answered = await runAgentSession(config, "Question without usage.");
    expect(answered.completion?.status).toBe("answered");
    // 上游失败（500 非瞬态，不退避重试；脚本越界的请求同样 500）。
    const failed = await runAgentSession(config, "Question that fails.");
    expect(failed.errorMessage).toBeTruthy();
    expect(appendSpy.appendUsageRows).not.toHaveBeenCalled();
    expect(await loadUsageLedger(root)).toEqual([]);
  } finally {
    abortAgentSession(root, config.sessionId);
    await upstream.close();
  }
}, 20000);
