import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createInitialWorkManifestWrite } from "@actalk/inkos-core";
import { createStudioServer } from "../api/server.js";

// 1678 号：章文件用旧命名（`NNNN-标题.md` / 裸 `NNNN.md`）的书，在 Node 腿的书架接口上
// 此前是整页 500——durable 进度的一致性守卫只认正典 `NNNN_标题.md`，把索引里列着的
// 这些章当成「缺文件」抛错；没有索引时进度算成 0（下一章号回到 1）。

describe("books whose chapters are saved under legacy names", () => {
  const roots: string[] = [];
  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  /** 三种命名形态各一章的书；`indexed` = 带不带章节索引。 */
  async function seedLegacyBook(root: string, id: string, indexed: boolean): Promise<void> {
    const initial = createInitialWorkManifestWrite({ workId: id, title: id, profileId: "longform-novel", language: "zh", writes: [] });
    const source = join(root, "works", id, "source");
    await mkdir(join(source, "chapters"), { recursive: true });
    await writeFile(join(root, initial.write.relativePath), initial.write.content);
    const stamp = "2026-11-25T00:00:00.000Z";
    await writeFile(join(source, "book.json"), JSON.stringify({
      id, title: id, platform: "local", genre: "mystery", status: "active",
      targetChapters: 12, chapterWordCount: 3000, language: "zh", createdAt: stamp, updatedAt: stamp,
    }));
    for (const [file, title] of [["0001-镜中醒来", "镜中醒来"], ["0002", "第2章"], ["0003_云涌", "云涌"]]) {
      await writeFile(join(source, "chapters", `${file}.md`), `# ${title}\n\n正文。`);
    }
    if (!indexed) return;
    await writeFile(join(source, "chapters", "index.json"), JSON.stringify([1, 2, 3].map((number) => ({
      number, title: `第${number}章`, status: "ready-for-review", wordCount: 2,
      createdAt: stamp, updatedAt: stamp, auditIssues: [], lengthWarnings: [],
    }))));
  }

  it("are listed with their chapters counted, with or without an index", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-legacy-forms-"));
    roots.push(root);
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "legacy-forms-project", version: "2.0.0", language: "zh",
      llm: { model: "test-model", provider: "openai" }, notify: [],
    }));
    await seedLegacyBook(root, "indexed", true);
    await seedLegacyBook(root, "unindexed", false);

    const response = await createStudioServer({} as never, root).request("/api/v1/books");
    expect(response.status).toBe(200);
    const body = await response.json() as { books: Array<{ id: string; chaptersWritten: number }>; incompleteWorkIds: string[] };
    expect(Object.fromEntries(body.books.map((book) => [book.id, book.chaptersWritten]))).toEqual({ indexed: 3, unindexed: 3 });
    expect(body.incompleteWorkIds).toEqual([]);
  });

  it("open their detail page with the next chapter after the last one on disk", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-legacy-forms-"));
    roots.push(root);
    await seedLegacyBook(root, "unindexed", false);
    const app = createStudioServer({} as never, root);
    const detail = await app.request("/api/v1/books/unindexed");
    expect(detail.status).toBe(200);
    const body = await detail.json() as { nextChapter: number };
    expect(body.nextChapter).toBe(4);
    // 书没被动过：读取面不落盘（没有补出索引）。
    await expect(readFile(join(root, "works", "unindexed", "source", "chapters", "index.json"))).rejects.toMatchObject({ code: "ENOENT" });
  });
});
