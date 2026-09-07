import { describe, expect, it, vi } from "vitest";

/**
 * 206 号：瞬时重试行为级对跑（与 engine-rs agent_router tests 同构）——
 * 同一 flaky 形态（首 429 后 200）验证 Node 侧 withTransientLLMRetry：
 * 重试成功（2 次 fetch）/ 耗尽如实失败（3 次）/ model_not_available 直败（1 次）。
 * 判定函数单测见 llm-retry.test.ts；本文件走真实 chatCompletion 链
 * （native transport + proxy-fetch mock）。
 */
const { fetchCalls, responseQueue } = vi.hoisted(() => ({
  fetchCalls: [] as Array<{ url: string; init: RequestInit; body: Record<string, unknown> }>,
  responseQueue: [] as Response[],
}));

vi.mock("../utils/proxy-fetch.js", () => ({
  fetchWithProxy: vi.fn(async (url: string, init: RequestInit) => {
    fetchCalls.push({ url, init, body: JSON.parse(String(init.body ?? "{}")) });
    const queued = responseQueue.shift();
    if (queued) return queued;
    return {
      ok: true,
      json: async () => ({
        choices: [{ message: { content: "OK" } }],
        usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
      }),
    } as Response;
  }),
}));

function statusResponse(status: number, body: string): Response {
  return {
    ok: false,
    status,
    statusText: body,
    text: async () => body,
  } as unknown as Response;
}

async function makeClient() {
  const { createLLMClient } = await import("../llm/provider.js");
  return createLLMClient({
    provider: "openai",
    service: "minimax",
    model: "test-model",
    apiKey: "sk-test",
    apiFormat: "chat",
    stream: false,
    temperature: 0.1,
    thinkingBudget: 0,
    extra: {},
  } as never);
}

function userMessage() {
  return [{ role: "user" as const, content: "x" }];
}

describe("chatCompletion transient retry behavior (206 号双端对跑)", () => {
  it("retries one transient 429 and succeeds (2 fetches)", async () => {
    const { chatCompletion } = await import("../llm/provider.js");
    const client = await makeClient();
    fetchCalls.length = 0;
    responseQueue.length = 0;
    responseQueue.push(statusResponse(429, '{"error":"rate limit"}'));
    // 第二次：队列空 → 默认 200 JSON 响应。

    const res = await chatCompletion(client, "test-model", userMessage(), {});
    expect(res.content).toBe("OK");
    // 首次 429 + 重试 1 次
    expect(fetchCalls.length).toBe(2);
  });

  it("exhausts the 2-retry budget on persistent 503 (3 fetches) and fails honestly", async () => {
    const { chatCompletion } = await import("../llm/provider.js");
    const client = await makeClient();
    fetchCalls.length = 0;
    responseQueue.length = 0;
    responseQueue.push(
      statusResponse(503, "service unavailable"),
      statusResponse(503, "service unavailable"),
      statusResponse(503, "service unavailable"),
    );

    await expect(chatCompletion(client, "test-model", userMessage(), {}))
      .rejects.toThrow(/503/);
    // 1 次原始 + 2 次重试
    expect(fetchCalls.length).toBe(3);
  });

  it("does not retry a permanent model_not_available 500 (1 fetch)", async () => {
    const { chatCompletion } = await import("../llm/provider.js");
    const client = await makeClient();
    fetchCalls.length = 0;
    responseQueue.length = 0;
    responseQueue.push(
      statusResponse(500, '{"error":{"code":"model_not_available","message":"model not available"}}'),
    );

    await expect(chatCompletion(client, "test-model", userMessage(), {}))
      .rejects.toThrow(/model not available|500/);
    // 非瞬时直接失败
    expect(fetchCalls.length).toBe(1);
  });
});
