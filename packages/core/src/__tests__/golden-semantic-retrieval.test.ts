//! G1/347 号：语义检索层 golden 断言（Phase C 首项首批）。
//!
//! 唯一事实源 = `golden/semantic-retrieval-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_semantic_retrieval_diff.rs` 读同一文件差分。
//! 六组断言：任务驱动查询构造、余弦相似度、向量 topK、RRF 混合融合、
//! 降级模式判定、契约形状。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  RETRIEVAL_MODES,
  buildTaskDrivenQuery,
  cosineSimilarity,
  reciprocalRankFusion,
  resolveRetrievalMode,
  topKBySimilarity,
} from "../retrieval/semantic-retrieval.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/semantic-retrieval-vectors.json"), "utf-8"),
) as {
  query: Array<{ name: string; input: Parameters<typeof buildTaskDrivenQuery>[0]; expected: string }>;
  cosine: Array<{ name: string; a: number[]; b: number[]; expected: number }>;
  topK: Array<{
    name: string;
    input: {
      k: number;
      queryVector: number[];
      chunks: Array<{ id: string; vector: number[] }>;
    };
    expected: Array<{ id: string; similarity: number }>;
  }>;
  rrf: Array<{
    name: string;
    input: { k: number; semantic: Array<{ id: string; rank: number }>; fts: Array<{ id: string; rank: number }>} ;
    expected: { order: string[]; sources: Record<string, string[]> };
  }>;
  mode: Array<{ name: string; input: { embeddingAvailable: boolean; embeddingError: boolean }; expected: string }>;
  contract: unknown;
};

describe("semantic retrieval contract (G1)", () => {
  it("builds task-driven queries per shared vectors", () => {
    for (const vector of vectors.query) {
      expect(buildTaskDrivenQuery(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("computes cosine similarity per shared vectors", () => {
    for (const vector of vectors.cosine) {
      expect(cosineSimilarity(vector.a, vector.b), vector.name).toBeCloseTo(vector.expected, 12);
    }
  });

  it("selects topK per shared vectors", () => {
    for (const vector of vectors.topK) {
      const got = topKBySimilarity(vector.input.queryVector, vector.input.chunks, vector.input.k);
      expect(got.map((hit) => hit.id), vector.name).toEqual(
        vector.expected.map((hit) => hit.id),
      );
      // 相似度为浮点：逐项 12 位容差比较（id 顺序严格相等）。
      got.forEach((hit, i) => {
        expect(hit.similarity, `${vector.name}#${i}`).toBeCloseTo(
          vector.expected[i]!.similarity,
          12,
        );
      });
    }
  });

  it("fuses semantic and fts rankings via RRF per shared vectors", () => {
    for (const vector of vectors.rrf) {
      const fused = reciprocalRankFusion(
        vector.input.semantic,
        vector.input.fts,
        vector.input.k,
      );
      expect(fused.map((hit) => hit.id), vector.name).toEqual(vector.expected.order);
      for (const hit of fused) {
        expect(hit.sources, `${vector.name}:${hit.id}`).toEqual(vector.expected.sources[hit.id] ?? []);
      }
    }
  });

  it("resolves retrieval mode with fts5 fallback per shared vectors", () => {
    for (const vector of vectors.mode) {
      expect(resolveRetrievalMode(vector.input), vector.name).toBe(vector.expected);
    }
    expect(RETRIEVAL_MODES).toEqual(["semantic", "fts5-fallback"]);
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual({
      modes: [...RETRIEVAL_MODES],
      rrfK: 60,
      queryTermLimit: 8,
      queryCharLimit: 200,
      dialogueWindowNote: "embedding 服务后续号接入；本号只定确定性契约",
    });
  });
});
