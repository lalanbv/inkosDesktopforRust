import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { ChapterExportSourceError, readBookExportSource, type ExportStateLike } from "../interaction/export-artifact.js";

// 1678 号：导出的章文件查找表此前只认正典 `NNNN_标题.md`——索引里列着旧命名的章
// （`NNNN-标题.md` / 裸 `NNNN.md`）时，导出把这些章报成「缺文件 / 未入索引」而拒绝
// （CHAPTER_EXPORT_SOURCE_MISMATCH）。章文件的认定要与索引重建同一集合；校验本身的
// 严格度不变：真缺文件、未入索引的章、同章两个文件，照样拒绝。

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

interface Chapter { readonly number: number; readonly title: string; readonly wordCount: number }

async function seedBook(files: ReadonlyArray<string>, chapters: ReadonlyArray<Chapter>): Promise<ExportStateLike> {
  const root = await mkdtemp(join(tmpdir(), "inkos-export-forms-"));
  roots.push(root);
  const bookDir = join(root, "book");
  await mkdir(join(bookDir, "chapters"), { recursive: true });
  for (const file of files) await writeFile(join(bookDir, "chapters", file), `# ${file}\n\n正文。`);
  return {
    bookDir: () => bookDir,
    loadBookConfig: async () => ({ title: "旧命名的书", language: "zh", chapterWordCount: 3000 }),
    loadChapterIndex: async () => chapters,
  };
}

const THREE = [
  { number: 1, title: "镜中醒来", wordCount: 2 },
  { number: 2, title: "云涌", wordCount: 2 },
  { number: 3, title: "收束", wordCount: 2 },
];

/** 被导出拒绝，且拒绝理由落在预期的那一项上。 */
const refusedBecause = (pick: (details: InstanceType<typeof ChapterExportSourceError>["details"]) => boolean) => (
  error: unknown,
): boolean => error instanceof ChapterExportSourceError && pick(error.details);

describe("export finds chapters saved under any recognized file form", () => {
  it("exports a book whose chapters use legacy names", async () => {
    const state = await seedBook(["0001-镜中醒来.md", "0002.md", "0003_收束.md"], THREE);
    const source = await readBookExportSource(state, "book");
    expect(source.chapters.map((chapter) => [chapter.number, chapter.file])).toEqual([
      [1, "0001-镜中醒来.md"],
      [2, "0002.md"],
      [3, "0003_收束.md"],
    ]);
  });

  it("still refuses when an indexed chapter has no file in any form", async () => {
    const state = await seedBook(["0001-镜中醒来.md", "0003_收束.md"], THREE);
    await expect(readBookExportSource(state, "book")).rejects.toSatisfy(
      refusedBecause((details) => details.missingChapterNumbers.join() === "2"),
    );
  });

  it("still refuses when a chapter file is not in the index, whatever its form", async () => {
    const state = await seedBook(["0001-镜中醒来.md", "0002.md", "0003_收束.md", "0004_旧.md"], THREE);
    await expect(readBookExportSource(state, "book")).rejects.toSatisfy(
      refusedBecause((details) => details.unindexedFiles.join() === "0004_旧.md"),
    );
  });

  it("still refuses when a chapter has two files", async () => {
    // 同章双形态（读侧的裁决是正典优先，845 号）——导出不替用户挑，照样报重复。
    const state = await seedBook(["0001-镜中醒来.md", "0001_新.md", "0002.md", "0003_收束.md"], THREE);
    await expect(readBookExportSource(state, "book")).rejects.toSatisfy(
      refusedBecause((details) => details.duplicateChapterNumbers.join() === "1"),
    );
  });
});
