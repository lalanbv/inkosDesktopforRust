import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { StateManager } from "../state/manager.js";
import {
  chapterFileNumber,
  findChapterFileByNumber,
  isCanonicalChapterFile,
  parseChapterFileName,
  preferredChapterFile,
  selectChapterFiles,
} from "../utils/chapter-files.js";

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

// 682 号：章节文件名前导号解析——「能列出（索引宽松侧）即可打开（详情侧）」
// 的单一事实源。引擎写入恒 4 位补零；导入/手工章节位数不限。
describe("parseChapterFileName（682 号）", () => {
  it("解析引擎主形态：4 位补零 + 下划线", () => {
    expect(parseChapterFileName("0007_雨夜.md")).toEqual({ number: 7, base: "雨夜" });
  });

  it("宽松位数与可选分隔符（与索引宽松侧同形）", () => {
    expect(parseChapterFileName("7_导入.md")?.number).toBe(7);
    expect(parseChapterFileName("007-尾声.md")?.number).toBe(7);
    expect(parseChapterFileName("12_番外篇.md")?.number).toBe(12);
    expect(parseChapterFileName("0007.md")?.number).toBe(7);
  });

  it("非章节文件与非法号拒绝", () => {
    expect(parseChapterFileName("index.json")).toBeNull();
    expect(parseChapterFileName("README.md")).toBeNull();
    expect(parseChapterFileName("0_零.md")).toBeNull();
    expect(parseChapterFileName("notes/0007_a.md")).toBeNull();
  });
});

describe("findChapterFileByNumber（682 号：详情侧与索引同源定位）", () => {
  it("非 4 位前导号可定位（修复「能列出打不开」）", () => {
    expect(findChapterFileByNumber(["0003_其他.md", "7_导入.md"], 7)).toBe("7_导入.md");
  });

  it("主路径回归：4 位补零形态照常定位", () => {
    expect(findChapterFileByNumber(["0007_雨夜.md"], 7)).toBe("0007_雨夜.md");
  });

  it("前缀碰撞根除：num=7 不误配 00071_", () => {
    expect(findChapterFileByNumber(["00071_七十一.md"], 7)).toBeUndefined();
    expect(findChapterFileByNumber(["00071_七十一.md"], 71)).toBe("00071_七十一.md");
  });

  it("同号多形态确定性择优：正典下划线类优先，类内文件名序", () => {
    expect(findChapterFileByNumber(["7_a.md", "0007_b.md"], 7)).toBe("0007_b.md");
    expect(findChapterFileByNumber(["7_b.md", "007_a.md"], 7)).toBe("007_a.md");
  });

  it("非 .md 不误收；非法章号直接 undefined", () => {
    expect(findChapterFileByNumber(["7_导入.txt", "0007_a.md"], 7)).toBe("0007_a.md");
    expect(findChapterFileByNumber(["0000_零.md"], 0)).toBeUndefined();
    expect(findChapterFileByNumber(["0001_a.md"], Number.NaN)).toBeUndefined();
  });
});

// 845 号：682 读侧定位与 813 索引去重此前各持一套择优（682 按 padded 类内字典序 '-'<'_' 取
// 连字符旧章；813 正典优先）——同一本书「索引列 0001_新、详情开 0001-旧」。合并为唯一裁决
// preferredChapterFile 后两者对任一章的胜者恒等（Rust preferred_chapter_file 同策略）。
describe("unified dual-form preference (845)", () => {
  it("canonical underscore beats legacy hyphen regardless of byte order", () => {
    expect(preferredChapterFile("0001-旧.md", "0001_新.md")).toBe("0001_新.md");
    expect(preferredChapterFile("0001_新.md", "0001-旧.md")).toBe("0001_新.md");
    expect(findChapterFileByNumber(["0001-旧.md", "0001_新.md"], 1)).toBe("0001_新.md");
    expect(findChapterFileByNumber(["0001_新.md", "0001-旧.md"], 1)).toBe("0001_新.md");
  });

  it("falls back to name order inside the same class", () => {
    expect(preferredChapterFile("0002-b.md", "0002-a.md")).toBe("0002-a.md");
    expect(preferredChapterFile("0002_b.md", "0002_a.md")).toBe("0002_a.md");
    expect(findChapterFileByNumber(["0002-b.md", "0002-a.md"], 2)).toBe("0002-a.md");
  });

  it("findChapterFileByNumber agrees with selectChapterFiles for every chapter (input order independent)", () => {
    const files = [
      "0002-b.md", "0001_新.md", "0001-旧.md", "0003.md", "0000_zero.md", "index.json", "0002-a.md",
      "7_a.md", "0007_b.md", "007_c.md", "00071_七十一.md",
    ];
    const reversed = [...files].reverse();
    for (const selected of selectChapterFiles(files)) {
      const number = chapterFileNumber(selected)!;
      expect(findChapterFileByNumber(files, number)).toBe(selected);
      expect(findChapterFileByNumber(reversed, number)).toBe(selected);
    }
    expect(selectChapterFiles(reversed)).toEqual(selectChapterFiles(files));
  });

  // 854 号：类内文件名序与 Rust `&str` 字节序（= 码点序）恒等——JS 默认 `<` 的 UTF-16
  // 码元序对「增补平面字符（emoji 代理对）vs U+E000-U+FFFF（全角标点）」次序相反，
  // 旧实现同章双正典时 TS 取 emoji 文件、Rust 取全角叹号文件（双引擎开不同章）。
  it("ties inside the same class by code-point order, matching Rust byte order (854)", () => {
    const fullwidth = "0001_开局！.md"; // U+FF01（UTF-8 首字节 EF）
    const emoji = "0001_开局🔥.md"; // U+1F525（UTF-8 首字节 F0，码点更大）
    expect(preferredChapterFile(fullwidth, emoji)).toBe(fullwidth);
    expect(preferredChapterFile(emoji, fullwidth)).toBe(fullwidth);
    expect(findChapterFileByNumber([fullwidth, emoji], 1)).toBe(fullwidth);
    expect(findChapterFileByNumber([emoji, fullwidth], 1)).toBe(fullwidth);
    // 前缀等值时短者在前（与 Rust 字节序一致）。
    expect(preferredChapterFile("0002_ab.md", "0002_a.md")).toBe("0002_a.md");
  });
});

// 854 号：章号上界与 Rust `parse::<u32>` 恒等——超 u32 的前导号 TS 旧实现 `parseInt` 给出
// 浮点整数（`Number.isInteger(1e20) === true`）列为章，Rust 解析失败判非章，双引擎索引分叉。
describe("chapter number bounds (854)", () => {
  it("rejects leading numbers beyond u32::MAX like Rust parse::<u32>", () => {
    expect(chapterFileNumber("4294967295_界内.md")).toBe(4294967295);
    expect(chapterFileNumber("4294967296_超界.md")).toBeUndefined();
    expect(chapterFileNumber("9".repeat(20) + "_天文.md")).toBeUndefined();
    expect(selectChapterFiles(["4294967296_超界.md", "0001_正常.md"])).toEqual(["0001_正常.md"]);
    expect(parseChapterFileName("4294967296_超界.md")).toBeNull();
    expect(findChapterFileByNumber(["4294967296_超界.md"], 4294967296)).toBeUndefined();
  });

  it("treats non-ASCII digits as not-a-chapter like JS \\d (Rust regex aligned to [0-9])", () => {
    expect(chapterFileNumber("１２３_全角数字.md")).toBeUndefined();
    expect(chapterFileNumber("١٢٣_阿拉伯数字.md")).toBeUndefined();
  });
});
