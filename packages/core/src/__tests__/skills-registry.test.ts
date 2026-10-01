import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { createSkillRegistry } from "../skills/registry.js";
import type { AgentSkill } from "../skills/types.js";

/**
 * R44/566 号 skills 注册表 rank 决胜双端共享 golden 回放：与 Rust 侧
 * `engine-rs/tests/golden_skills_registry_diff.rs` 消费同一份向量——
 * 同 id 决胜=有效 rank 升序（显式 rank 优先于装载层缺省表）、平秩后写胜、
 * rank 缺省退化为纯后写胜（R44 前行为兼容面）。改向量必须双端同批。
 */

const VECTORS: {
  version: number;
  cases: Array<{
    name: string;
    skills: Array<{ id: string; rank: number | null }>;
    expected: { ids: string[]; winnerRanks: Record<string, number | null> };
  }>;
} = JSON.parse(
  readFileSync(new URL("./golden/skills-registry-vectors.json", import.meta.url), "utf8"),
);

function makeSkill(id: string, rank: number | null): AgentSkill {
  return {
    id,
    name: id,
    description: `d-${id}`,
    body: "",
    source: "project",
    ...(rank === null ? {} : { rank }),
  };
}

describe("skills registry rank resolution shared golden vectors (R44 / 566)", () => {
  it("loads the shared vector file", () => {
    expect(VECTORS.version).toBe(1);
    expect(VECTORS.cases.length).toBeGreaterThan(0);
  });

  for (const testCase of VECTORS.cases) {
    it(`case: ${testCase.name}`, () => {
      const registry = createSkillRegistry({
        skills: testCase.skills.map((skill) => makeSkill(skill.id, skill.rank)),
      });
      const skills = registry.listSkills();
      expect(skills.map((skill) => skill.id)).toEqual(testCase.expected.ids);
      const winnerRanks: Record<string, number | null> = {};
      for (const skill of skills) {
        winnerRanks[skill.id] = skill.rank ?? null;
      }
      expect(winnerRanks).toEqual(testCase.expected.winnerRanks);
    });
  }
});
