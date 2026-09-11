//! G2/330 号：上下文来源优先级契约 golden 断言。
//!
//! 唯一事实源 = `golden/context-priority-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_context_priority_diff.rs` 读同一文件差分。
//! 三组断言：层级分类、组装固化排序（核心：参考资料不得覆盖章纲段）、
//! 契约形状 + 与 isProtectedContextSource 的一致性。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  CONTEXT_SOURCE_PRIORITY_CONTRACT,
  CONTEXT_SOURCE_TIERS,
  PROTECTED_CONTEXT_SOURCE_TIERS,
  contextSourceTier,
  contextSourceTierPrecedence,
  enforceContextPriorityOrder,
} from "../utils/context-source-tier.js";
import { isProtectedContextSource } from "../utils/context-assembly.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/context-priority-vectors.json"), "utf-8"),
) as {
  tier: Array<{ name: string; source: string; expected: string }>;
  order: Array<{
    name: string;
    input: Array<{ source: string; reason: string }>;
    expected: string[];
  }>;
  contract: unknown;
};

describe("context source priority contract (G2)", () => {
  it("classifies every registered source family per shared vectors", () => {
    for (const vector of vectors.tier) {
      expect(contextSourceTier(vector.source), vector.name).toBe(vector.expected);
    }
  });

  it("enforces priority order — references never precede outline/planning", () => {
    for (const vector of vectors.order) {
      const got = enforceContextPriorityOrder(vector.input).map((entry) => entry.source);
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("keeps reference entries after chapter-outline entries for any interleaving", () => {
    // G2 核心不变量的性质化校验：无论参考资料混入哪个位置，
    // 排序后参考资料必须排在 story_frame / volume_map 之后。
    const outline = "story/outline/volume_map.md";
    const reference = "reference/mat-01#开场";
    const filler = ["story/current_state.md", "story/chapter_summaries.md#3", "runtime/chapter_memo"];
    for (let slot = 0; slot <= filler.length; slot++) {
      const input = [...filler];
      input.splice(slot, 0, reference);
      const ordered = enforceContextPriorityOrder(
        input.map((source) => ({ source, reason: "x" })),
      ).map((entry) => entry.source);
      expect(ordered.indexOf(reference)).toBeGreaterThan(ordered.indexOf(outline));
    }
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual(CONTEXT_SOURCE_PRIORITY_CONTRACT);
    // 层级优先级严格递减，保证排序语义与声明一致。
    for (let i = 1; i < CONTEXT_SOURCE_TIERS.length; i++) {
      expect(CONTEXT_SOURCE_TIERS[i]!.precedence)
        .toBeLessThan(CONTEXT_SOURCE_TIERS[i - 1]!.precedence);
    }
  });
  it("keeps tier protection consistent with isProtectedContextSource", () => {
    // 两条保护契约必须同进同退：tier.protected === isProtectedContextSource。
    for (const vector of vectors.tier) {
      const tier = contextSourceTier(vector.source);
      expect(isProtectedContextSource(vector.source), `${vector.name} (${tier})`)
        .toBe(PROTECTED_CONTEXT_SOURCE_TIERS.has(tier));
    }
    // 兜底：已知保护族全覆盖 + 已知可压缩族不误保护。
    for (const source of ["story/outline/story_frame.md#卷一", "story/volume_outline.md"]) {
      expect(contextSourceTierPrecedence(contextSourceTier(source))).toBeGreaterThanOrEqual(80);
      expect(isProtectedContextSource(source)).toBe(true);
    }
  });
});
