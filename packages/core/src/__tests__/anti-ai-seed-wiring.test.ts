//! R22/392 号：反AI规则种子接线行为断言（与 Rust composer 测试同构）。
//!
//! 语义：文件缺失 → 内置种子条目（seeded reason，内存态不落盘）；
//! 显式空规则 = 用户关闭防线，不回填种子。
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { loadRuleExperienceEntries } from "../agents/composer.js";

describe("anti-AI rule seed wiring (R22)", () => {
  it("falls back to built-in seeds when no rules file exists", async () => {
    const bookDir = await mkdtemp(join(tmpdir(), "inkos-rules-seed-"));
    const entries = await loadRuleExperienceEntries(bookDir);
    const seeded = entries.find((entry) => entry.source === "rules/anti-ai");
    expect(seeded).toBeDefined();
    expect(seeded!.reason).toBe("Built-in anti-AI baseline rules (seeded).");
    expect(seeded!.excerpt).toContain("眼中闪过一丝");
  });

  it("treats explicit empty rules as opting out", async () => {
    const bookDir = await mkdtemp(join(tmpdir(), "inkos-rules-empty-"));
    await mkdir(join(bookDir, "story"), { recursive: true });
    await writeFile(
      join(bookDir, "story", "anti_ai_rules.json"),
      JSON.stringify({ version: 1, rules: [] }),
      "utf-8",
    );
    const entries = await loadRuleExperienceEntries(bookDir);
    expect(entries.find((entry) => entry.source === "rules/anti-ai")).toBeUndefined();
  });
});
