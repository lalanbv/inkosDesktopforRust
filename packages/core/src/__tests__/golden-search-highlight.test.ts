//! R47/1640 号：全局搜索高亮裁剪 golden 断言（双端纯函数一致性的 TS 锚面）。
//!
//! 唯一事实源 = `golden/search-highlight-vectors.json`（TS 生产函数导出，
//! 生成器一次性 /tmp 惯例——契约样例须生产派生）；Rust 侧
//! `engine-rs/tests/golden_search_highlight_diff.rs` 读同一文件差分。
//! 偏移口径 = 码点下标（Rust chars().count() 同口径，astral 向量锁定）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { buildHighlightSnippet } from "../retrieval/global-search.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/search-highlight-vectors.json"), "utf-8"),
) as {
  vectors: Array<{
    name: string;
    content: string;
    query: string;
    expected: { text: string; matchStart: number; matchLength: number } | null;
  }>;
};

describe("buildHighlightSnippet golden（R47/1640 号）", () => {
  it.each(vectors.vectors.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    const snippet = buildHighlightSnippet(vector.content, vector.query);
    expect(snippet).toEqual(vector.expected);
  });

  it("golden 非空防空转", () => {
    expect(vectors.vectors.length).toBeGreaterThan(0);
  });
});
