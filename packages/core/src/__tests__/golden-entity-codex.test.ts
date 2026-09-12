//! R10/375 号：实体卡 Codex 化 golden 断言。
//!
//! 唯一事实源 = `golden/entity-codex-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_entity_codex_diff.rs` 读同一文件差分。
//! 四组断言：名册同源派生（排序/置空）、卡片修订（扩展字段/截断）、
//! 场景命中（别名计数/排序/零命中省略）、注入渲染（双语/空）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  deriveCodexCards,
  matchCodexCards,
  renderCodexBlock,
  reviseCodexCard,
  type EntityCodexCard,
} from "../utils/entity-codex.js";
import type { RosterEntity } from "../utils/entity-roster.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/entity-codex-vectors.json"), "utf-8"),
) as {
  derive: Array<{ name: string; roster: ReadonlyArray<RosterEntity>; expected: EntityCodexCard[] }>;
  revise: Array<{
    name: string;
    base: EntityCodexCard;
    revision: Partial<Pick<EntityCodexCard, "summary" | "facts" | "relationships">>;
    expected: EntityCodexCard;
  }>;
  match: Array<{
    name: string;
    text: string;
    cards: EntityCodexCard[];
    expected: Array<{ name: string; hits: number }>;
  }>;
  render: Array<{
    name: string;
    language: "zh" | "en";
    matches: Array<{ card: EntityCodexCard; hits: number }>;
    expected: string | null;
  }>;
};

describe("entity codex contract (R10)", () => {
  it("derives cards from roster per shared vectors", () => {
    for (const vector of vectors.derive) {
      expect(deriveCodexCards(vector.roster), vector.name).toEqual(vector.expected);
    }
  });

  it("revises cards per shared vectors", () => {
    for (const vector of vectors.revise) {
      expect(reviseCodexCard(vector.base, vector.revision), vector.name).toEqual(vector.expected);
    }
  });

  it("matches scene text per shared vectors", () => {
    for (const vector of vectors.match) {
      const got = matchCodexCards(vector.text, vector.cards);
      expect(
        got.map((match) => ({ name: match.card.name, hits: match.hits })),
        vector.name,
      ).toEqual(vector.expected);
    }
  });

  it("renders codex blocks per shared vectors", () => {
    for (const vector of vectors.render) {
      const got = renderCodexBlock(vector.matches as never, vector.language);
      expect(got ?? null, vector.name).toEqual(vector.expected);
    }
  });
});
