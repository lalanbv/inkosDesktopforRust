import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { migrateLegacyWorks } from "@actalk/inkos-core";

/**
 * 696/697 号：2.0 work 迁移边界——legacy 书（books/<中文 id>/）经
 * migrateLegacyWorks 注册进 works 清单后，/books 与 /doctor bookCount
 * 在 TS 腿与 Rust 腿对齐（contract-diff 共享面剩余 2 分歧的消解验证）。
 */
describe("legacy book work-migration boundary (696/697)", () => {
  let root: string;
  const BOOK = "镜花水月";

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "work-migrate-"));
    // 模拟 walkthrough-fixture 的 legacy 播种（book.json + 章节 + story/）。
    await mkdir(join(root, "books", BOOK, "chapters"), { recursive: true });
    await mkdir(join(root, "books", BOOK, "story"), { recursive: true });
    await writeFile(join(root, "books", BOOK, "book.json"), JSON.stringify({
      id: BOOK, title: BOOK, platform: "qidian", genre: "xuanhuan", status: "active",
      targetChapters: 12, chapterWordCount: 3000, language: "zh",
      createdAt: "2026-09-14T00:00:00.000Z", updatedAt: "2026-09-14T00:00:00.000Z",
    }));
    await writeFile(join(root, "books", BOOK, "chapters", "0001_第一节.md"), "# 第一节\n\n正文。");
    await writeFile(join(root, "books", BOOK, "story", "current_state.md"), "# 当前状态\n");
    await writeFile(join(root, "inkos.json"), JSON.stringify({ name: "walkthrough", version: "0.1.0", language: "zh" }));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("migrateLegacyWorks registers the legacy book so /books and /doctor see it", async () => {
    const results = await migrateLegacyWorks(root, { apply: true });
    const bookMigration = results.find((item) => item.source === `books/${BOOK}`);
    expect(bookMigration?.status, JSON.stringify(results)).toBe("migrated");

    const app = createStudioServer({} as never, root);
    const books = await app.request("/api/v1/books");
    expect(books.status).toBe(200);
    const booksBody = await books.json() as { books: ReadonlyArray<{ id: string }> };
    expect(booksBody.books).toHaveLength(1);
    expect(booksBody.books[0]?.id).toBe(BOOK);

    const doctor = await app.request("/api/v1/doctor");
    const doctorBody = await doctor.json() as { bookCount?: number };
    expect(doctorBody.bookCount).toBe(1);
  });
});
