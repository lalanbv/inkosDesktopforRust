//! G1/348 号：embedding 客户端与增量向量化单测。
//! 决策表语义由 golden-semantic-retrieval 覆盖；此处验证双协议请求形态与
//! 配置校验（HTTP 层 mock，协议路径/头部/批量透传）。
import { describe, expect, it, vi, afterEach } from "vitest";
import { createEmbeddingClient, EmbeddingConfigSchema } from "../retrieval/embedding-client.js";

const captured: Array<{ url: string; init: RequestInit }> = [];

function mockFetch(responseBody: unknown): void {
  vi.stubGlobal("fetch", vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    captured.push({ url: String(url), init: init ?? {} });
    return new Response(JSON.stringify(responseBody), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }));
}

afterEach(() => {
  vi.unstubAllGlobals();
  captured.length = 0;
});

describe("embedding client (G1/348)", () => {
  it("rejects invalid configs and returns null for absent config", () => {
    expect(createEmbeddingClient(null)).toBeNull();
    expect(createEmbeddingClient(undefined)).toBeNull();
    expect(createEmbeddingClient({ provider: "ollama" })).toBeNull();
    expect(createEmbeddingClient({ provider: "bogus", baseUrl: "http://x", model: "m" })).toBeNull();
    expect(EmbeddingConfigSchema.safeParse({ provider: "openai-compatible", baseUrl: "not-a-url", model: "m" }).success).toBe(false);
  });

  it("openai-compatible: posts batch input with bearer auth to /embeddings", async () => {
    mockFetch({ data: [{ embedding: [1, 2] }, { embedding: [3, 4] }] });
    vi.stubEnv("TEST_KEY_ENV", "sk-test");
    const client = createEmbeddingClient({
      provider: "openai-compatible",
      baseUrl: "http://127.0.0.1:9/v1",
      model: "text-embedding-x",
      apiKeyEnv: "TEST_KEY_ENV",
    });
    expect(client).not.toBeNull();
    const vectors = await client!.embed(["甲", "乙"]);
    expect(vectors).toEqual([[1, 2], [3, 4]]);
    expect(captured[0]!.url).toBe("http://127.0.0.1:9/v1/embeddings");
    const body = JSON.parse(String(captured[0]!.init.body));
    expect(body).toEqual({ model: "text-embedding-x", input: ["甲", "乙"] });
    expect((captured[0]!.init.headers as Record<string, string>).Authorization).toBe("Bearer sk-test");
  });

  it("ollama: posts batch input to /api/embed without auth", async () => {
    mockFetch({ embeddings: [[5]] });
    const client = createEmbeddingClient({
      provider: "ollama",
      baseUrl: "http://127.0.0.1:11434",
      model: "nomic-embed-text",
    });
    expect(client).not.toBeNull();
    const vectors = await client!.embed(["正文片段"]);
    expect(vectors).toEqual([[5]]);
    expect(captured[0]!.url).toBe("http://127.0.0.1:11434/api/embed");
    const body = JSON.parse(String(captured[0]!.init.body));
    expect(body).toEqual({ model: "nomic-embed-text", input: ["正文片段"] });
    expect((captured[0]!.init.headers as Record<string, string>).Authorization).toBeUndefined();
  });

  it("empty batch short-circuits without HTTP", async () => {
    mockFetch({ data: [] });
    const client = createEmbeddingClient({
      provider: "openai-compatible",
      baseUrl: "http://127.0.0.1:9/v1",
      model: "m",
    });
    expect(await client!.embed([])).toEqual([]);
    expect(captured).toHaveLength(0);
  });
});
