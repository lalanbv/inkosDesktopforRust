import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { StateManager } from "../state/manager.js";
import { bootstrapStructuredStateFromMarkdown } from "../state/state-bootstrap.js";

// 1678 号：durable 进度对「章文件」的认定，要和索引重建、落盘计数是同一个集合
// （正典 `NNNN_标题.md` / legacy `NNNN-标题.md` / 裸 `NNNN.md`——807/813/845 号原则）。
//
// 此前 resolveDurableStoryProgress 只认正典形态：
// - 索引里列着旧命名的章 → 一致性守卫把这些章当成「缺文件」抛错，
//   getNextChapterNumber 失败，书架接口（GET /api/v1/books）整页 500；
// - 没有索引 → 旧命名的章不算数，进度为 0，下一章号回到 1（789 号要防的「重写第 1 章」，
//   当时只修了正典命名）。

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function tempRoot(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "inkos-durable-forms-"));
  roots.push(root);
  return root;
}

const STAMP = "2026-11-25T00:00:00.000Z";

/** 建一本书：给定的章文件（原样写进 chapters/），可选的章节索引。返回书目录。 */
async function seedBook(root: string, files: ReadonlyArray<string>, index?: ReadonlyArray<number>): Promise<string> {
  const book = new StateManager(root).bookDir("legacy");
  await mkdir(join(book, "chapters"), { recursive: true });
  for (const file of files) {
    await writeFile(join(book, "chapters", file), `# ${file}\n\n正文。`);
  }
  if (index) {
    await writeFile(
      join(book, "chapters", "index.json"),
      JSON.stringify(index.map((number) => ({
        number, title: `第${number}章`, status: "ready-for-review", wordCount: 2,
        createdAt: STAMP, updatedAt: STAMP, auditIssues: [], lengthWarnings: [],
      }))),
    );
  }
  return book;
}

/** 三种命名形态各一章。 */
const MIXED_FORMS = ["0001-镜中醒来.md", "0002.md", "0003_云涌.md"];

describe("durable story progress and chapter file forms", () => {
  it("follows chapters saved under legacy names when the index lists them", async () => {
    const root = await tempRoot();
    await seedBook(root, MIXED_FORMS, [1, 2, 3]);
    await expect(new StateManager(root).getNextChapterNumber("legacy")).resolves.toBe(4);
  });

  it("follows legacy-named chapters when there is no index — the same chapters the index rebuild lists", async () => {
    const root = await tempRoot();
    await seedBook(root, MIXED_FORMS);
    const manager = new StateManager(root);
    expect((await manager.loadChapterIndex("legacy")).map((meta) => meta.number)).toEqual([1, 2, 3]);
    await expect(manager.getNextChapterNumber("legacy")).resolves.toBe(4);
  });

  it("does not let leftovers from old rollbacks block the book", async () => {
    // 807 号之前的回滚删不掉旧命名的章：索引已经是 [1, 2]，盘上还留着 `0003-旧.md`。
    // 它挡不住正典章（读侧正典优先），也不该让整本书打不开。
    const root = await tempRoot();
    await seedBook(root, ["0001_风起.md", "0002_云涌.md", "0003-旧.md"], [1, 2]);
    await expect(new StateManager(root).getNextChapterNumber("legacy")).resolves.toBe(3);
  });

  it("still fails closed when an indexed chapter has no file in any form", async () => {
    const root = await tempRoot();
    await seedBook(root, ["0001_风起.md"], [1, 2]);
    await expect(new StateManager(root).getNextChapterNumber("legacy")).rejects.toThrow(/missing=2/u);
  });

  it("still fails closed when a canonical file is not in the index", async () => {
    const root = await tempRoot();
    await seedBook(root, ["0001_风起.md", "0002_云涌.md"], [1]);
    await expect(new StateManager(root).getNextChapterNumber("legacy")).rejects.toThrow(/orphaned=2/u);
  });

  it("ignores markdown that is not a chapter file", async () => {
    const root = await tempRoot();
    await seedBook(root, ["0001_风起.md", "notes.md", "设定-素材.md"]);
    await expect(new StateManager(root).getNextChapterNumber("legacy")).resolves.toBe(2);
  });
});

describe("state bootstrap with legacy-named chapters", () => {
  it("derives the current chapter from the chapters on disk when no state exists", async () => {
    const root = await tempRoot();
    const book = await seedBook(root, MIXED_FORMS);
    // 没有 story/*.md、没有结构化状态：当前章节只能来自盘上的章。
    const snapshot = await bootstrapStructuredStateFromMarkdown({ bookDir: book });
    expect(snapshot.currentState.chapter).toBe(3);
    expect(snapshot.manifest.lastAppliedChapter).toBe(3);
  });
});
