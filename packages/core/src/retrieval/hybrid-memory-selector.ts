import type { EmbeddingClient } from "./semantic-retrieval.js";
import { chunkFingerprint, selectStaleChunks, topKBySimilarity } from "./semantic-retrieval.js";
import type { MemorySemanticSelector } from "../utils/memory-retrieval.js";

/**
 * 混合召回记忆精选器（G1/350a 号）：write-next 检索链的语义精选实现。
 *
 * 输入 request.query 已由检索链按章任务单组装（347 号任务驱动口径）；
 * 本选择器把 BM25 候选做**向量重排**：
 *   1. 候选 excerpt 指纹对比缓存（MemoryDB.retrieval_chunks），仅增量嵌入；
 *   2. 查询与候选余弦 topN 作为精选 id 列表；
 *   3. embedding 任何失败 → 返回 []（检索链自动回退 BM25 排序，行为不变）。
 *
 * 返回类型对齐 `MemorySemanticSelector`，可直接替换默认 LLM 精选
 * （compose.memorySemanticSelector）。
 */

export interface HybridSelectorCache {
  listChunkVectors(): ReadonlyArray<{
    readonly chunkId: string;
    readonly fingerprint: string;
    readonly vector: ReadonlyArray<number>;
  }>;
  upsertChunkVector(chunk: {
    readonly chunkId: string;
    readonly source: string;
    readonly fingerprint: string;
    readonly dim: number;
    readonly vector: ReadonlyArray<number>;
    readonly createdAt: string;
  }): void;
}

export interface HybridSelectorOptions {
  readonly client: EmbeddingClient;
  /** 指纹缓存库（MemoryDB；缺省则每次全量嵌入、不缓存）。 */
  readonly cache?: HybridSelectorCache;
  /** 精选数量上限（默认 6，对齐 LLM 精选量级）。 */
  readonly limit?: number;
}

export function createHybridMemorySelector(options: HybridSelectorOptions): MemorySemanticSelector {
  const limit = options.limit ?? 6;
  return async (request) => {
    try {
      return await runHybridSelection(options, request, limit);
    } catch {
      // 降级契约（347 号）：embedding 任何失败 → 空精选，调用方回退 BM25 排序。
      return [];
    }
  };
}

async function runHybridSelection(
  options: HybridSelectorOptions,
  request: Parameters<MemorySemanticSelector>[0],
  limit: number,
): Promise<ReadonlyArray<string>> {
  {
    if (request.candidates.length === 0) return [];
    const queryVector = (await options.client.embed([request.query]))[0] ?? [];
    if (queryVector.length === 0) return [];

    const cached = new Map<string, { fingerprint: string; vector: ReadonlyArray<number> }>();
    if (options.cache) {
      for (const chunk of options.cache.listChunkVectors()) {
        cached.set(chunk.chunkId, { fingerprint: chunk.fingerprint, vector: chunk.vector });
      }
    }

    // 增量嵌入：仅指纹与缓存不一致（含无缓存）的候选。
    const cachedFingerprints = new Map<string, string>(
      [...cached.entries()].map(([id, entry]) => [id, entry.fingerprint]),
    );
    const stale = selectStaleChunks(
      request.candidates.map((candidate) => ({
        id: candidate.id,
        text: candidate.excerpt || candidate.title,
      })),
      cachedFingerprints,
    );
    if (stale.length > 0) {
      const vectors = await options.client.embed(stale.map((chunk) => chunk.text));
      const now = new Date().toISOString();
      stale.forEach((chunk, index) => {
        const vector = vectors[index];
        if (vector) {
          cached.set(chunk.id, { fingerprint: chunkFingerprint(chunk.text), vector });
          options.cache?.upsertChunkVector({
            chunkId: chunk.id,
            source: request.candidates.find((c) => c.id === chunk.id)?.source ?? "",
            fingerprint: chunkFingerprint(chunk.text),
            dim: vector.length,
            vector,
            createdAt: now,
          });
        }
      });
    }

    // 向量重排：全部候选取缓存的最新向量（新嵌 + 已缓存），余弦 topN。
    const ranked = topKBySimilarity(
      queryVector,
      request.candidates.map((candidate) => ({
        id: candidate.id,
        vector: [...(cached.get(candidate.id)?.vector ?? [])],
      })),
      limit,
    );
    return ranked.map((hit) => hit.id);
  };
}
