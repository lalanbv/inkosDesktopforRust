//! R5/365 号：反AI规则资产 + G13 经验条目 golden 断言。
//!
//! 唯一事实源 = `golden/rule-experience-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_rule_experience_diff.rs` 读同一文件差分。
//! 五组断言：规则校验（合法/坏id/坏regex）、扫描（字面计数/ex regex/
//! 排序+disabled 跳过）、修复块（双语+replacement/空命中）、写作预防块
//!（enabled 过滤+replacement+maxChars）、经验合并与渲染（去重/排序/上限/截断）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  ANTI_AI_RULE_SEEDS,
  antiAiRuleCanonicalForm,
  antiAiRuleContentHash,
  antiAiRulePackHash,
  composeAntiAiGuidance,
  mergeExperienceEntries,
  renderAntiAiFixGuidance,
  renderExperienceGuidance,
  resolveAntiAiRulesWithSeeds,
  scanAntiAiRules,
  validateAntiAiRule,
  type AntiAiHit,
  type AntiAiRule,
  type ExperienceEntry,
} from "../utils/rule-experience-engine.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/rule-experience-vectors.json"), "utf-8"),
) as {
  validate: Array<{ name: string; input: unknown; valid: boolean; errorField?: string }>;
  scan: Array<{
    name: string;
    text: string;
    rules: Array<Record<string, unknown>>;
    expected: Array<Record<string, unknown>>;
  }>;
  fixGuidance: Array<{
    name: string;
    language: "zh" | "en";
    hits: Array<Record<string, unknown>>;
    expected: string | null;
  }>;
  antiAiGuidance: Array<{
    name: string;
    language: "zh" | "en";
    rules: Array<Record<string, unknown>>;
    maxChars: number | null;
    expected: string;
  }>;
  experience: Array<
    | {
      name: string;
      existing: Array<Record<string, unknown>>;
      incoming: Array<Record<string, unknown>>;
      expectedIds: string[];
    }
    | {
      name: string;
      language: "zh" | "en";
      entries: Array<Record<string, unknown>>;
      maxChars: number | null;
      expected: string;
    }
  >;
  seeds: { count: number; ids: string[]; typesCovered: string[]; packHash: string };
  seedCanonical: Array<{ name: string; ruleId: string; expected: string }>;
  seedHash: Array<{ name: string; ruleId: string; expected: string }>;
  seedFallback: Array<{
    name: string;
    parsed: Array<Record<string, unknown>> | null;
    seeded: boolean;
    ruleCount: number;
  }>;
};

function asRule(raw: Record<string, unknown>): AntiAiRule {
  const result = validateAntiAiRule(raw);
  if (!result.rule) throw new Error(`golden rule should be valid: ${result.errors.join("; ")}`);
  return result.rule;
}

function asEntry(raw: Record<string, unknown>): ExperienceEntry {
  return raw as unknown as ExperienceEntry;
}

describe("anti-AI rules and experience contract (R5/G13)", () => {
  it("validates rules per shared vectors", () => {
    for (const vector of vectors.validate) {
      const result = validateAntiAiRule(vector.input);
      expect(result.errors.length === 0, vector.name).toBe(vector.valid);
      if (vector.errorField) {
        expect(
          result.errors.some((message) => message.startsWith(vector.errorField!)),
          vector.name,
        ).toBe(true);
      }
    }
  });

  it("scans text per shared vectors", () => {
    for (const vector of vectors.scan) {
      const got = scanAntiAiRules(vector.text, vector.rules.map(asRule));
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("renders fix guidance per shared vectors", () => {
    for (const vector of vectors.fixGuidance) {
      const got = renderAntiAiFixGuidance(
        vector.hits as unknown as AntiAiHit[],
        vector.language,
      );
      expect(got ?? null, vector.name).toEqual(vector.expected);
    }
  });

  it("composes prevention guidance per shared vectors", () => {
    for (const vector of vectors.antiAiGuidance) {
      const got = composeAntiAiGuidance(
        vector.rules.map(asRule),
        vector.language,
        vector.maxChars ?? undefined,
      );
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("merges and renders experience entries per shared vectors", () => {
    for (const vector of vectors.experience) {
      if ("existing" in vector) {
        const merged = mergeExperienceEntries(
          vector.existing.map(asEntry),
          vector.incoming.map(asEntry),
        );
        expect(merged.map((entry) => entry.id), vector.name).toEqual(vector.expectedIds);
        continue;
      }
      const got = renderExperienceGuidance(
        vector.entries.map(asEntry),
        vector.language,
        vector.maxChars ?? undefined,
      );
      expect(got, vector.name).toEqual(vector.expected);
    }
  });
});

describe("anti-AI rule seeds contract (R22)", () => {
  const seedById = new Map(ANTI_AI_RULE_SEEDS.map((rule) => [rule.id, rule]));

  it("ships the seed pack matching the shared contract", () => {
    expect(ANTI_AI_RULE_SEEDS).toHaveLength(vectors.seeds.count);
    expect(ANTI_AI_RULE_SEEDS.map((rule) => rule.id)).toEqual(vectors.seeds.ids);
    expect([...new Set(ANTI_AI_RULE_SEEDS.map((rule) => rule.type))].sort()).toEqual(
      vectors.seeds.typesCovered,
    );
    // 每条种子都必须通过自家校验器（种子即合法规则）。
    for (const rule of ANTI_AI_RULE_SEEDS) {
      expect(validateAntiAiRule(rule).errors, rule.id).toEqual([]);
    }
    // 种子包完整性锚点：逐条 canonical 以 \n 连接后的 FNV 指纹（Python 独立计算）。
    expect(antiAiRulePackHash(ANTI_AI_RULE_SEEDS), "pack hash").toBe(vectors.seeds.packHash);
  });

  it("locks seed canonical forms per shared vectors", () => {
    for (const vector of vectors.seedCanonical) {
      const seed = seedById.get(vector.ruleId);
      expect(seed, vector.name).toBeDefined();
      expect(antiAiRuleCanonicalForm(seed!), vector.name).toBe(vector.expected);
    }
  });

  it("locks seed content hashes per shared vectors", () => {
    for (const vector of vectors.seedHash) {
      const seed = seedById.get(vector.ruleId);
      expect(seed, vector.name).toBeDefined();
      expect(antiAiRuleContentHash(seed!), vector.name).toBe(vector.expected);
    }
  });

  it("resolves seed fallback per shared vectors", () => {
    for (const vector of vectors.seedFallback) {
      const parsed = vector.parsed === null ? undefined : vector.parsed.map(asRule);
      const got = resolveAntiAiRulesWithSeeds(parsed);
      expect(got.seeded, vector.name).toBe(vector.seeded);
      expect(got.rules, vector.name).toHaveLength(vector.ruleCount);
      if (vector.parsed === null) {
        expect(got.rules, vector.name).toEqual(ANTI_AI_RULE_SEEDS);
      } else {
        expect(got.rules, vector.name).toEqual(parsed);
      }
    }
  });
});
