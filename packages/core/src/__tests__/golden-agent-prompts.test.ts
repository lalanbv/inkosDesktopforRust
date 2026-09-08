import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { buildGoldenPromptSnapshot } from "../agent/agent-system-prompt.js";

/**
 * 230/231/234 号：聊天面系统提示词 golden 快照（双端守门）。
 * 事实源 = golden/agent-prompts.json；engine 侧
 * `engine-rs/tests/golden_agent_prompts_diff.rs` include_str! 同文件断言。
 * 首次生成：REGEN=1 npx vitest run 本文件。
 */
const goldenPath = join(import.meta.dirname, "golden", "agent-prompts.json");

describe("agent prompts golden snapshot (shared with engine)", () => {
  it("matches the committed snapshot", () => {
    const got = buildGoldenPromptSnapshot("b1");
    if (process.env.REGEN) {
      writeFileSync(goldenPath, `${JSON.stringify(got, null, 2)}\n`);
    }
    const want = JSON.parse(readFileSync(goldenPath, "utf-8")) as Record<string, string>;
    expect(got).toEqual(want);
  });
});
