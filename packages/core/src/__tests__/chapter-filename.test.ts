import { describe, expect, it } from "vitest";
import { findChapterFileByNumber, parseChapterFileName } from "../state/chapter-filename.js";

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

  it("同号多形态确定性择优：4 位补零（引擎主形态）优先", () => {
    expect(findChapterFileByNumber(["7_a.md", "0007_b.md"], 7)).toBe("0007_b.md");
    expect(findChapterFileByNumber(["7_b.md", "007_a.md"], 7)).toBe("007_a.md");
  });

  it("非 .md 不误收", () => {
    expect(findChapterFileByNumber(["7_导入.txt", "0007_a.md"], 7)).toBe("0007_a.md");
  });
});
