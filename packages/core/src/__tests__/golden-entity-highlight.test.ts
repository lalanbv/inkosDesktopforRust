//! R52/1656 号：实体命中集扫描 golden 锚面（TS 生产向量）——中英大小写
//! 敏感/别名最长优先/跨卡重叠贪心/排除短语/minLength/astral 码点口径/
//! 空输入。Rust 侧 = engine-rs/tests/golden_entity_highlight_diff.rs 同
//! 文件差分。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { scanEntityHighlights, type EntityCardLike, type EntityHighlightHit } from "../utils/entity-highlight.js";

const here = dirname(fileURLToPath(import.meta.url));
const fixture = JSON.parse(
  readFileSync(resolve(here, "golden/entity-highlight-vectors.json"), "utf-8"),
) as {
  cards: EntityCardLike[];
  vectors: Array<{
    name: string;
    text: string;
    minLength: number;
    exclude: string[];
    expected: EntityHighlightHit[];
  }>;
};

describe("scanEntityHighlights golden（R52/1656 号）", () => {
  it.each(fixture.vectors.map((v) => [v.name, v] as const))("%s", (_name, vector) => {
    expect([
      ...scanEntityHighlights(vector.text, fixture.cards, {
        minLength: vector.minLength,
        exclude: vector.exclude,
      }),
    ]).toEqual(vector.expected);
  });

  it("golden 非空防空转", () => {
    expect(fixture.vectors.length).toBeGreaterThanOrEqual(8);
  });
});
