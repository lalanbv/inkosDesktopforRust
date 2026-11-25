import { mkdir, mkdtemp, readdir, readFile, rm, symlink, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { ChapterMetaSchema, type ChapterMeta } from "../models/chapter.js";
import { createInitialWorkManifestWrite } from "../harness/source-sync.js";
import { StateManager } from "../state/manager.js";
import { chapterMetaFromFile } from "../state/chapter-meta.js";
import {
  TrashError,
  bookIdOfTrashEntry,
  listBookTrash,
  listChapterTrash,
  originalPathOfTrashEntry,
  purgeBookTrash,
  purgeChapterTrash,
  restoreBookFromTrash,
  restoreChapterTrash,
  trashedAtOfTrashEntry,
} from "../state/trash.js";

// 1679 号（R60）：回收站只进不出——此前两腿都没有列出 / 恢复 / 清空的面。
// 这里测 Node 腿 core 的一半：条目识别（两腿共用向量）、书级与章级的列出 / 恢复 / 清空。
// 恢复的底线：目标已在原位 → 409 不覆盖；找回章文件时目录里缺这一章就补一条，而且是
// 先写目录后搬文件（否则进度守卫把它判成孤儿，整本书在书架接口上 500）；状态与快照不动。

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function tempRoot(prefix = "inkos-trash-api-"): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), prefix));
  roots.push(root);
  return root;
}

async function write(path: string, content: string): Promise<void> {
  await mkdir(join(path, ".."), { recursive: true });
  await writeFile(path, content);
}

/** 不登记目录的直接找回（目录登记另测）。 */
const restore = (book: string, target: Parameters<typeof restoreChapterTrash>[1]) =>
  restoreChapterTrash(book, target, async () => undefined);

const STAMP = "2026-08-17T00:00:00.000Z";
const meta = (number: number, title: string, status: ChapterMeta["status"] = "ready-for-review", wordCount = 3): ChapterMeta => ({
  number, title, status, wordCount, createdAt: STAMP, updatedAt: STAMP, auditIssues: [], lengthWarnings: [],
});

async function tree(root: string, dir = root): Promise<string[]> {
  const out: string[] = [];
  for (const entry of await readdir(dir, { withFileTypes: true }).catch(() => [])) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...await tree(root, path));
    else out.push(path.slice(root.length + 1));
  }
  return out.sort();
}

const refused = (code: string) => (error: unknown) => error instanceof TrashError && error.code === code;

const vectors = JSON.parse(await readFile(
  new URL("../../../studio/src/__tests__/golden/trash-entry-vectors.json", import.meta.url),
  "utf8",
)) as {
  originalPath: { accept: { path: string; originalPath: string; batch: string | null }[]; reject: string[] };
  bookEntry: { accept: { entry: string; bookId: string; trashedAt: string }[]; reject: string[] };
};

describe("trash entry recognition (shared vectors, both legs)", () => {
  it("maps a chapter trash path back to where the file came from", () => {
    for (const { path, originalPath, batch } of vectors.originalPath.accept) {
      expect(originalPathOfTrashEntry(path), path).toEqual({ originalPath, batch });
    }
    for (const path of vectors.originalPath.reject) {
      expect(originalPathOfTrashEntry(path), JSON.stringify(path)).toBeNull();
    }
  });

  it("reads the book id and the time out of a book trash entry name", () => {
    for (const { entry, bookId, trashedAt } of vectors.bookEntry.accept) {
      expect(bookIdOfTrashEntry(entry), entry).toBe(bookId);
      expect(trashedAtOfTrashEntry(entry), entry).toBe(trashedAt);
    }
    for (const entry of vectors.bookEntry.reject) {
      expect(bookIdOfTrashEntry(entry), JSON.stringify(entry)).toBeNull();
    }
  });
});

describe("book-level trash", () => {
  /** 把一份真实的作品清单写进 `dir`（标题取 `title`）。 */
  async function manifestAt(dir: string, id: string, title: string): Promise<string | Uint8Array> {
    const initial = createInitialWorkManifestWrite({ workId: id, title, profileId: "longform-novel", language: "zh", writes: [] });
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, "work.json"), initial.write.content);
    return initial.write.content;
  }

  /** 两腿共用 `.trash/books/`：放一本 Node 腿删的书、一本产品引擎删的书、一份孤儿目录、一个杂散文件。 */
  async function seed(root: string): Promise<void> {
    const trash = join(root, ".trash", "books");
    await manifestAt(join(trash, "b1--20261125093001"), "b1", "被删的书");
    await write(join(trash, "b1--20261125093001", "source", "book.json"), "{}");
    await write(join(trash, "b1--20261125093001", "source", "chapters", "0001_风起.md"), "正文");
    // 产品引擎删的书：根上是 book.json。Node 腿不认。
    await write(join(trash, "r1--20261125093002", "book.json"), JSON.stringify({ id: "r1", title: "产品引擎删的" }));
    // 没有清单、只剩 source/ 的孤儿目录也是 Node 腿的形态。
    await write(join(trash, "b2--20261125093003-2", "source", "book.json"), JSON.stringify({ id: "b2", title: "孤儿" }));
    await manifestAt(join(trash, "b1--20261125093007-3"), "b1", "第二次删的");
    await write(join(trash, "notes.txt"), "不是条目");
  }

  it("lists only this leg's entries, newest first, with size and title", async () => {
    const root = await tempRoot();
    await seed(root);
    const entries = await listBookTrash(root);
    expect(entries.map((entry) => entry.entry)).toEqual([
      "b1--20261125093007-3",
      "b2--20261125093003-2",
      "b1--20261125093001",
    ]);
    // 字节数 = 清单 + book.json + 章正文（UTF-8）；清单里的标题是「被删的书」。
    const manifest = await readFile(join(root, ".trash", "books", "b1--20261125093007-3", "work.json"), "utf8");
    expect(entries[2]).toEqual({
      entry: "b1--20261125093001",
      bookId: "b1",
      trashedAt: "2026-11-25T09:30:01.000Z",
      bytes: Buffer.byteLength(manifest.replace("第二次删的", "被删的书")) + 2 + Buffer.byteLength("正文"),
      fileCount: 3,
      title: "被删的书",
    });
    // 没有清单时标题回落到 source/book.json。
    expect(entries[1]!.title).toBe("孤儿");
  });

  it("moves a deleted book back into the library", async () => {
    const root = await tempRoot();
    await seed(root);
    expect(await restoreBookFromTrash(root, "b1--20261125093001")).toEqual({ bookId: "b1" });
    expect(await tree(join(root, "works", "b1"))).toEqual(["source/book.json", "source/chapters/0001_风起.md", "work.json"]);
    await expect(readdir(join(root, ".trash", "books"), { withFileTypes: true }).then((names) => names.filter((e) => e.name === "b1--20261125093001"))).resolves.toEqual([]);
    expect(await new StateManager(root).listBooks()).toEqual(["b1"]);
  });

  it("refuses to restore over a book that exists now, and leaves the entry in place", async () => {
    const root = await tempRoot();
    await seed(root);
    await write(join(root, "works", "b1", "work.json"), "{}");
    const before = await tree(join(root, ".trash", "books"));
    await expect(restoreBookFromTrash(root, "b1--20261125093001")).rejects.toSatisfy(refused("TRASH_TARGET_EXISTS"));
    expect(await tree(join(root, ".trash", "books"))).toEqual(before);
  });

  it("does not know entries deleted by the other leg, nor malformed names", async () => {
    const root = await tempRoot();
    await seed(root);
    await expect(restoreBookFromTrash(root, "r1--20261125093002")).rejects.toSatisfy(refused("TRASH_ENTRY_NOT_FOUND"));
    await expect(restoreBookFromTrash(root, "b9--20261125093001")).rejects.toSatisfy(refused("TRASH_ENTRY_NOT_FOUND"));
    await expect(restoreBookFromTrash(root, "../b1--20261125093001")).rejects.toSatisfy(refused("INVALID_TRASH_REQUEST"));
    await expect(purgeBookTrash(root, "notes.txt")).rejects.toSatisfy(refused("INVALID_TRASH_REQUEST"));
    expect(await tree(join(root, ".trash", "books"))).toContain("r1--20261125093002/book.json");
  });

  it("purges for real — only the named entry", async () => {
    const root = await tempRoot();
    await seed(root);
    expect(await purgeBookTrash(root, "b1--20261125093007-3")).toEqual({ entry: "b1--20261125093007-3" });
    const after = await tree(join(root, ".trash", "books"));
    expect(after).not.toContain("b1--20261125093007-3/work.json");
    expect(after).toContain("b1--20261125093001/work.json");
    expect(after).toContain("r1--20261125093002/book.json");
    await expect(purgeBookTrash(root, "r1--20261125093002")).rejects.toSatisfy(refused("TRASH_ENTRY_NOT_FOUND"));
    await expect(purgeBookTrash(root, "b1--20261125093007-3")).rejects.toSatisfy(refused("TRASH_ENTRY_NOT_FOUND"));
  });
});

describe("chapter-level trash", () => {
  const BATCH = "rollback-20261125093007-1";

  /** 一本书：第 1 章在位；回收站里有平铺的两份第 2 章、一个回滚批次（第 3 章 + 快照 + 回滚前的目录）。 */
  async function seed(root: string): Promise<string> {
    const book = join(root, "works", "b1", "source");
    await write(join(book, "chapters", "0001_风起.md"), "# 第1章 风起\n\n正文。");
    await write(join(book, "chapters", "index.json"), JSON.stringify([meta(1, "风起")]));
    await write(join(book, "chapters", ".trash", "0002_云涌.md"), "# 第2章 云涌\n\n第一份。");
    await write(join(book, "chapters", ".trash", "0002_云涌-2.md"), "# 第2章 云涌\n\n第二份。");
    await write(join(book, "chapters", ".trash", BATCH, "chapters", "0003.md"), "# 第3章 收束\n\n正文三。");
    await write(join(book, "chapters", ".trash", BATCH, "chapters", "index.json"), JSON.stringify([meta(1, "风起"), meta(2, "云涌"), meta(3, "收束", "approved", 5)]));
    await write(join(book, "chapters", ".trash", BATCH, "story", "snapshots", "2", "current_state.md"), "state-2");
    // 回滚之后原位还在的：当前状态与第 1 章的快照。
    await write(join(book, "story", "current_state.md"), "state-1");
    return book;
  }

  it("lists flat entries and batch entries with where they came from", async () => {
    const root = await tempRoot();
    const book = await seed(root);
    const entries = await listChapterTrash(book);
    const bytes = (text: string) => Buffer.byteLength(text);
    expect(entries.map((entry) => [entry.path, entry.originalPath, entry.batch, entry.bytes])).toEqual([
      ["chapters/.trash/0002_云涌-2.md", "chapters/0002_云涌.md", null, bytes("# 第2章 云涌\n\n第二份。")],
      ["chapters/.trash/0002_云涌.md", "chapters/0002_云涌.md", null, bytes("# 第2章 云涌\n\n第一份。")],
      [`chapters/.trash/${BATCH}/chapters/0003.md`, "chapters/0003.md", BATCH, bytes("# 第3章 收束\n\n正文三。")],
      [`chapters/.trash/${BATCH}/chapters/index.json`, "chapters/index.json", BATCH, expect.any(Number)],
      [`chapters/.trash/${BATCH}/story/snapshots/2/current_state.md`, "story/snapshots/2/current_state.md", BATCH, 7],
    ]);
    // 平铺的条目刚进回收站（按文件修改时间排在前面）；它们没有「删掉的时刻」。批次里的
    // 条目时间是批次名里的时间，批次内按路径排。
    expect(entries[0]).toMatchObject({ trashedAt: null });
    expect(typeof entries[0]!.modifiedAt).toBe("string");
    for (const entry of entries.slice(2)) expect(entry.trashedAt).toBe("2026-11-25T09:30:07.000Z");
    await expect(readFile(join(book, entries[0]!.path), "utf8")).resolves.toBe("# 第2章 云涌\n\n第二份。");
  });

  it("brings a flat chapter file back under its original name and tells what the index should gain", async () => {
    const root = await tempRoot();
    const book = await seed(root);
    const restored = await restore(book, { path: "chapters/.trash/0002_云涌-2.md" });
    expect(restored).toMatchObject({
      restored: [{ path: "chapters/.trash/0002_云涌-2.md", originalPath: "chapters/0002_云涌.md", chapter: { number: 2 } }],
    });
    expect(restored.restored[0]!.chapter?.meta).toMatchObject({ number: 2, title: "云涌", status: "ready-for-review" });
    await expect(readFile(join(book, "chapters", "0002_云涌.md"), "utf8")).resolves.toBe("# 第2章 云涌\n\n第二份。");
    await expect(readFile(join(book, "chapters", ".trash", "0002_云涌-2.md"), "utf8")).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("carries the chapter's pre-rollback index entry along when it comes out of a batch", async () => {
    const root = await tempRoot();
    const book = await seed(root);
    const restored = await restore(book, { path: `chapters/.trash/${BATCH}/chapters/0003.md` });
    expect(restored.restored[0]!.chapter?.meta).toEqual(meta(3, "收束", "approved", 5));
    // 非章文件没有目录条目可带。
    const snapshot = await restore(book, { path: `chapters/.trash/${BATCH}/story/snapshots/2/current_state.md` });
    expect(snapshot.restored).toEqual([{ path: `chapters/.trash/${BATCH}/story/snapshots/2/current_state.md`, originalPath: "story/snapshots/2/current_state.md", chapter: null }]);
  });

  it("never overwrites: a file already back in place makes the restore fail and nothing moves", async () => {
    const root = await tempRoot();
    const book = await seed(root);
    await expect(restore(book, { path: `chapters/.trash/${BATCH}/chapters/index.json` })).rejects.toMatchObject({
      code: "TRASH_TARGET_EXISTS",
      conflicts: ["chapters/index.json"],
    });
    await expect(readFile(join(book, "chapters", "index.json"), "utf8")).resolves.toBe(JSON.stringify([meta(1, "风起")]));
    await expect(readFile(join(book, "chapters", ".trash", BATCH, "chapters", "index.json"), "utf8")).resolves.toContain("收束");
  });

  it("restores a whole batch only when every file can go back", async () => {
    const root = await tempRoot();
    const book = await seed(root);
    // 回滚前的目录与第 2 章快照的原位……快照可以回去，目录回不去（原位有回滚后的目录）。
    const attempt = restore(book, { batch: BATCH });
    await expect(attempt).rejects.toSatisfy((error: unknown) =>
      error instanceof TrashError && error.code === "TRASH_TARGET_EXISTS"
        && JSON.stringify(error.conflicts) === JSON.stringify(["chapters/index.json"]));
    await expect(readFile(join(book, "chapters", ".trash", BATCH, "chapters", "0003.md"), "utf8")).resolves.toContain("正文三");
    await expect(readFile(join(book, "chapters", ".trash", BATCH, "story", "snapshots", "2", "current_state.md"), "utf8")).resolves.toBe("state-2");

    // 一个没有冲突的批次：全部回去。
    await write(join(book, "chapters", ".trash", "rollback-20261125093008-2", "story", "drafts", "0004.md"), "草稿");
    await write(join(book, "chapters", ".trash", "rollback-20261125093008-2", "story", "drafts", "0005.md"), "草稿五");
    const restored = await restore(book, { batch: "rollback-20261125093008-2" });
    expect(restored.restored.map((entry) => entry.originalPath)).toEqual(["story/drafts/0004.md", "story/drafts/0005.md"]);
    await expect(readdir(join(book, "chapters", ".trash", "rollback-20261125093008-2"))).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("rejects paths that are not trash entries, and entries that are not there", async () => {
    const root = await tempRoot();
    const book = await seed(root);
    for (const path of ["chapters/0001_风起.md", "chapters/.trash/../0001_风起.md", "/chapters/.trash/0002_云涌.md", `chapters/.trash/${BATCH}`, `chapters/.trash/${BATCH}/`]) {
      await expect(restore(book, { path }), path).rejects.toSatisfy(refused("INVALID_TRASH_REQUEST"));
    }
    await expect(restore(book, { path: "chapters/.trash/0009.md" })).rejects.toSatisfy(refused("TRASH_ENTRY_NOT_FOUND"));
    await expect(restore(book, { batch: "rollback-20261125093008-9" })).rejects.toSatisfy(refused("TRASH_ENTRY_NOT_FOUND"));
    await expect(restore(book, { batch: "../" })).rejects.toSatisfy(refused("INVALID_TRASH_REQUEST"));
    await expect(restore(book, {})).rejects.toSatisfy(refused("INVALID_TRASH_REQUEST"));
    // 符号链接不是条目：列不出来，也找回不了。
    await symlink(join(root, "outside.md"), join(book, "chapters", ".trash", "0007_链接.md"));
    await write(join(root, "outside.md"), "外面的文件");
    expect((await listChapterTrash(book)).some((entry) => entry.path.includes("0007_链接"))).toBe(false);
    await expect(restore(book, { path: "chapters/.trash/0007_链接.md" })).rejects.toSatisfy(refused("TRASH_ENTRY_NOT_FOUND"));
    await expect(readFile(join(root, "outside.md"), "utf8")).resolves.toBe("外面的文件");
    expect(await tree(join(book, "chapters"))).toContain("0001_风起.md");
  });

  it("registers the chapters before the first file moves, and moves nothing when the index cannot be written", async () => {
    const root = await tempRoot();
    const book = join(root, "works", "b1", "source");
    await write(join(book, "chapters", "index.json"), JSON.stringify([meta(1, "风起")]));
    await write(join(book, "chapters", ".trash", BATCH, "chapters", "0003.md"), "# 第3章 收束\n\n正文三。");
    await write(join(book, "chapters", ".trash", BATCH, "chapters", "0004.md"), "# 第4章\n\n正文四。");
    const before = await tree(book);
    const seen: string[][] = [];
    const diskFull = async (): Promise<void> => {
      seen.push(await tree(join(book, "chapters", ".trash")));
      throw Object.assign(new Error("no space left on device"), { code: "ENOSPC" });
    };
    await expect(restoreChapterTrash(book, { batch: BATCH }, diskFull)).rejects.toMatchObject({ code: "ENOSPC" });
    // 目录没写成：一个文件都没搬。先搬后写的话这里文件已经回位、目录却不列它们。
    expect(await tree(book)).toEqual(before);

    const registered: (readonly ChapterMeta[])[] = [];
    const { restored } = await restoreChapterTrash(book, { batch: BATCH }, async (chapters) => {
      seen.push(await tree(join(book, "chapters", ".trash")));
      registered.push(chapters);
    });
    // 两次调用看到的回收站一模一样：登记发生在任何文件搬走之前。条目按文件现做——裸章号的
    // 文件名里没有标题，标题是「第N章」。
    expect(seen[1]).toEqual(seen[0]);
    expect(registered[0]!.map((entry) => [entry.number, entry.title])).toEqual([[3, "第3章"], [4, "第4章"]]);
    expect(restored.map((entry) => entry.originalPath)).toEqual(["chapters/0003.md", "chapters/0004.md"]);
    await expect(readFile(join(book, "chapters", "0004.md"), "utf8")).resolves.toBe("# 第4章\n\n正文四。");
  });

  it("registers one entry per chapter number when both forms of a chapter come back together", async () => {
    const root = await tempRoot();
    const book = join(root, "works", "b1", "source");
    const batch = "rollback-20261125093009-1";
    await write(join(book, "chapters", "index.json"), JSON.stringify([meta(1, "风起")]));
    // 旧命名的章与重写后的正典章在同一个回滚批次里（批次里没存回滚前的目录）。
    await write(join(book, "chapters", ".trash", batch, "chapters", "0002-旧名.md"), "# 第2章 旧名\n\n旧稿。");
    await write(join(book, "chapters", ".trash", batch, "chapters", "0002_新名.md"), "# 第2章 新名\n\n新稿。");
    const registered: (readonly ChapterMeta[])[] = [];
    const { restored } = await restoreChapterTrash(book, { batch }, async (chapters) => {
      registered.push(chapters);
    });
    // 两个文件都回到原位；目录条目只有一条，跟随读侧会打开的正典文件。
    expect(restored.map((entry) => entry.originalPath)).toEqual(["chapters/0002-旧名.md", "chapters/0002_新名.md"]);
    expect(registered[0]!.map((entry) => [entry.number, entry.title])).toEqual([[2, "新名"]]);
  });

  it("orders entries that share a time by code point, the way the product engine orders them", async () => {
    const root = await tempRoot();
    const book = join(root, "works", "b1", "source");
    const at = new Date("2026-08-17T00:00:00.000Z");
    for (const name of ["0001_开局🔥.md", "0001_开局！.md"]) {
      await write(join(book, "chapters", ".trash", name), "正文");
      await utimes(join(book, "chapters", ".trash", name), at, at);
    }
    // 全角叹号 U+FF01 排在增补平面的 emoji 之前（码点序 = 产品引擎的字节序）；JS 的 `<`
    // 是 UTF-16 码元序，会反过来。
    expect((await listChapterTrash(book)).map((entry) => entry.path)).toEqual([
      "chapters/.trash/0001_开局！.md",
      "chapters/.trash/0001_开局🔥.md",
    ]);
  });

  it("purges only what it listed: symlinks and unknown layouts are left alone", async () => {
    const root = await tempRoot();
    const book = await seed(root);
    await write(join(book, "chapters", ".trash", "rollback-x", "keep.md"), "认不出的布局");
    await symlink(join(root, "outside.md"), join(book, "chapters", ".trash", BATCH, "link.md"));
    await write(join(root, "outside.md"), "外面的文件");
    // 两条平铺 + 批次里的三个条目；符号链接与 `rollback-x` 从没被列出来过。
    expect(await purgeChapterTrash(book, { all: true })).toEqual({ removed: 5 });
    expect(await tree(join(book, "chapters", ".trash"))).toEqual([`${BATCH}/link.md`, "rollback-x/keep.md"]);
    await expect(readFile(join(root, "outside.md"), "utf8")).resolves.toBe("外面的文件");
    expect(await listChapterTrash(book)).toEqual([]);
    expect(await purgeChapterTrash(book, { all: true })).toEqual({ removed: 0 });
  });

  it("purges one entry, one batch, or everything in the trash — and nothing outside it", async () => {
    const root = await tempRoot();
    const book = await seed(root);
    expect(await purgeChapterTrash(book, { path: "chapters/.trash/0002_云涌.md" })).toEqual({ removed: 1 });
    await expect(purgeChapterTrash(book, { path: "chapters/.trash/0002_云涌.md" })).rejects.toSatisfy(refused("TRASH_ENTRY_NOT_FOUND"));
    expect(await purgeChapterTrash(book, { batch: BATCH })).toEqual({ removed: 3 });
    await expect(readdir(join(book, "chapters", ".trash", BATCH))).rejects.toMatchObject({ code: "ENOENT" });
    expect(await purgeChapterTrash(book, { all: true })).toEqual({ removed: 1 });
    expect(await readdir(join(book, "chapters", ".trash"))).toEqual([]);
    expect(await readFile(join(book, "chapters", "0001_风起.md"), "utf8")).toContain("正文");
    for (const target of [{ path: "chapters/0001_风起.md" }, { batch: ".." }, { path: "x", batch: "y" }, {}]) {
      await expect(purgeChapterTrash(book, target as never)).rejects.toSatisfy(refused("INVALID_TRASH_REQUEST"));
    }
  });
});

describe("registering restored chapters in the index", () => {
  it("adds only chapters the index does not list yet, in order, and leaves other entries as they are", async () => {
    const root = await tempRoot();
    const manager = new StateManager(root);
    const book = manager.bookDir("b1");
    await write(join(book, "chapters", "index.json"), JSON.stringify([meta(1, "风起"), meta(3, "原来的第三章", "approved", 9)]));
    await manager.registerRestoredChapters("b1", [meta(2, "云涌"), meta(3, "批次里的第三章")]);
    expect(await manager.loadChapterIndex("b1")).toEqual([meta(1, "风起"), meta(2, "云涌"), meta(3, "原来的第三章", "approved", 9)]);
    // 盘上落了盘。
    expect(JSON.parse(await readFile(join(book, "chapters", "index.json"), "utf8")).map((entry: { number: number }) => entry.number)).toEqual([1, 2, 3]);
  });

  it("keeps one row per chapter number even when handed the same number twice", async () => {
    const root = await tempRoot();
    const manager = new StateManager(root);
    const book = manager.bookDir("b1");
    await write(join(book, "chapters", "index.json"), JSON.stringify([meta(1, "风起")]));
    await manager.registerRestoredChapters("b1", [meta(2, "正典"), meta(2, "旧命名"), meta(4, "四")]);
    // 同号只收第一条（调用方把读侧会打开的那个文件排在前面）。
    expect(await manager.loadChapterIndex("b1")).toEqual([meta(1, "风起"), meta(2, "正典"), meta(4, "四")]);
    await manager.registerRestoredChapters("b1", [meta(2, "又一条")]);
    expect(await manager.loadChapterIndex("b1")).toEqual([meta(1, "风起"), meta(2, "正典"), meta(4, "四")]);
  });
});

describe("chapter meta built from a file", () => {
  it("counts UTF-16 code units outside JS whitespace — the number the product engine computes too", () => {
    // BOM 算空白；emoji 占两个码元：正 文 😀(2) 继 续。
    expect(chapterMetaFromFile("0001_风起.md", "\uFEFF正文😀 \n继续", STAMP)).toMatchObject({
      number: 1, title: "风起", status: "ready-for-review", wordCount: 6, createdAt: STAMP, updatedAt: STAMP,
    });
    expect(chapterMetaFromFile("0002-云_涌.md", "x", STAMP)).toMatchObject({ number: 2, title: "云 涌" });
    expect(chapterMetaFromFile("0003.md", "x", STAMP)).toMatchObject({ number: 3, title: "第3章" });
    expect(chapterMetaFromFile("0000_零.md", "x", STAMP)).toBeNull();
    expect(chapterMetaFromFile("notes.md", "x", STAMP)).toBeNull();
  });
});
