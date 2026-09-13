//! R21/391 号：Context Lens 共享 golden 断言。
//!
//! 唯一事实源 = `golden/context-lens-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_context_lens_diff.rs` 读同一文件差分。
//! 三组用例：全层级装配（无压缩/rank 透传/空 excerpt/未注册来源）、
//! 预算压缩后实况（编译条目+留痕透传）、冷启动空装配。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { ChapterTraceSchema, ContextPackageSchema } from "../models/input-governance.js";
import { buildContextLens } from "../utils/context-lens.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/context-lens-vectors.json"), "utf-8"),
) as {
  cases: Array<{
    name: string;
    contextPackage: unknown;
    trace: unknown;
    expected: unknown;
  }>;
};

describe("context lens contract (R21)", () => {
  it("projects assembly lenses per shared vectors", () => {
    for (const vector of vectors.cases) {
      const got = buildContextLens({
        contextPackage: ContextPackageSchema.parse(vector.contextPackage),
        trace: ChapterTraceSchema.parse(vector.trace),
      });
      expect(got, vector.name).toEqual(vector.expected);
    }
  });

  it("never trusts trace tier lists for per-entry protection", () => {
    // 保护判定必须独立于留痕：篡改 trace 分层不影响 lens 的 protected/tier。
    const vector = vectors.cases[0]!;
    const tampered = ChapterTraceSchema.parse({
      ...(vector.trace as object),
      contextTiers: { protectedSources: [], compressibleSources: [] },
    });
    const got = buildContextLens({
      contextPackage: ContextPackageSchema.parse(vector.contextPackage),
      trace: tampered,
    });
    expect(got.entries, vector.name).toEqual(
      (vector.expected as { entries: unknown }).entries,
    );
  });
});
