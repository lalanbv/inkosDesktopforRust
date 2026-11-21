//! R48a/1641 号：章节结构事务单测（commitAtomicFileSet 单提交原子性）。
//! 覆盖：split 拆分/产物迁移/归档、merge trash+前移、reorder 双射、
//! revision 乐观并发 409、负路径（越界/非相邻/非双射）。
//! Rust 镜像单测 = engine-rs/src/interaction/chapter_structure.rs tests。
import { mkdtempSync, mkdirSync, existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  chapterSetRevision,
  mergeChapters,
  reorderChapters,
  splitChapter,
  ChapterStructureError,
  type ChapterStructureDeps,
} from "../state/chapter-structure.js";
import type { ChapterMeta } from "../models/chapter.js";

const dirs: string[] = [];
afterEach(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function seedBook(): { bookDir: string; deps: ChapterStructureDeps } {
  const root = mkdtempSync(join(tmpdir(), "chapter-structure-"));
  dirs.push(root);
  const bookDir = join(root, "works", "b1", "source");
  mkdirSync(join(bookDir, "chapters"), { recursive: true });
  mkdirSync(join(bookDir, "story", "runtime"), { recursive: true });
  mkdirSync(join(bookDir, "story", "drafts"), { recursive: true });
  const rows = [
    { number: 1, title: "风起", body: "第一章正文。" },
    { number: 2, title: "灵脉", body: "第二章正文乙。" },
    { number: 3, title: "收束", body: "第三章正文丙。" },
  ];
  for (const row of rows) {
    writeFileSync(
      join(bookDir, "chapters", `${String(row.number).padStart(4, "0")}_${row.title}.md`),
      `# ${row.title}\n\n${row.body}\n`,
    );
  }
  writeFileSync(join(bookDir, "story", "runtime", "chapter-0002.plan.md"), "二号章产物\n");
  writeFileSync(join(bookDir, "story", "drafts", "0002_灵脉草稿.md"), "二号章草稿\n");
  const index: ReadonlyArray<ChapterMeta> = rows.map((row) => ({
    number: row.number,
    title: row.title,
    status: "ready-for-review",
    wordCount: 10,
    createdAt: "2026-11-21T00:00:00.000Z",
    updatedAt: "2026-11-21T00:00:00.000Z",
    auditIssues: [],
    lengthWarnings: [],
  }));
  writeFileSync(join(bookDir, "chapters", "index.json"), JSON.stringify(index));
  const deps: ChapterStructureDeps = {
    bookDir,
    loadChapterIndex: async () =>
      JSON.parse(readFileSync(join(bookDir, "chapters", "index.json"), "utf-8")) as ReadonlyArray<ChapterMeta>,
  };
  return { bookDir, deps };
}

describe("splitChapter（R48a/1641 号）", () => {
  it("拆分：前段覆写、新章后段、后章前移、产物与归档就位", async () => {
    const { bookDir, deps } = seedBook();
    const outcome = await splitChapter(deps, "b1", 2, 6, undefined);
    expect(outcome.op).toBe("split");
    expect(outcome.splitFrom).toBe(2);
    expect(outcome.newChapter).toBe(3);
    expect(outcome.renumbered).toEqual([{ from: 3, to: 4 }]);
    const chapters = join(bookDir, "chapters");
    // 前 6 units = "# 灵脉\n\n"。
    const head = readFileSync(join(chapters, "0002_灵脉.md"), "utf-8");
    const tail = readFileSync(join(chapters, "0003_灵脉.md"), "utf-8");
    const moved = readFileSync(join(chapters, "0004_收束.md"), "utf-8");
    expect(head).toBe("# 灵脉\n\n");
    expect(tail).toContain("第二章正文乙。");
    expect(moved).toContain("正文丙。");
    expect(existsSync(join(chapters, "0003_收束.md"))).toBe(false);
    expect(existsSync(join(chapters, ".tmp-structure"))).toBe(false);
    // 归档（structure 来源）：旧号旧文 + 新号后段。
    expect(existsSync(join(chapters, ".versions", "0002"))).toBe(true);
    expect(existsSync(join(chapters, ".versions", "0003"))).toBe(true);
    // 产物：2 号不变号原位。
    expect(existsSync(join(bookDir, "story", "runtime", "chapter-0002.plan.md"))).toBe(true);
    expect(existsSync(join(bookDir, "story", "drafts", "0002_灵脉草稿.md"))).toBe(true);
    const index = JSON.parse(readFileSync(join(chapters, "index.json"), "utf-8")) as ChapterMeta[];
    expect(index.map((meta) => meta.number)).toEqual([1, 2, 3, 4]);
    expect(outcome.revision).toBe(chapterSetRevision(index));
  });

  it("乐观并发：expectedRevision 不符 409 CHAPTER_SET_CONFLICT，缺省跳过", async () => {
    const { deps } = seedBook();
    const current = chapterSetRevision(await deps.loadChapterIndex());
    await expect(splitChapter(deps, "b1", 2, 6, "deadbeefdeadbeef")).rejects.toMatchObject({
      status: 409,
      code: "CHAPTER_SET_CONFLICT",
    });
    await expect(splitChapter(deps, "b1", 2, 6, current)).resolves.toBeTruthy();
  });

  it("负路径：offset 越界 400、章缺失 404", async () => {
    const { deps } = seedBook();
    const content = readFileSync(join(deps.bookDir, "chapters", "0002_灵脉.md"), "utf-8");
    await expect(splitChapter(deps, "b1", 2, 0, undefined)).rejects.toMatchObject({ status: 400 });
    await expect(splitChapter(deps, "b1", 2, content.length, undefined)).rejects.toMatchObject({ status: 400 });
    await expect(splitChapter(deps, "b1", 9, 1, undefined)).rejects.toMatchObject({ status: 404 });
  });
});

describe("mergeChapters（R48a/1641 号）", () => {
  it("相邻合并：保留小号、被删章进 trash、后续前移", async () => {
    const { bookDir, deps } = seedBook();
    const outcome = await mergeChapters(deps, "b1", 2, 3, "\n\n", undefined);
    expect(outcome.op).toBe("merge");
    expect(outcome.mergedInto).toBe(2);
    expect(outcome.removedChapter).toBe(3);
    const chapters = join(bookDir, "chapters");
    const merged = readFileSync(join(chapters, "0002_灵脉.md"), "utf-8");
    expect(merged).toContain("正文乙。");
    expect(merged).toContain("正文丙。");
    expect(existsSync(join(chapters, "0003_收束.md"))).toBe(false);
    expect(existsSync(join(chapters, ".trash", "0003_收束.md"))).toBe(true);
    // 保留章（2 号）产物留原位；被删 3 号在 fixture 无产物（trash/runtime 空）。
    expect(existsSync(join(bookDir, "story", "runtime", "chapter-0002.plan.md"))).toBe(true);
    const index = JSON.parse(readFileSync(join(chapters, "index.json"), "utf-8")) as ChapterMeta[];
    expect(index.map((meta) => meta.number)).toEqual([1, 2]);
    expect(outcome.revision).toBe(chapterSetRevision(index));
  });

  it("负路径：非相邻 400、同章 400", async () => {
    const { deps } = seedBook();
    await expect(mergeChapters(deps, "b1", 1, 3, undefined, undefined)).rejects.toMatchObject({ status: 400 });
    await expect(mergeChapters(deps, "b1", 2, 2, undefined, undefined)).rejects.toMatchObject({ status: 400 });
  });
});

describe("reorderChapters（R48a/1641 号）", () => {
  it("重排：文件与索引按新序重编号", async () => {
    const { bookDir, deps } = seedBook();
    const outcome = await reorderChapters(deps, "b1", [3, 1, 2], undefined);
    expect(outcome.op).toBe("reorder");
    expect(outcome.renumbered).toEqual([
      { from: 1, to: 2 },
      { from: 2, to: 3 },
      { from: 3, to: 1 },
    ]);
    const chapters = join(bookDir, "chapters");
    expect(existsSync(join(chapters, "0001_收束.md"))).toBe(true);
    expect(existsSync(join(chapters, "0002_风起.md"))).toBe(true);
    expect(existsSync(join(chapters, "0003_灵脉.md"))).toBe(true);
    const index = JSON.parse(readFileSync(join(chapters, "index.json"), "utf-8")) as ChapterMeta[];
    expect(index.map((meta) => meta.title)).toEqual(["收束", "风起", "灵脉"]);
  });

  it("负路径：非双射 400、恒序 400", async () => {
    const { deps } = seedBook();
    await expect(reorderChapters(deps, "b1", [1, 2], undefined)).rejects.toMatchObject({ status: 400 });
    await expect(reorderChapters(deps, "b1", [1, 2, 2], undefined)).rejects.toMatchObject({ status: 400 });
    await expect(reorderChapters(deps, "b1", [1, 2, 3], undefined)).rejects.toMatchObject({ status: 400 });
  });
});

describe("ChapterStructureError 形状（端点层映射面）", () => {
  it("409 冲突携带 code", () => {
    const error = new ChapterStructureError(409, "x", "CHAPTER_SET_CONFLICT");
    expect(error.status).toBe(409);
    expect(error.code).toBe("CHAPTER_SET_CONFLICT");
    expect(error instanceof Error).toBe(true);
  });
});
