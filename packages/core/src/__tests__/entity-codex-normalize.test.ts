//! 518 号：消费面卡归一化断言（差分器实证缺陷的回归锚）。
//!
//! 背景：PUT codex 端点双端均 Value 原样透传，磁盘可存缺 `aliases` 的卡；
//! Rust 消费面 `#[serde(default)]` 容差，TS 消费面此前原样信任——
//! `matchCodexCards` 展开 `...card.aliases` 即崩（resync 500）。
//! `normalizeCodexCards` = Rust serde(default) 的 TS 对应物：
//! 补缺省数组/字符串、kind 缺省归 other（不校验取值）、未知字段保留、坏卡跳过。
import { describe, expect, it } from "vitest";
import { matchCodexCards, normalizeCodexCards } from "../utils/entity-codex.js";

describe("normalizeCodexCards（518 号）", () => {
  it("缺 aliases/facts/relationships 的卡补空数组——matchCodexCards 不再崩", () => {
    const cards = normalizeCodexCards([
      { id: "card_wen", kind: "character", name: "苏檀", summary: "镜宗外门弟子。", facts: ["佩带碎镜"] },
    ]);
    expect(cards).toHaveLength(1);
    expect(cards[0]).toMatchObject({
      name: "苏檀",
      aliases: [],
      facts: ["佩带碎镜"],
      relationships: [],
      kind: "character",
    });
    // 未知字段（Rust 回显的 id）零丢失。
    expect((cards[0] as unknown as Record<string, unknown>).id).toBe("card_wen");
    // 缺 aliases 的卡可安全走场景命中（修复前此处 TypeError）。
    expect(matchCodexCards("苏檀佩带碎镜", cards)).toHaveLength(1);
  });

  it("kind 缺省归 other；非字符串标量字段归默认；坏卡（无名）跳过", () => {
    const cards = normalizeCodexCards([
      { name: "无 kind 卡" },
      { name: "坏 aliases", aliases: "林小哥", facts: [1, "正典"] },
      { name: "" },
      null,
      "str",
    ]);
    expect(cards).toHaveLength(2);
    expect(cards[0]).toMatchObject({ name: "无 kind 卡", kind: "other", aliases: [] });
    expect(cards[1]).toMatchObject({ name: "坏 aliases", aliases: [], facts: ["正典"] });
  });

  it("类型完备的卡原样通过（firstChapter 等可选字段保留）", () => {
    const cards = normalizeCodexCards([
      {
        name: "齐全",
        aliases: ["别名"],
        kind: "person",
        summary: "s",
        facts: [],
        relationships: [{ target: "另", note: "敌" }],
        firstChapter: 2,
      },
    ]);
    expect(cards[0]).toMatchObject({ aliases: ["别名"], firstChapter: 2 });
    expect(cards[0]!.relationships).toEqual([{ target: "另", note: "敌" }]);
  });
});
