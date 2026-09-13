//! R20/390 号：composer 系列正典双层注入行为断言（Rust mirror:
//! `engine-rs/src/agents/composer.rs` tests::load_codex_entries_dual_layer_with_override）。
//! 验证文件级链路：book 卡块 + series 幸存条目块，同名 book 覆盖（覆盖即省略）。
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, expect, it } from "vitest";
import { loadCodexEntries } from "../agents/composer.js";

let dir: string;
beforeAll(async () => {
  dir = await mkdtemp(join(tmpdir(), "inkos-series-canon-"));
});
afterAll(async () => {
  await rm(dir, { recursive: true, force: true });
});

it("injects book and series blocks with override semantics", async () => {
  const bookDir = join(dir, "books", "b1");
  await mkdir(join(bookDir, "story"), { recursive: true });
  await writeFile(
    join(bookDir, "story", "entity_codex.json"),
    JSON.stringify({
      version: 1,
      cards: [
        { name: "林动", aliases: [], kind: "person", summary: "书内卡。", facts: [], relationships: [] },
      ],
    }),
  );
  const seriesFile = join(dir, ".inkos", "series", "wu_dong.json");
  await mkdir(join(dir, ".inkos", "series"), { recursive: true });
  await writeFile(
    seriesFile,
    JSON.stringify({
      version: 1,
      seriesId: "wu_dong",
      entries: [
        { name: "林动", aliases: [], kind: "person", summary: "系列卡同名被覆盖。", facts: [], relationships: [] },
        { name: "祖符石", aliases: ["符石"], kind: "item", summary: "跨书信物。", facts: ["可吸收源气"], relationships: [] },
      ],
    }),
  );
  const entries = await loadCodexEntries(bookDir, 7, "林动摩挲祖符石。", "", seriesFile);
  expect(entries.map((entry) => entry.source)).toEqual(["codex/林动", "codex-series/祖符石"]);
  const bookBlock = entries[0]!.excerpt ?? "";
  expect(bookBlock).toContain("书内卡。");
  expect(bookBlock).not.toContain("系列实体卡");
  const seriesBlock = entries[1]!.excerpt ?? "";
  expect(seriesBlock).toContain("系列实体卡");
  expect(seriesBlock).not.toContain("同名被覆盖");
});

it("series file without book codex still injects series block", async () => {
  const bookDir = join(dir, "books", "b2");
  await mkdir(join(bookDir, "story"), { recursive: true });
  const seriesFile = join(dir, ".inkos", "series", "wu_dong.json");
  const entries = await loadCodexEntries(bookDir, 1, "祖符石发烫。", "", seriesFile);
  expect(entries.map((entry) => entry.source)).toEqual(["codex-series/祖符石"]);
});

it("missing series file degrades to book-only injection", async () => {
  const bookDir = join(dir, "books", "b1");
  const entries = await loadCodexEntries(
    bookDir,
    1,
    "林动攥紧怀表。",
    "",
    join(dir, ".inkos", "series", "absent.json"),
  );
  expect(entries.map((entry) => entry.source)).toEqual(["codex/林动"]);
});
