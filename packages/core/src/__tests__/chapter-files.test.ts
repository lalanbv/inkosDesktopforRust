import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { StateManager } from "../state/manager.js";
import { chapterFileNumber, isCanonicalChapterFile, selectChapterFiles } from "../utils/chapter-files.js";

// 813 号：章节文件识别单一真源 + 同章双形态去重（Rust state::manager leading_number /
// select_chapter_files / rebuild_chapter_index_from_files_at 同策略）。
describe("chapter file identity (813)", () => {
  it("recognises canonical, legacy-hyphen and bare numbered files by leading chapter number", () => {
    expect(chapterFileNumber("0001_新题.md")).toBe(1);
    expect(chapterFileNumber("0001-旧题.md")).toBe(1);
    expect(chapterFileNumber("0003.md")).toBe(3);
    expect(chapterFileNumber("index.json")).toBeUndefined();
    expect(chapterFileNumber("chapter-0001.plan.md")).toBeUndefined();
    expect(isCanonicalChapterFile("0001_新题.md")).toBe(true);
    expect(isCanonicalChapterFile("0001-旧题.md")).toBe(false);
    expect(isCanonicalChapterFile("0003.md")).toBe(false);
  });

  it("dedupes same-chapter files preferring the canonical form, then name order, skipping chapter 0", () => {
    expect(selectChapterFiles([
      "0002-b.md", "0001_新.md", "0001-旧.md", "0003.md", "0000_zero.md", "index.json", "0002-a.md",
    ])).toEqual(["0001_新.md", "0002-a.md", "0003.md"]);
  });

  it("rebuilds a chapter index with one row per chapter when dual-form files coexist", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-chapter-files-"));
    try {
      const manager = new StateManager(root);
      const chaptersDir = join(manager.bookDir("dup"), "chapters");
      await mkdir(chaptersDir, { recursive: true });
      await writeFile(join(chaptersDir, "0001-旧题.md"), "legacy");
      await writeFile(join(chaptersDir, "0001_新题.md"), "canonical");
      await writeFile(join(chaptersDir, "0002.md"), "bare");
      const index = await manager.loadChapterIndex("dup"); // 索引缺失 → 文件重建
      expect(index.map((meta) => meta.number)).toEqual([1, 2]);
      expect(index[0]?.title).toBe("新题");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});
