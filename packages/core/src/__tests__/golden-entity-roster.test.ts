//! G7b/342 号：实体名册+新专名确认卡 golden 断言（Phase B 批次二末项）。
//!
//! 唯一事实源 = `golden/entity-roster-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_entity_roster_diff.rs` 读同一文件差分。
//! 六组断言：名册解析、渲染、候选提取、三选比对、确认卡渲染、契约形状。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  extractCharacterCandidates,
  parseEntityRoster,
  renderEntityRoster,
  renderRosterConfirmationCard,
  applyRosterConfirmation,
  resolveRosterCandidates,
  type RosterCandidateAction,
  type RosterEntity,
} from "../utils/entity-roster.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/entity-roster-vectors.json"), "utf-8"),
) as {
  parse: Array<{ name: string; input: string; expected: RosterEntity[] }>;
  render: Array<{ name: string; input: RosterEntity[]; expected: string }>;
  candidates: Array<{
    name: string;
    input: Array<{ chapter: number; characters: string }>;
    expected: string[];
  }>;
  resolve: Array<{
    name: string;
    input: { candidates: string[]; roster: RosterEntity[] };
    expected: Array<{ candidate: string; action: string; targetName?: string; confidence: string }>;
  }>;
  cardRender: Array<{
    name: string;
    input: { cards: Array<Record<string, unknown>>; language: "zh" | "en" };
    expected: string;
  }>;
  confirm: Array<{
    name: string;
    input: { roster: RosterEntity[]; confirmation: { candidate: string; action: RosterCandidateAction; targetName?: string; chapter?: number } };
    applied: Record<string, unknown>;
    expectedRoster: RosterEntity[];
  }>;
  contract: unknown;
};

describe("entity roster (G7b)", () => {
  it("parses author-editable roster markdown per shared vectors", () => {
    for (const vector of vectors.parse) {
      expect(parseEntityRoster(vector.input), vector.name).toEqual(vector.expected);
    }
  });

  it("renders the roster round-trip per shared vectors", () => {
    for (const vector of vectors.render) {
      expect(renderEntityRoster(vector.input), vector.name).toBe(vector.expected);
    }
  });

  it("extracts character candidates with dedupe and stopwords", () => {
    for (const vector of vectors.candidates) {
      expect(extractCharacterCandidates(vector.input), vector.name).toEqual(vector.expected);
    }
  });

  it("resolves three-way confirmation cards per shared vectors", () => {
    for (const vector of vectors.resolve) {
      const got = resolveRosterCandidates(vector.input.candidates, vector.input.roster);
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("renders confirmation cards per shared vectors", () => {
    for (const vector of vectors.cardRender) {
      const got = renderRosterConfirmationCard(
        vector.input.cards as never,
        vector.input.language,
      );
      expect(got, vector.name).toBe(vector.expected);
    }
  });

  it("applies confirmations per shared vectors", () => {
    for (const vector of vectors.confirm) {
      const got = applyRosterConfirmation(vector.input.roster, vector.input.confirmation);
      expect(got.roster, vector.name).toEqual(vector.expectedRoster);
      expect(got.applied, vector.name).toEqual(vector.applied);
    }
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual({
      actions: ["new-entity", "alias", "typo"],
      kinds: ["person", "place", "faction", "item", "other"],
      truthFile: "story/entity_roster.md",
      typoMaxDistance: 1,
      containmentMinChars: 2,
    });
  });
});
