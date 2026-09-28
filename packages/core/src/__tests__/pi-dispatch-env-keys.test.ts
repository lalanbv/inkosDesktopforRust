import { afterEach, describe, expect, it, vi } from "vitest";
import { getEnvApiKey } from "../llm/pi-env-keys.js";
import { piStreamSimple } from "../llm/pi-dispatch.js";
import type { Api, Context, Model } from "@earendil-works/pi-ai";

// R30b 正统化（554 号）：pi-env-keys/pi-dispatch 为上游 deprecated ./compat
// 子路径中 env-api-keys 与 streamSimple 分发的自持替代。本测试锁两模块的
// 行为面：env 键表（含 anthropic 特判）与 dispatch 表（api 键分发+未知抛错）。

const { completionsStreamSimple, responsesStreamSimple, anthropicStreamSimple } = vi.hoisted(() => ({
  completionsStreamSimple: vi.fn(() => "completions-stream"),
  responsesStreamSimple: vi.fn(() => "responses-stream"),
  anthropicStreamSimple: vi.fn(() => "anthropic-stream"),
}));

vi.mock("@earendil-works/pi-ai/api/openai-completions", () => ({
  streamSimple: completionsStreamSimple,
}));
vi.mock("@earendil-works/pi-ai/api/openai-responses", () => ({
  streamSimple: responsesStreamSimple,
}));
vi.mock("@earendil-works/pi-ai/api/anthropic-messages", () => ({
  streamSimple: anthropicStreamSimple,
}));

function makeModel(api: Api): Model<Api> {
  return {
    id: "m",
    name: "m",
    api,
    provider: "p",
    baseUrl: "https://example.invalid",
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 128_000,
    maxTokens: 4096,
  };
}

afterEach(() => {
  completionsStreamSimple.mockClear();
  responsesStreamSimple.mockClear();
  anthropicStreamSimple.mockClear();
});

describe("getEnvApiKey（上游 env-api-keys 自持镜像）", () => {
  it("resolves single-key providers from the upstream table", () => {
    expect(getEnvApiKey("openai", { OPENAI_API_KEY: "k1" })).toBe("k1");
    expect(getEnvApiKey("deepseek", { DEEPSEEK_API_KEY: "k2" })).toBe("k2");
    expect(getEnvApiKey("zai", { ZAI_API_KEY: "k3" })).toBe("k3");
    expect(getEnvApiKey("openrouter", { OPENROUTER_API_KEY: "k4" })).toBe("k4");
    expect(getEnvApiKey("moonshotai", { MOONSHOT_API_KEY: "k5" })).toBe("k5");
    expect(getEnvApiKey("google", { GEMINI_API_KEY: "k6" })).toBe("k6");
  });

  it("falls back to process.env when the scoped env omits the key", () => {
    const previous = process.env.OPENAI_API_KEY;
    process.env.OPENAI_API_KEY = "from-process";
    try {
      expect(getEnvApiKey("openai")).toBe("from-process");
    } finally {
      if (previous === undefined) delete process.env.OPENAI_API_KEY;
      else process.env.OPENAI_API_KEY = previous;
    }
  });

  it("mirrors the anthropic candidate order and skips AUTH_TOKEN", () => {
    // AUTH_TOKEN 在场但不作为 key 返回（上游须以 Authorization: Bearer 传递）。
    expect(getEnvApiKey("anthropic", { ANTHROPIC_AUTH_TOKEN: "bearer" })).toBeUndefined();
    // AUTH_TOKEN 在场时跳过，取 API_KEY。
    expect(
      getEnvApiKey("anthropic", { ANTHROPIC_AUTH_TOKEN: "bearer", ANTHROPIC_API_KEY: "sk" }),
    ).toBe("sk");
    // 仅 AUTH_TOKEN 在场时跳过后无候选。
    expect(getEnvApiKey("anthropic", { ANTHROPIC_OAUTH_TOKEN: "oauth" })).toBe("oauth");
  });

  it("returns undefined for unknown providers and ollama (ambient, keyless)", () => {
    expect(getEnvApiKey("ollama")).toBeUndefined();
    expect(getEnvApiKey("not-a-provider", { WHATEVER: "x" })).toBeUndefined();
  });
});

describe("piStreamSimple dispatch（上游 streamSimple 分发自持）", () => {
  it("routes by model.api to the matching per-API streamSimple", () => {
    const context = { messages: [{ role: "user", content: "hi" }] } as unknown as Context;
    expect(piStreamSimple(makeModel("openai-completions"), context)).toBe("completions-stream");
    expect(completionsStreamSimple).toHaveBeenCalledTimes(1);
    expect(piStreamSimple(makeModel("openai-responses"), context)).toBe("responses-stream");
    expect(responsesStreamSimple).toHaveBeenCalledTimes(1);
    expect(piStreamSimple(makeModel("anthropic-messages"), context)).toBe("anthropic-stream");
    expect(anthropicStreamSimple).toHaveBeenCalledTimes(1);
  });

  it("throws a clear error for an unregistered api value", () => {
    const context = { messages: [] } as unknown as Context;
    expect(() => piStreamSimple(makeModel("mistral-conversations" as Api), context)).toThrow(
      /No pi-ai API module registered for api: mistral-conversations/,
    );
  });
});
