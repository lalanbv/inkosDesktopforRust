//! G1/350a 号：混合召回记忆精选器单测。
//! 决策语义：指纹增量嵌入（未变跳过）、向量重排 topN、embedding 失败返回
//! 空数组（调用方回退 BM25）、空候选短路。
import { describe, expect, it } from "vitest";
import { createHybridMemorySelector } from "../retrieval/hybrid-memory-selector.js";
import type { EmbeddingClient } from "../retrieval/semantic-retrieval.js";

const VECTORS: Record<string, number[]> = {
  "青镇旧事 怀表": [0.95, 0.15],
  "关于怀表的悬念": [1, 0],
  "王胖的日常": [0, 1],
  "青镇旧事": [0.9, 0.1],
};

function mockClient(calls: string[][]): EmbeddingClient {
  return {
    async embed(texts: ReadonlyArray<string>) {
      calls.push(texts as string[]);
      return texts.map((text) => VECTORS[text] ?? [0, 0]);
    },
  };
}

function memoryCache() {
  const store = new Map<string, { fingerprint: string; vector: number[] }>();
  return {
    store,
    listChunkVectors() {
      return [...store.entries()].map(([chunkId, entry]) => ({
        chunkId,
        fingerprint: entry.fingerprint,
        vector: entry.vector,
      }));
    },
    upsertChunkVector(chunk: {
      chunkId: string;
      source: string;
      fingerprint: string;
      dim: number;
      vector: number[];
      createdAt: string;
    }) {
      store.set(chunk.chunkId, {
        fingerprint: chunk.fingerprint,
        vector: chunk.vector,
      });
    },
  };
}

describe("hybrid memory selector (G1/350a)", () => {
  it("reranks candidates by query similarity and caches embeddings", async () => {
    const calls: string[][] = [];
    const client = mockClient(calls);
    const cache = memoryCache();
    const selector = createHybridMemorySelector({ client, cache, limit: 2 });
    const request = {
      chapterNumber: 4,
      query: "青镇旧事 怀表",
      candidates: [
        { id: "s-2", kind: "summary", source: "story/chapter_summaries.md#2", title: "日常", excerpt: "王胖的日常" },
        { id: "s-1", kind: "summary", source: "story/chapter_summaries.md#1", title: "悬念", excerpt: "关于怀表的悬念" },
        { id: "s-3", kind: "summary", source: "story/chapter_summaries.md#3", title: "旧事", excerpt: "青镇旧事" },
      ],
    };

    const first = await selector(request);
    expect(first).toEqual(["s-3", "s-1"]);
    // 首轮嵌入：query 与候选分两次批量调用（query 先行，候选增量一批）。
    expect(calls[0]).toEqual(["青镇旧事 怀表"]);
    expect(calls[1]).toEqual(["王胖的日常", "关于怀表的悬念", "青镇旧事"]);
    expect(cache.store.size).toBe(3);

    // 二轮同候选：全部缓存命中，仅嵌入 query。
    const second = await selector(request);
    expect(second).toEqual(["s-3", "s-1"]);
    expect(calls[2]).toEqual(["青镇旧事 怀表"]);
  });

  it("returns empty on embedding failure (caller falls back to BM25)", async () => {
    const client: EmbeddingClient = {
      async embed() {
        throw new Error("embedding service down");
      },
    };
    const selector = createHybridMemorySelector({ client, limit: 2 });
    const got = await selector({
      chapterNumber: 1,
      query: "任意",
      candidates: [{ id: "a", kind: "summary", source: "x", title: "t", excerpt: "e" }],
    });
    expect(got).toEqual([]);
  });

  it("short-circuits empty candidate lists", async () => {
    const calls: string[][] = [];
    const selector = createHybridMemorySelector({ client: mockClient(calls) });
    expect(await selector({ chapterNumber: 1, query: "x", candidates: [] })).toEqual([]);
    expect(calls).toHaveLength(0);
  });
});
