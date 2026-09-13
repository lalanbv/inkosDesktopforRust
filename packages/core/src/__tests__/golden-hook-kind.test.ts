//! R23/393 号：伏笔类型标注共享 golden 断言。
//!
//! 唯一事实源 = `golden/hook-kind-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_hook_kind_diff.rs` 读同一文件差分。
//! 三组断言：别名归一化（7 规范 id + zh/en 别名 + 未知 null）、
//! 双语标签（7×2）、HookRecord.kind 零迁移兼容（带/不带/非法）。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { HookRecordSchema, type HookKind } from "../models/runtime-state.js";
import { hookKindLabel, normalizeHookKind } from "../utils/hook-kind.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/hook-kind-vectors.json"), "utf-8"),
) as {
  normalize: Array<{ name: string; raw: string; expected: string | null }>;
  labels: Array<{ name: string; kind: string; language: "zh" | "en"; expected: string }>;
  recordRoundtrip: Array<{
    name: string;
    input: unknown;
    parses: boolean;
    kind: string | null;
  }>;
};

describe("hook kind contract (R23)", () => {
  it("normalizes free text per shared vectors", () => {
    for (const vector of vectors.normalize) {
      const got = normalizeHookKind(vector.raw);
      expect(got ?? null, vector.name).toBe(vector.expected);
    }
  });

  it("renders bilingual labels per shared vectors", () => {
    for (const vector of vectors.labels) {
      const got = hookKindLabel(vector.kind as HookKind, vector.language);
      expect(got, vector.name).toBe(vector.expected);
    }
  });

  it("keeps HookRecord.kind optional per shared vectors (zero migration)", () => {
    for (const vector of vectors.recordRoundtrip) {
      const result = HookRecordSchema.safeParse(vector.input);
      expect(result.success, vector.name).toBe(vector.parses);
      if (result.success) {
        expect(result.data.kind ?? null, vector.name).toBe(vector.kind);
      }
    }
  });
});
