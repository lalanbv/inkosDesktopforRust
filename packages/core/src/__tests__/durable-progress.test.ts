import { afterEach, describe, expect, it } from "vitest";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
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
