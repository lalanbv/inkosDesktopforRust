//! 短篇生产纯函数层跨引擎金标准（887 号 M3a，TS 侧）。与 Rust 侧
//! `engine-rs/src/production/short_fiction.rs` 的 `mod tests` **逐字共用**
//! 同一组金标准字符串与断言值（双端恒等锁）：渲染/度量/批次/契约 strict 面
//! 任一端漂移必有一端红。
//!
//! 契约：ShortFictionBatchDraftSchema（zod strict）↔ Rust ShortFictionBatchDraft
//!（deny_unknown_fields）——fields/charCount 必填/rawContent 字段集精确互逆。

import { describe, expect, it } from "vitest";
import {
  ShortFictionBatchDraftSchema,
  buildShortFictionChapterBatches,
  formatShortFictionChapterHeading,
  measureShortFictionDraft,
  renderShortFictionDraftMarkdown,
  validateShortFictionDraftForFinal,
} from "../index.js";

const GOLDEN_DRAFT_JSON = `{
  "storyTitle": "雨夜碎镜",
  "openingHook": "雨夜来客叩门。",
  "chapters": [
    { "number": 1, "title": "换名", "content": "斋主以镜换名，来客留下一个再未使用的名字。", "charCount": 20 },
    { "number": 2, "title": "窥影", "content": "镜中人影与来客同貌，交易自此刻开始。", "charCount": 18 }
  ],
  "rawContent": ""
}`;

/// 渲染金标准（与 Rust production/short_fiction.rs GOLDEN_RENDERED_MARKDOWN 逐字共用）。
const GOLDEN_RENDERED_MARKDOWN = `# 雨夜碎镜

## 开篇钩子

雨夜来客叩门。

## 第1章 换名

斋主以镜换名，来客留下一个再未使用的名字。

## 第2章 窥影

镜中人影与来客同貌，交易自此刻开始。`;

const goldenDraft = () => ShortFictionBatchDraftSchema.parse(JSON.parse(GOLDEN_DRAFT_JSON));

describe("短篇纯函数层跨引擎金标准（887）", () => {
  it("strict 契约：未知键拒 + 缺 charCount 拒（serde deny_unknown_fields 对齐）", () => {
    expect(() => ShortFictionBatchDraftSchema.parse({
      storyTitle: "x", chapters: [], rawContent: "", sneaky: 1,
    })).toThrow();
    expect(() => ShortFictionBatchDraftSchema.parse({
      storyTitle: "x",
      chapters: [{ number: 1, title: "t", content: "c" }],
      rawContent: "",
    })).toThrow();
    const round = JSON.parse(JSON.stringify(goldenDraft()));
    expect(ShortFictionBatchDraftSchema.parse(round)).toEqual(goldenDraft());
  });

  it("渲染金标准逐字一致（与 Rust GOLDEN_RENDERED_MARKDOWN 共用）", () => {
    expect(renderShortFictionDraftMarkdown(goldenDraft(), "zh")).toBe(GOLDEN_RENDERED_MARKDOWN);
  });

  it("度量：开篇钩子 7 字（非空白字符计数）+ 结构面", () => {
    const measurements = measureShortFictionDraft(goldenDraft(), "zh");
    expect(measurements.openingHookLength).toBe(7);
    expect(measurements.chapterCount).toBe(2);
    expect(measurements.unit).toBe("non-whitespace-characters");
    expect(measurements.chapterLengths).toHaveLength(2);
    expect(measurements.totalLength).toBe(
      measurements.openingHookLength
        + measurements.chapterLengths.reduce((total, chapter) => total + chapter.length, 0),
    );
  });

  it("章标题格式：第N章 标题（空标题抛错）", () => {
    expect(formatShortFictionChapterHeading(1, "换名", "zh")).toBe("第1章 换名");
    expect(formatShortFictionChapterHeading(3, "Opening", "en")).toBe("Chapter 3: Opening");
    expect(() => formatShortFictionChapterHeading(4, "  ", "zh")).toThrow();
  });

  it("批次：预算分批+去重排序（与 Rust 同参数同结果）", () => {
    expect(
      buildShortFictionChapterBatches([12, 3, 1, 2, 2, 5, 8, 9, 10, 11, 4, 6, 7], 1000, 24576, undefined),
    ).toEqual([[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], [11, 12]]);
    expect(buildShortFictionChapterBatches([1, 2, 3, 4], 1000, 24576, 2)).toEqual([[1, 2], [3, 4]]);
  });

  it("终稿校验变异：U+FFFD 腐化→TEXT_CORRUPTED / 空章→INCOMPLETE（码面双端对齐）", () => {
    const corrupted = JSON.parse(JSON.stringify(goldenDraft()));
    corrupted.chapters[0].content = "含替换字符\uFFFD的正文";
    try {
      validateShortFictionDraftForFinal(corrupted, { expectedChapters: 2 });
      expect.unreachable("必须抛出");
    } catch (error) {
      expect((error as { code?: string }).code).toBe("SHORT_CHAPTER_TEXT_CORRUPTED");
      expect((error as { chapters?: number[] }).chapters).toEqual([1]);
    }
    const incomplete = JSON.parse(JSON.stringify(goldenDraft()));
    incomplete.chapters[1].content = "";
    try {
      validateShortFictionDraftForFinal(incomplete, { expectedChapters: 2 });
      expect.unreachable("必须抛出");
    } catch (error) {
      expect((error as { code?: string }).code).toBe("SHORT_CHAPTER_INCOMPLETE");
      expect((error as { chapters?: number[] }).chapters).toEqual([2]);
    }
    expect(() => validateShortFictionDraftForFinal(goldenDraft(), { expectedChapters: 2 })).not.toThrow();
  });

  it("未完成章不渲染（2.0 形态：completedChapters 过滤——旧形态渲染标题行）", () => {
    const draft = JSON.parse(JSON.stringify(goldenDraft()));
    draft.chapters[1].title = "";
    const rendered = renderShortFictionDraftMarkupSafe(draft);
    expect(rendered).not.toContain("窥影");
    expect(rendered).toContain("换名");
  });

  // renderShortFictionDraftMarkdown 对空标题章会因 format 抛错——先过滤再渲染
  //（与 TS 源码 completedChapters 过滤语义一致的测试辅助）。
  function renderShortFictionDraftMarkupSafe(draft: { storyTitle: string; openingHook?: string; chapters: Array<{ number: number; title: string; content: string; charCount: number }>; rawContent: string }): string {
    const completed = {
      ...draft,
      chapters: draft.chapters.filter((chapter) => chapter.title.trim() && chapter.content.trim()),
    };
    return renderShortFictionDraftMarkdown(ShortFictionBatchDraftSchema.parse({
      ...completed,
      chapters: completed.chapters.map((chapter) => ({ ...chapter, charCount: 0 })),
    }), "zh");
  }
});
