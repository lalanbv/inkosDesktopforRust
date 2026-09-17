//! 521 号：搜索分词器 golden 断言（双端 token 序列一致性的 TS 锚面）。
//!
//! 唯一事实源 = `golden/search-tokenizer-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_search_tokenizer_diff.rs` 读同一文件差分。
//! 背景：Rust 分词自 521 号起换 icu_segmenter（与 TS Intl.Segmenter 同源
//! ICU 词典分词），token 序列一致是双端 BM25 打分可比的前提
//!（差分器实证：分词漂移 → tf/df 与长度归一化全漂移 → hybrid-search
//! 边缘命中抖动）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { tokenizeSearchText } from "../retrieval/local-search.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/search-tokenizer-vectors.json"), "utf-8"),
) as { vectors: Array<{ input: string; tokens: string[] }> };

describe("tokenizeSearchText golden（521 号）", () => {
  it.each(vectors.vectors.map((v) => [v.input, v.tokens] as const))(
    "%s",
    (input, expected) => {
      expect(tokenizeSearchText(input)).toEqual(expected);
    },
  );

  it("中文词典词整词切出、相邻单字 bigram 追加、连字复合词追加并存", () => {
    const tokens = tokenizeSearchText("镜中醒来：苏檀坠入镜界，通过外门试炼。");
    expect(tokens).toContain("试炼");
    expect(tokens).toContain("外门");
    expect(tokens).toContain("苏檀");
  });
});
