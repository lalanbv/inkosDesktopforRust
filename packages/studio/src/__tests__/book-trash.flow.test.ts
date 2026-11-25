import { mkdir, mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createInitialWorkManifestWrite } from "@actalk/inkos-core";
import { createStudioServer } from "../api/server.js";
import { untar } from "../lib/tar-read.js";

// R58/1676 号：破坏性操作护栏的 Node 腿面——删书进项目级回收站、备份不带回收站。
// 此前 DELETE /api/v1/books/:id 只 rm 掉 works/<id>/source/、清单留在 works 里：书「删了」
// 却还挂在「未完成初始化的作品」下，内容则找不回来。

describe("deleting a book (R58)", () => {
  const roots: string[] = [];
  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  async function tempRoot(): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-book-trash-"));
    roots.push(root);
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "trash-project", version: "2.0.0", language: "zh",
      llm: { model: "test-model", provider: "openai" }, notify: [],
    }));
    return root;
  }

  async function seedWork(root: string, id: string, profileId = "longform-novel"): Promise<string> {
    const initial = createInitialWorkManifestWrite({ workId: id, title: id, profileId, language: "zh", writes: [] });
    const workDir = join(root, "works", id);
    await mkdir(join(workDir, "source", "chapters"), { recursive: true });
    await writeFile(join(root, initial.write.relativePath), initial.write.content);
    await writeFile(join(workDir, "source", "book.json"), JSON.stringify({ id, title: id }));
    await writeFile(join(workDir, "source", "chapters", "0001_风起.md"), "# 第1章 风起\n");
    return workDir;
  }

  it("moves the whole work into the project trash and the book leaves the library", async () => {
    const root = await tempRoot();
    await seedWork(root, "b1");
    const app = createStudioServer({} as never, root);

    const response = await app.request("/api/v1/books/b1", { method: "DELETE" });
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ ok: true, bookId: "b1" });

    // 书目录不在了；整个作品目录（清单 + source/）在项目级回收站里。
    await expect(readdir(join(root, "works", "b1"))).rejects.toMatchObject({ code: "ENOENT" });
    const entries = await readdir(join(root, ".trash", "books"));
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatch(/^b1--\d{14}$/);
    const trashed = join(root, ".trash", "books", entries[0]!);
    expect(JSON.parse(await (await import("node:fs/promises")).readFile(join(trashed, "work.json"), "utf8"))).toMatchObject({ id: "b1", profileId: "longform-novel" });
    expect(await (await import("node:fs/promises")).readFile(join(trashed, "source", "chapters", "0001_风起.md"), "utf8")).toBe("# 第1章 风起\n");

    // 此前的缺陷：清单留在 works 里，书挂在「未完成初始化的作品」下。
    expect(await (await app.request("/api/v1/books")).json()).toEqual({ books: [], incompleteWorkIds: [] });
    expect(await (await app.request("/api/v1/works")).json()).toMatchObject({ works: [] });
  });

  it("still answers ok for a book that is not there and creates no trash entry", async () => {
    const root = await tempRoot();
    const response = await createStudioServer({} as never, root).request("/api/v1/books/nowhere", { method: "DELETE" });
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ ok: true, bookId: "nowhere" });
    await expect(readdir(join(root, ".trash"))).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("refuses to delete a work that is not a book through the books route", async () => {
    const root = await tempRoot();
    await seedWork(root, "world-1", "interactive-world");
    const response = await createStudioServer({} as never, root).request("/api/v1/books/world-1", { method: "DELETE" });
    expect(response.status).toBe(400);
    expect(await (await import("node:fs/promises")).readFile(join(root, "works", "world-1", "source", "book.json"), "utf8")).toContain("world-1");
  });
});

describe("backup export leaves trash directories out (R58)", () => {
  it("keeps chapter-level and book-level trash on disk but out of the archive", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-backup-trash-"));
    try {
      const source = join(root, "works", "b1", "source");
      const initial = createInitialWorkManifestWrite({ workId: "b1", title: "b1", profileId: "longform-novel", language: "zh", writes: [] });
      await mkdir(join(source, "chapters", ".trash", "rollback-20261125093007-1", "chapters"), { recursive: true });
      await writeFile(join(root, initial.write.relativePath), initial.write.content);
      await writeFile(join(source, "book.json"), "{}");
      await writeFile(join(source, "chapters", "0001_风起.md"), "# 第1章 风起\n");
      await writeFile(join(source, "chapters", ".trash", "0000_old.md"), "old");
      await writeFile(join(source, "chapters", ".trash", "rollback-20261125093007-1", "chapters", "0002_云涌.md"), "discarded");
      const bookTrash = join(root, ".trash", "books", "b0--20261125093007");
      await mkdir(bookTrash, { recursive: true });
      await writeFile(join(bookTrash, "book.json"), "{}");

      const response = await createStudioServer({} as never, root).request("/api/v1/backup/export");
      expect(response.status).toBe(200);
      const { entries, rejected } = untar(new Uint8Array(await response.arrayBuffer()));
      expect(rejected).toEqual([]);
      const names = entries.map((entry) => entry.name);
      expect(names.some((name) => name.includes("/.trash"))).toBe(false);
      expect(names).toContain("inkos-backup/works/b1/work.json");
      expect(names).toContain("inkos-backup/works/b1/source/book.json");
      expect(names).toContain("inkos-backup/works/b1/source/chapters/0001_风起.md");
      // 盘上的回收站原样还在。
      expect(await readdir(join(source, "chapters", ".trash"))).toEqual(["0000_old.md", "rollback-20261125093007-1"]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});
