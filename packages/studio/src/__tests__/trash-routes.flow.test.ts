import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInitialWorkManifestWrite } from "@actalk/inkos-core";
import { afterEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

// 1679 号（R60）：回收站的路由面——此前两腿都只有「往里放」，没有列出 / 恢复 / 清空。
//
// 恢复是「把文件找回来」，不是撤销：目标已在原位 → 409 不覆盖；找回章文件时目录里缺这章
// 就补一条（否则进度守卫把它判成孤儿，这本书在书架接口上 500）；状态与快照不动。
// 书级回收站两腿共用一个目录：本腿只认自己删出来的形态。

describe("trash routes", () => {
  const roots: string[] = [];
  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  async function tempRoot(): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-trash-routes-"));
    roots.push(root);
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "trash-project", version: "2.0.0", language: "zh",
      llm: { model: "test-model", provider: "openai" }, notify: [],
    }));
    return root;
  }

  const STAMP = "2026-08-17T00:00:00.000Z";
  const TITLES = ["风起", "云涌", "收束"];

  /** 一本书：三章、带索引、每章都有快照（回滚要靠它恢复状态）。 */
  async function seedBook(root: string, id: string, title: string): Promise<string> {
    const initial = createInitialWorkManifestWrite({ workId: id, title, profileId: "longform-novel", language: "zh", writes: [] });
    const source = join(root, "works", id, "source");
    await mkdir(join(source, "chapters"), { recursive: true });
    await writeFile(join(root, initial.write.relativePath), initial.write.content);
    await writeFile(join(source, "book.json"), JSON.stringify({
      id, title, platform: "local", genre: "mystery", status: "active",
      targetChapters: 12, chapterWordCount: 3000, language: "zh", createdAt: STAMP, updatedAt: STAMP,
    }));
    const index = [];
    for (const [offset, name] of TITLES.entries()) {
      const number = offset + 1;
      await writeFile(join(source, "chapters", `000${number}_${name}.md`), `# 第${number}章 ${name}\n\n正文${number}。`);
      index.push({
        number, title: name, status: "approved", wordCount: 5, createdAt: STAMP, updatedAt: STAMP,
        auditIssues: [], lengthWarnings: [],
      });
      const snapshot = join(source, "story", "snapshots", String(number - 1));
      await mkdir(join(snapshot, "state"), { recursive: true });
      await writeFile(join(snapshot, "current_state.md"), `state-${number - 1}`);
      await writeFile(join(snapshot, "pending_hooks.md"), `hooks-${number - 1}`);
      await writeFile(join(snapshot, "state", "manifest.json"), JSON.stringify({ schemaVersion: 2, language: "zh", lastAppliedChapter: number - 1, projectionVersion: 1 }));
    }
    await writeFile(join(source, "chapters", "index.json"), JSON.stringify(index));
    await writeFile(join(source, "story", "current_state.md"), "state-3");
    return source;
  }

  const json = (body: unknown) => ({ method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });

  describe("book-level: /api/v1/trash/books", () => {
    it("lists a deleted book and brings it back with its chapters", async () => {
      const root = await tempRoot();
      await seedBook(root, "b1", "回来的书");
      const app = createStudioServer({} as never, root);

      const empty = await app.request("/api/v1/trash/books");
      expect(empty.status).toBe(200);
      expect(await empty.json()).toEqual({ entries: [] });

      expect((await app.request("/api/v1/books/b1", { method: "DELETE" })).status).toBe(200);
      const listed = await (await app.request("/api/v1/trash/books")).json() as {
        entries: { entry: string; bookId: string; title: string; trashedAt: string; fileCount: number; bytes: number }[];
      };
      expect(listed.entries).toHaveLength(1);
      // 清单 + book.json + 三章 + 索引 + 三份快照各三件 + 当前状态。
      expect(listed.entries[0]).toMatchObject({ bookId: "b1", title: "回来的书", fileCount: 16 });
      expect(listed.entries[0]!.entry).toMatch(/^b1--\d{14}$/u);
      expect(listed.entries[0]!.trashedAt).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u);
      expect(listed.entries[0]!.bytes).toBeGreaterThan(100);

      const restored = await app.request("/api/v1/trash/books/restore", json({ entry: listed.entries[0]!.entry }));
      expect(restored.status).toBe(200);
      expect(await restored.json()).toEqual({ ok: true, entry: listed.entries[0]!.entry, bookId: "b1" });
      const library = await (await app.request("/api/v1/books")).json() as { books: { id: string; chaptersWritten: number }[] };
      expect(library.books).toEqual([expect.objectContaining({ id: "b1", title: "回来的书", chaptersWritten: 3 })]);
      expect((await app.request("/api/v1/books/b1")).status).toBe(200);
      expect(await (await app.request("/api/v1/trash/books")).json()).toEqual({ entries: [] });
    });

    it("refuses to restore over a book that exists now; unknown and malformed entries are rejected", async () => {
      const root = await tempRoot();
      await seedBook(root, "b1", "第一版");
      const app = createStudioServer({} as never, root);
      await app.request("/api/v1/books/b1", { method: "DELETE" });
      const entry = ((await (await app.request("/api/v1/trash/books")).json()) as { entries: { entry: string }[] }).entries[0]!.entry;
      // 删掉之后又建了一本同 id 的书。
      await seedBook(root, "b1", "第二版");
      await expect(readFile(join(root, "works", "b1", "source", "book.json"), "utf8")).resolves.toContain("第二版");

      const conflict = await app.request("/api/v1/trash/books/restore", json({ entry }));
      expect(conflict.status).toBe(409);
      expect(await conflict.json()).toMatchObject({ error: { code: "TRASH_TARGET_EXISTS", conflicts: ["works/b1"] } });
      // 被删的那一版还留在回收站里，新书没被动。
      expect(((await (await app.request("/api/v1/trash/books")).json()) as { entries: unknown[] }).entries).toHaveLength(1);
      await expect(readFile(join(root, "works", "b1", "source", "book.json"), "utf8")).resolves.toContain("第二版");

      const missing = await app.request("/api/v1/trash/books/restore", json({ entry: "nope--20261125093007" }));
      expect(missing.status).toBe(404);
      expect(await missing.json()).toMatchObject({ error: { code: "TRASH_ENTRY_NOT_FOUND" } });
      for (const body of [{ entry: "../b1--20261125093007" }, { entry: "notes.txt" }, {}, { entry: 7 }]) {
        const invalid = await app.request("/api/v1/trash/books/restore", json(body));
        expect(invalid.status).toBe(400);
        expect(await invalid.json()).toMatchObject({ error: { code: "INVALID_TRASH_REQUEST" } });
      }
      expect((await app.request("/api/v1/trash/books/restore", { method: "POST", body: "{not json" })).status).toBe(400);
    });

    it("leaves books deleted by the product engine to the product engine", async () => {
      const root = await tempRoot();
      // 产品引擎删的书：条目根上是 book.json。搬进本腿的书根会是一本坏书。
      const foreign = join(root, ".trash", "books", "r1--20261125093007");
      await mkdir(foreign, { recursive: true });
      await writeFile(join(foreign, "book.json"), JSON.stringify({ id: "r1", title: "产品引擎删的" }));
      const app = createStudioServer({} as never, root);
      expect(await (await app.request("/api/v1/trash/books")).json()).toEqual({ entries: [] });
      expect((await app.request("/api/v1/trash/books/restore", json({ entry: "r1--20261125093007" }))).status).toBe(404);
      expect((await app.request("/api/v1/trash/books/purge", json({ entry: "r1--20261125093007" }))).status).toBe(404);
      await expect(readdir(foreign)).resolves.toEqual(["book.json"]);
    });

    it("purges for real", async () => {
      const root = await tempRoot();
      await seedBook(root, "b1", "要清掉的书");
      const app = createStudioServer({} as never, root);
      await app.request("/api/v1/books/b1", { method: "DELETE" });
      const entry = ((await (await app.request("/api/v1/trash/books")).json()) as { entries: { entry: string }[] }).entries[0]!.entry;
      const purged = await app.request("/api/v1/trash/books/purge", json({ entry }));
      expect(purged.status).toBe(200);
      expect(await purged.json()).toEqual({ ok: true, entry });
      await expect(readdir(join(root, ".trash", "books"))).resolves.toEqual([]);
      expect((await app.request("/api/v1/trash/books/purge", json({ entry }))).status).toBe(404);
    });
  });

  describe("chapter-level: /api/v1/books/:id/trash", () => {
    async function rejectThirdChapter(app: ReturnType<typeof createStudioServer>) {
      const rejected = await app.request("/api/v1/books/b1/chapters/3/reject", { method: "POST" });
      expect(rejected.status).toBe(200);
    }

    it("lists what a reject put in the trash, and brings a chapter file back — into the index too", async () => {
      const root = await tempRoot();
      const source = await seedBook(root, "b1", "驳回过的书");
      const app = createStudioServer({} as never, root);
      expect(await (await app.request("/api/v1/books/b1/trash")).json()).toEqual({ entries: [] });
      await rejectThirdChapter(app);

      const listed = await (await app.request("/api/v1/books/b1/trash")).json() as {
        entries: { path: string; originalPath: string; batch: string | null; bytes: number; trashedAt: string | null }[];
      };
      const chapter = listed.entries.find((entry) => entry.originalPath === "chapters/0003_收束.md");
      expect(chapter).toMatchObject({ path: expect.stringMatching(/^chapters\/\.trash\/rollback-\d{14}-2\/chapters\/0003_收束\.md$/u), bytes: expect.any(Number) });
      expect(chapter!.batch).toMatch(/^rollback-\d{14}-2$/u);
      // 驳回第 3 章移走的是：第 3 章正文、回滚前的目录、回滚前的当前状态。
      expect(listed.entries.map((entry) => entry.originalPath).sort()).toEqual([
        "chapters/0003_收束.md", "chapters/index.json", "story/current_state.md",
      ]);
      for (const entry of listed.entries) expect(entry.trashedAt).toMatch(/^\d{4}-\d{2}-\d{2}T/u);

      const restored = await app.request("/api/v1/books/b1/trash/restore", json({ path: chapter!.path }));
      expect(restored.status).toBe(200);
      const body = await restored.json() as { ok: boolean; restored: { path: string; originalPath: string; chapter: { number: number; meta: { title: string; status: string } } | null }[] };
      expect(body.ok).toBe(true);
      expect(body.restored).toHaveLength(1);
      expect(body.restored[0]).toMatchObject({
        originalPath: "chapters/0003_收束.md",
        chapter: { number: 3, meta: { title: "收束", status: "approved" } },
      });
      // 文件逐字节回到原位；目录里重新有这一章（带着驳回前的条目）；状态文件一个字节没动。
      await expect(readFile(join(source, "chapters", "0003_收束.md"), "utf8")).resolves.toBe("# 第3章 收束\n\n正文3。");
      const detail = await (await app.request("/api/v1/books/b1")).json() as { chapters: { number: number; status: string }[] };
      expect(detail.chapters.map((meta) => [meta.number, meta.status])).toEqual([[1, "approved"], [2, "approved"], [3, "approved"]]);
      await expect(readFile(join(source, "story", "current_state.md"), "utf8")).resolves.toBe("state-2");
      // 书架照常：找回的这一章不会让这本书 500。
      const library = await (await app.request("/api/v1/books")).json() as { books: { id: string; chaptersWritten: number }[] };
      expect(library.books).toEqual([expect.objectContaining({ id: "b1", chaptersWritten: 3 })]);
      // 不会重复登记：再找回同一章的另一个拷贝，目录还是三条。
      await writeFile(join(source, "chapters", ".trash", "0003_另一份.md"), "# 第3章 另一份\n\n正文。");
      await app.request("/api/v1/books/b1/trash/restore", json({ path: "chapters/.trash/0003_另一份.md" }));
      expect(((await (await app.request("/api/v1/books/b1")).json()) as { chapters: unknown[] }).chapters).toHaveLength(3);
    });

    it("never overwrites what is already in place — a single file or a whole batch", async () => {
      const root = await tempRoot();
      const source = await seedBook(root, "b1", "驳回过的书");
      const app = createStudioServer({} as never, root);
      await rejectThirdChapter(app);
      const listed = await (await app.request("/api/v1/books/b1/trash")).json() as { entries: { path: string; originalPath: string; batch: string | null }[] };
      const state = listed.entries.find((entry) => entry.originalPath === "story/current_state.md")!;

      const conflict = await app.request("/api/v1/books/b1/trash/restore", json({ path: state.path }));
      expect(conflict.status).toBe(409);
      expect(await conflict.json()).toMatchObject({ error: { code: "TRASH_TARGET_EXISTS", conflicts: ["story/current_state.md"] } });
      await expect(readFile(join(source, "story", "current_state.md"), "utf8")).resolves.toBe("state-2");

      const whole = await app.request("/api/v1/books/b1/trash/restore", json({ batch: state.batch }));
      expect(whole.status).toBe(409);
      const wholeBody = await whole.json() as { error: { code: string; conflicts: string[] } };
      expect(wholeBody.error.code).toBe("TRASH_TARGET_EXISTS");
      expect(wholeBody.error.conflicts).toEqual(expect.arrayContaining(["chapters/index.json", "story/current_state.md"]));
      // 一个都没动：第 3 章的文件还在回收站里。
      const after = await (await app.request("/api/v1/books/b1/trash")).json() as { entries: { path: string }[] };
      expect(after.entries.map((entry) => entry.path)).toEqual(listed.entries.map((entry) => entry.path));
      await expect(readFile(join(source, "chapters", "0003_收束.md"), "utf8")).rejects.toMatchObject({ code: "ENOENT" });
    });

    it("purges one entry or the whole trash, and refuses anything that is not a trash entry", async () => {
      const root = await tempRoot();
      const source = await seedBook(root, "b1", "驳回过的书");
      const app = createStudioServer({} as never, root);
      await rejectThirdChapter(app);
      const listed = await (await app.request("/api/v1/books/b1/trash")).json() as { entries: { path: string }[] };

      const one = await app.request("/api/v1/books/b1/trash/purge", json({ path: listed.entries[0]!.path }));
      expect(one.status).toBe(200);
      expect(await one.json()).toEqual({ ok: true, removed: 1 });

      for (const body of [{ path: "chapters/0001_风起.md" }, { path: "chapters/.trash/../0001_风起.md" }, { batch: ".." }, { path: "x", all: true }, {}, { all: "yes" }]) {
        const invalid = await app.request("/api/v1/books/b1/trash/purge", json(body));
        expect(invalid.status).toBe(400);
        expect(await invalid.json()).toMatchObject({ error: { code: "INVALID_TRASH_REQUEST" } });
      }
      expect((await app.request("/api/v1/books/b1/trash/purge", json({ path: "chapters/.trash/0009_没有.md" }))).status).toBe(404);

      const all = await app.request("/api/v1/books/b1/trash/purge", json({ all: true }));
      expect(all.status).toBe(200);
      expect(await all.json()).toEqual({ ok: true, removed: listed.entries.length - 1 });
      await expect(readdir(join(source, "chapters", ".trash"))).resolves.toEqual([]);
      for (const name of ["0001_风起.md", "0002_云涌.md"]) {
        await expect(readFile(join(source, "chapters", name), "utf8")).resolves.toContain("正文");
      }
    });

    it("writes the index before moving files: when the index fails, nothing moves and the answer is TRASH_IO_ERROR", async () => {
      const root = await tempRoot();
      const source = await seedBook(root, "b1", "驳回过的书");
      const app = createStudioServer({} as never, root);
      await rejectThirdChapter(app);
      const listed = await (await app.request("/api/v1/books/b1/trash")).json() as { entries: { path: string; originalPath: string }[] };
      // 目录文件的位置上是一个目录：读不出来（EISDIR），不是「没有目录」。
      await rm(join(source, "chapters", "index.json"));
      await mkdir(join(source, "chapters", "index.json"));

      const failed = await app.request("/api/v1/books/b1/trash/restore", json({
        path: listed.entries.find((entry) => entry.originalPath === "chapters/0003_收束.md")!.path,
      }));
      expect(failed.status).toBe(500);
      expect(await failed.json()).toMatchObject({ error: { code: "TRASH_IO_ERROR" } });
      // 一个都没搬：第 3 章的文件还在回收站里。先搬后写的话这里文件已经回位、目录却不列它。
      await expect(readFile(join(source, "chapters", "0003_收束.md"))).rejects.toMatchObject({ code: "ENOENT" });
      const after = await (await app.request("/api/v1/books/b1/trash")).json() as { entries: { path: string }[] };
      expect(after.entries.map((entry) => entry.path)).toEqual(listed.entries.map((entry) => entry.path));
    });

    it("answers 404 for a book that is not there", async () => {
      const root = await tempRoot();
      const app = createStudioServer({} as never, root);
      const response = await app.request("/api/v1/books/nowhere/trash");
      expect(response.status).toBe(404);
      expect(await response.json()).toEqual({ error: 'Book "nowhere" not found' });
    });
  });
});
