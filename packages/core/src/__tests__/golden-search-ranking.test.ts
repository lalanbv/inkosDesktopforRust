//! R47/1640 号：全局搜索排序 golden 断言（双端 BM25 命中序一致性的 TS 锚面）。
//!
//! 唯一事实源 = `golden/search-ranking-vectors.json`（TS 生产函数导出）；
//! Rust 侧 `engine-rs/tests/golden_search_ranking_diff.rs` 对拍同一文件——
//! 双端 LocalSearchIndex（FTS5 bm25 5.0/1.0 权重）同输入同命中序。
//! 分数本身不进向量（ICU 版本漂移容忍面），序与命中集=契约面。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { LocalSearchIndex } from "../retrieval/local-search.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/search-ranking-vectors.json"), "utf-8"),
) as {
  vectors: Array<{
    name: string;
    documents: Array<{ id: string; title: string; body: string }>;
    query: string;
    limit: number;
    expectedIds: string[];
  }>;
};

describe("LocalSearchIndex.search 命中序 golden（R47/1640 号）", () => {
  it.each(vectors.vectors.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    const index = new LocalSearchIndex(":memory:");
    try {
      index.replaceScope(
        "golden:ranking",
        vector.documents.map((document) => ({
          id: document.id,
          scope: "golden:ranking",
          kind: "chapter",
          source: `${document.id}.md`,
          title: document.title,
          body: document.body,
        })),
      );
      const ids = index
        .search(vector.query, { scope: "golden:ranking", limit: vector.limit })
        .map((hit) => hit.id);
      expect(ids).toEqual(vector.expectedIds);
    } finally {
      index.close();
    }
  });

  it("golden 非空防空转", () => {
    expect(vectors.vectors.length).toBeGreaterThan(0);
  });
});
