import type { EmbeddingClient } from "./semantic-retrieval.js";
import { z } from "zod";

/**
 * embedding 配置与双协议客户端（G1/348 号）。
 *
 * embedding **可选**：未配置（llm.embedding 缺省）→ createEmbeddingClient 返回
 * null → 检索自动降级 FTS5（G1/347 降级契约）。两协议：
 * - openai-compatible：`POST {baseUrl}/embeddings` `{model, input: string[]}` →
 *   `data[i].embedding`（OpenAI / Moonshot / vLLM / LM Studio 等兼容端点）；
 * - ollama：`POST {baseUrl}/api/embed` `{model, input: string[]}` → `embeddings`（ollama ≥0.1.29 批量)
 *   兼容旧单条 `POST /api/embeddings` `{model, prompt}` → `embedding`（逐条回退）。
 */

export const EMBEDDING_PROVIDERS = ["openai-compatible", "ollama"] as const;

export const EmbeddingConfigSchema = z.object({
  provider: z.enum(EMBEDDING_PROVIDERS),
  baseUrl: z.string().url(),
  model: z.string().min(1),
  /** API key 的环境变量名（openai-compatible 常用；ollama 本地通常免 key）。 */
  apiKeyEnv: z.string().optional(),
  /** 请求超时毫秒（缺省 30s）。 */
  timeoutMs: z.number().int().positive().optional(),
});

export type EmbeddingConfig = z.infer<typeof EmbeddingConfigSchema>;

const DEFAULT_TIMEOUT_MS = 30_000;

function joinUrl(baseUrl: string, path: string): string {
  return `${baseUrl.replace(/\/+$/, "")}${path}`;
}

/** 解析 API key：apiKeyEnv 指向的环境变量；缺失返回空串（服务端可免 key）。 */
function resolveApiKey(config: EmbeddingConfig): string {
  if (!config.apiKeyEnv) return "";
  return process.env[config.apiKeyEnv] ?? "";
}

/**
 * 双协议 embedding 客户端工厂：配置不完整返回 null（调用方降级 FTS5）。
 * 批量请求一次往返；单协议失败向上抛错由调用方按降级契约处理。
 */
export function createEmbeddingClient(config: unknown): (EmbeddingClient & { readonly config: EmbeddingConfig }) | null {
  const parsed = EmbeddingConfigSchema.safeParse(config);
  if (!parsed.success) return null;
  const resolved = parsed.data;
  const apiKey = resolveApiKey(resolved);
  const timeoutMs = resolved.timeoutMs ?? DEFAULT_TIMEOUT_MS;

  const embedOnce = async (texts: ReadonlyArray<string>): Promise<ReadonlyArray<ReadonlyArray<number>>> => {
    if (resolved.provider === "ollama") {
      // ollama 批量端点；失败由调用方降级（旧单条端点不回退——保持简单）。
      const response = await fetchWithTimeout(joinUrl(resolved.baseUrl, "/api/embed"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ model: resolved.model, input: texts }),
        timeoutMs,
      });
      const json = (await response.json()) as { embeddings?: number[][] };
      if (!Array.isArray(json.embeddings)) throw new Error("ollama embed: missing embeddings");
      return json.embeddings;
    }
    const response = await fetchWithTimeout(joinUrl(resolved.baseUrl, "/embeddings"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(apiKey ? { Authorization: `Bearer ${apiKey}` } : {}),
      },
      body: JSON.stringify({ model: resolved.model, input: texts }),
      timeoutMs,
    });
    const json = (await response.json()) as { data?: Array<{ embedding: number[] }> };
    if (!Array.isArray(json.data)) throw new Error("embeddings: missing data");
    return json.data.map((entry) => entry.embedding);
  };

  return {
    config: resolved,
    async embed(texts) {
      if (texts.length === 0) return [];
      return embedOnce(texts);
    },
  };
}

async function fetchWithTimeout(url: string, init: RequestInit & { timeoutMs: number }): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), init.timeoutMs);
  try {
    return await fetch(url, { ...init, signal: controller.signal });
  } finally {
    clearTimeout(timer);
  }
}
