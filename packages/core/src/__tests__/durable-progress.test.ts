import { afterEach, describe, expect, it } from "vitest";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";

import { resolveDurableStoryProgress } from "../state/state-bootstrap.js";
import { StateManager } from "../state/manager.js";

// =============================================================================
// 789 号：durable 进度解析容忍降级——788 号登记项清偿。旧行为 index.json 缺失
// 即返回 0 → index-less legacy 书 getNextChapterNumber=1 → write-next 覆盖
// 既有第 1 章。新行为：index 缺失/损坏 → 文件名 `N_` 前缀连续前缀（durable
// 产物链权威=章节文件本身，Rust load_durable_artifact_chapter_numbers 镜像）；
// index 有效时保留 2.0 一致性守卫（不一致即抛，比 Rust 更严的刻意不对称）。
// =============================================================================
describe("789 durable progress tolerant fallback", () => {
  const roots: string[] = [];

  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  async function tempBook(): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-durable-progress-"));
    roots.push(root);
    return root;
  }

  async function seedChapters(bookDir: string, count: number): Promise<void> {
    await mkdir(join(bookDir, "chapters"), { recursive: true });
    for (let i = 1; i <= count; i += 1) {
      await writeFile(join(bookDir, "chapters", `${String(i).padStart(4, "0")}_第${i}章.md`), `# 第${i}章\n\n正文\n`);
    }
  }

  async function writeIndex(bookDir: string, numbers: number[]): Promise<void> {
    await writeFile(
      join(bookDir, "chapters", "index.json"),
      JSON.stringify(
        numbers.map((number) => ({
          number,
          title: `第${number}章`,
          wordCount: 2,
          createdAt: "2026-11-01T00:00:00.000Z",
          updatedAt: "2026-11-01T00:00:00.000Z",
          observations: [],
          provenance: "generated",
        })),
      ),
    );
  }

  it("fresh book without chapters dir returns 0 (unchanged)", async () => {
    const bookDir = await tempBook();
    await expect(resolveDurableStoryProgress({ bookDir })).resolves.toBe(0);
  });

  it("index-less legacy book falls back to file-name prefix (was 0)", async () => {
    const bookDir = await tempBook();
    await seedChapters(bookDir, 2);
    await expect(resolveDurableStoryProgress({ bookDir })).resolves.toBe(2);
  });

  it("corrupt index.json falls back to file-name prefix (was throw)", async () => {
    const bookDir = await tempBook();
    await seedChapters(bookDir, 2);
    await writeFile(join(bookDir, "chapters", "index.json"), "{broken");
    await expect(resolveDurableStoryProgress({ bookDir })).resolves.toBe(2);
  });

  it("non-contiguous files yield contiguous prefix (1,3 → 1)", async () => {
    const bookDir = await tempBook();
    await seedChapters(bookDir, 3);
    await rm(join(bookDir, "chapters", "0002_第2章.md"));
    await expect(resolveDurableStoryProgress({ bookDir })).resolves.toBe(1);
  });

  it("valid index agreeing with files still resolves (unchanged)", async () => {
    const bookDir = await tempBook();
    await seedChapters(bookDir, 2);
    await writeIndex(bookDir, [1, 2]);
    await expect(resolveDurableStoryProgress({ bookDir })).resolves.toBe(2);
  });

  it("valid index disagreeing with files still throws (guard kept)", async () => {
    const bookDir = await tempBook();
    await seedChapters(bookDir, 2);
    await writeIndex(bookDir, [1]); // 文件 2 不在 index → orphaned
    await expect(resolveDurableStoryProgress({ bookDir })).rejects.toThrow(/disagree/);
  });

  it("write flow probe: getNextChapterNumber on index-less legacy book returns file-prefix+1 (was 1)", async () => {
    // 写流安全证明：旧行为此处返回 1，write-next 会以「第 1 章」覆盖既有产物。
    // StateManager.bookDir = workDirectory(root, id)/source（works 布局），播种其下。
    const root = await tempBook();
    const bookDir = join(root, "works", "b1", "source");
    await seedChapters(bookDir, 2);
    const manager = new StateManager(root);
    await expect(manager.getNextChapterNumber("b1")).resolves.toBe(3);
  });
});

// =============================================================================
// 791 号：loadChapterIndex 容忍重建（Rust rebuild 镜像）——legacy 簇读路径收尾。
// 旧行为：index 缺失+有文件 / 损坏 / 非数组 / 条目 schema 不符 → throw
// （Node 回退腿 bookDetail 500）；Rust 同场景 = 从文件重建内存返回 → 200。
// =============================================================================
describe("791 chapter index tolerant rebuild", () => {
  const roots: string[] = [];

  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  async function tempManager(): Promise<{ manager: StateManager; bookDir: string }> {
    const root = await mkdtemp(join(tmpdir(), "inkos-index-rebuild-"));
    roots.push(root);
    const bookDir = join(root, "works", "b1", "source");
    await mkdir(join(bookDir, "chapters"), { recursive: true });
    return { manager: new StateManager(root), bookDir };
  }

  it("index-less book rebuilds rows from chapter files (was throw)", async () => {
    const { manager, bookDir } = await tempManager();
    await writeFile(join(bookDir, "chapters", "0002_推进.md"), "# 第2章\n\n正文两字\n");
    await writeFile(join(bookDir, "chapters", "0001_风起.md"), "# 第1章\n\n正文\n");
    const rows = await manager.loadChapterIndex("b1");
    expect(rows.map((row) => [row.number, row.title])).toEqual([
      [1, "风起"],
      [2, "推进"],
    ]);
    expect(rows[0]).toMatchObject({
      status: "ready-for-review",
      // 792 号：行键与 Rust rebuild 行对齐——auditIssues/lengthWarnings 恒在，
      // observations/provenance 不发（Rust ChapterMeta 无此二字段）。
      auditIssues: [],
      lengthWarnings: [],
      // 词数口径=去全部空白后的码元数（含标题行：#第1章正文）。
      wordCount: "#第1章正文".length,
    });
    // 仅内存重建：index.json 不被写回（Rust 同款，写回走显式 saveChapterIndex）。
    await expect(readFile(join(bookDir, "chapters", "index.json"), "utf-8")).rejects.toThrow();
  });

  it("corrupt index rebuilds from files (was throw)", async () => {
    const { manager, bookDir } = await tempManager();
    await writeFile(join(bookDir, "chapters", "0001_风起.md"), "# 第1章\n\n正文\n");
    await writeFile(join(bookDir, "chapters", "index.json"), "{broken");
    const rows = await manager.loadChapterIndex("b1");
    expect(rows.map((row) => row.number)).toEqual([1]);
  });

  it("non-array index rebuilds (was throw)", async () => {
    const { manager, bookDir } = await tempManager();
    await writeFile(join(bookDir, "chapters", "0001_风起.md"), "# 第1章\n\n正文\n");
    await writeFile(join(bookDir, "chapters", "index.json"), '{"number":1}');
    const rows = await manager.loadChapterIndex("b1");
    expect(rows.map((row) => row.number)).toEqual([1]);
  });

  it("valid index returns as-is (unchanged)", async () => {
    const { manager, bookDir } = await tempManager();
    await writeFile(
      join(bookDir, "chapters", "index.json"),
      JSON.stringify([{
        number: 7,
        title: "定制标题",
        wordCount: 10,
        createdAt: "2026-11-01T00:00:00.000Z",
        updatedAt: "2026-11-01T00:00:00.000Z",
        observations: [],
        provenance: "generated",
      }]),
    );
    const rows = await manager.loadChapterIndex("b1");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ number: 7, title: "定制标题" });
  });

  it("title rules mirror Rust title_from_filename", async () => {
    const { manager, bookDir } = await tempManager();
    await writeFile(join(bookDir, "chapters", "0003_夜_探.md"), "x");
    await writeFile(join(bookDir, "chapters", "0004.md"), "x");
    await writeFile(join(bookDir, "chapters", "0005__暗线.md"), "x");
    const rows = await manager.loadChapterIndex("b1");
    const titles = new Map(rows.map((row) => [row.number, row.title]));
    expect(titles.get(3)).toBe("夜 探");
    expect(titles.get(4)).toBe("第4章");
    expect(titles.get(5)).toBe("暗线");
  });

  it("empty book (no chapters dir) returns empty (unchanged)", async () => {
    const { manager } = await tempManager();
    await expect(manager.loadChapterIndex("b1")).resolves.toEqual([]);
  });
});
