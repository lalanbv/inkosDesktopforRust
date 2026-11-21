//! R49/1644 号：book.json 与 chapters/index.json 键面 parity 锁（1604 号
//! 机制扩展到 book 面）。
//!
//! fixtures（book-config-keyset.json / chapter-meta-keyset.json）=
//! engine-rs/tests/book_config_keyset_fixture.rs 守卫断言的最小合法 JSON
//! serde round-trip。两侧守卫语义：Rust struct 键面变化 → Rust 守卫红 →
//! 人工定性更新 fixture → 本测试的键集对比与全文 parse 给出 TS zod schema
//! 是否需要跟随的信号。R49 新增可选键：writing.volumes（卷有序列表）与
//! chapters[].volumeId（卷归属）——存量书键缺省零迁移。

import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { BookConfigSchema, BookVolumeSchema, BookWritingSchema } from "../models/book.js";
import { ChapterMetaSchema } from "../models/chapter.js";

const here = dirname(fileURLToPath(import.meta.url));
const bookFixture: Record<string, unknown> = JSON.parse(
  readFileSync(join(here, "fixtures/book-config-keyset.json"), "utf-8"),
);
const chapterFixture: Record<string, unknown> = JSON.parse(
  readFileSync(join(here, "fixtures/chapter-meta-keyset.json"), "utf-8"),
);

describe("BookConfig 键面 parity（Rust fixture ↔ TS zod schema，R49/1644 号）", () => {
  it("writing 段键集 ⊆ BookWritingSchema shape 键集（volumes 在位）", () => {
    const writing = bookFixture.writing as Record<string, unknown>;
    const shapeKeys = new Set(Object.keys(BookWritingSchema.shape));
    const missing = Object.keys(writing).filter(
      (key) => !shapeKeys.has(key) && key !== "$comment",
    );
    expect(missing).toEqual([]);
    // R49 新增键显式在位（卷有序列表 schema 面锁定）。
    expect(shapeKeys.has("volumes")).toBe(true);
  });

  it("writing.volumes 条目键集 ⊆ BookVolumeSchema shape 键集", () => {
    const volumesShape = BookVolumeSchema.shape;
    const entry = (bookFixture.writing as { volumes: Array<Record<string, unknown>> }).volumes[0]!;
    const missing = Object.keys(entry).filter(
      (key) => !(key in volumesShape) && key !== "$comment",
    );
    expect(missing).toEqual([]);
  });

  it("chapters 行键集 ⊆ ChapterMetaSchema shape 键集（volumeId 在位）", () => {
    const shapeKeys = new Set(Object.keys(ChapterMetaSchema.shape));
    const missing = Object.keys(chapterFixture).filter(
      (key) => !shapeKeys.has(key) && key !== "$comment",
    );
    expect(missing).toEqual([]);
    expect(shapeKeys.has("volumeId")).toBe(true);
  });

  it("全文 parse：book.json 样本过 BookConfigSchema（含 writing.volumes）", () => {
    const { $comment: _ignored, ...bookConfig } = bookFixture;
    const parsed = BookConfigSchema.parse(bookConfig);
    expect(parsed.writing?.volumes).toHaveLength(2);
    expect(parsed.writing?.volumes?.[0]?.id).toBe("vol-1");
  });

  it("全文 parse：chapter 行样本过 ChapterMetaSchema（含 volumeId）", () => {
    // ChapterMetaSchema 是 strict（709 号族——未知键拒收），fixture 注释键剥除。
    const { $comment: _ignored, ...chapterMeta } = chapterFixture;
    const parsed = ChapterMetaSchema.parse(chapterMeta);
    expect(parsed.volumeId).toBe("vol-1");
  });

  it("存量书零迁移：键缺省（无 volumes/volumeId）parse 行为不变", () => {
    const { $comment: _ignored, writing, ...legacyBook } = bookFixture;
    const parsed = BookConfigSchema.parse(legacyBook);
    expect(parsed.writing).toBeUndefined();
    const { $comment: _comment, volumeId: _dropped, ...legacyChapter } = chapterFixture;
    const legacyParsed = ChapterMetaSchema.parse(legacyChapter);
    expect(legacyParsed.volumeId).toBeUndefined();
  });
});
