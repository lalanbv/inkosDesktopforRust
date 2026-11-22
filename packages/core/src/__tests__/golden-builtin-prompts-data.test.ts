//! R54/1662 号：builtin prompts 数据单源守卫（TS 环）——JSON（Rust example
//! 导出）↔ BUILTIN_PROMPTS/BUILTIN_PROMPT_PACKS 常量逐字段相等。与 Rust
//! golden_builtin_prompts_diff.rs（JSON↔RAW_BUILTIN_PROMPTS）构成双环。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { BUILTIN_PROMPT_PACKS, BUILTIN_PROMPTS } from "../prompts/builtin-prompts-data.js";

const here = dirname(fileURLToPath(import.meta.url));
const data = JSON.parse(
  readFileSync(resolve(here, "../prompts/builtin-prompts.json"), "utf-8"),
) as { packs: unknown[]; prompts: unknown[] };

describe("builtin prompts 数据单源（R54/1662 号）", () => {
  it("packs：JSON ↔ 常量逐字段相等", () => {
    expect([...BUILTIN_PROMPT_PACKS]).toEqual(data.packs);
  });

  it("prompts：JSON ↔ 常量逐字段相等（12 条）", () => {
    expect(BUILTIN_PROMPTS).toHaveLength(12);
    expect([...BUILTIN_PROMPTS]).toEqual(data.prompts);
  });
});
