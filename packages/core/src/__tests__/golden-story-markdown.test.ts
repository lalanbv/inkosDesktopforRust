import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { renderHookSnapshot, renderSummarySnapshot } from "../utils/story-markdown.js";
import type { StoredHook } from "../utils/promise-ledger.js";

/**
 * 715 号：story-markdown 双端 golden 对照——直接消费 Rust 差分测试同一向量
 * 文件（engine-rs/tests/golden/utils/leaf.json，golden_leaf.rs
 * render_hook_snapshot_matches_ts / render_summary_snapshot_matches_ts 的
 * 断言源）。TS 侧恢复移植（fork 逐字）后以同向量反向断言：TS 实现 ==
 * Rust 已验证期望。render_hook_snapshot 的回收节奏列经 hook-lifecycle
 * resolveHookPayoffTiming 推导——同一向量同时活体覆盖 hook-lifecycle 链。
 */
interface GoldenCase {
  readonly name: string;
  readonly input: { readonly hooks?: unknown; readonly summaries?: unknown; readonly language?: string };
  readonly expected: string;
}

const leaf = JSON.parse(
  readFileSync(join(import.meta.dirname ?? ".", "../../../../engine-rs/tests/golden/utils/leaf.json"), "utf-8"),
) as Record<string, ReadonlyArray<GoldenCase>>;

const lang = (c: GoldenCase): "zh" | "en" => (c.input.language === "en" ? "en" : "zh");

describe("story-markdown golden leaf vectors (715)", () => {
  it("renderHookSnapshot 与 Rust render_hook_snapshot 向量逐字节一致", () => {
    for (const c of leaf.render_hook_snapshot) {
      const hooks = c.input.hooks as ReadonlyArray<StoredHook>;
      expect(renderHookSnapshot(hooks, lang(c)), `case \`${c.name}\``).toBe(c.expected);
    }
  });

  it("renderSummarySnapshot 与 Rust render_summary_snapshot 向量逐字节一致（含缺张力列空单元格）", () => {
    for (const c of leaf.render_summary_snapshot) {
      const summaries = c.input.summaries as Parameters<typeof renderSummarySnapshot>[0];
      expect(renderSummarySnapshot(summaries, lang(c)), `case \`${c.name}\``).toBe(c.expected);
    }
  });
});
