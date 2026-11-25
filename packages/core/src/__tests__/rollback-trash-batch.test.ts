import { chmod, mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, relative, sep } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createInitialWorkManifestWrite } from "../harness/source-sync.js";
import { StateManager } from "../state/manager.js";
import { ROLLBACK_TRUTH_FILES, reserveRollbackBatch, trashBatchName, trashBatchWrites, trashBook, trashedBookName } from "../state/trash.js";

// R58/1676 号：破坏性操作护栏——回滚与删书不再硬删，被废内容进回收站。
// 与产品引擎同一份夹具形态（engine-rs/src/state/manager.rs 的 rollback_fixture）：
// 对跑用书在两腿上各回滚一次，钉两端回收站条目集一致。

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function tempRoot(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "inkos-trash-"));
  roots.push(root);
  return root;
}

/** 目录下的全部文件：相对路径（POSIX 分隔）→ 字节。 */
async function tree(root: string, dir = root): Promise<Record<string, Buffer>> {
  const out: Record<string, Buffer> = {};
  for (const entry of await readdir(dir, { withFileTypes: true }).catch(() => [])) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) Object.assign(out, await tree(root, path));
    else out[relative(root, path).split(sep).join("/")] = await readFile(path);
  }
  return out;
}

async function write(path: string, content: string | Uint8Array): Promise<void> {
  await mkdir(join(path, ".."), { recursive: true });
  await writeFile(path, content);
}

/** 三章（三种命名形态）、草稿、快照 0–3、runtime 工件、真相文件、结构化状态、sqlite 加速索引；索引落盘。 */
async function rollbackFixture(manager: StateManager): Promise<string> {
  const book = manager.bookDir("rb");
  for (const [name, content] of [["0001_风起.md", "c1"], ["0002-云涌.md", "c2"], ["0003.md", "c3"]]) {
    await write(join(book, "chapters", name!), content!);
  }
  await write(join(book, "story", "drafts", "0002_云涌.md"), "d2");
  await write(join(book, "story", "drafts", "0003.md"), "d3");
  for (let n = 0; n <= 3; n += 1) {
    const snapshot = join(book, "story", "snapshots", String(n));
    await write(join(snapshot, "current_state.md"), `state-${n}`);
    await write(join(snapshot, "pending_hooks.md"), `hooks-${n}`);
    await write(join(snapshot, "chapter_summaries.md"), `summaries-${n}`);
    await write(join(snapshot, "state", "manifest.json"), `manifest-${n}`);
  }
  for (const name of ["chapter-01.trace.json", "chapter-02.trace.json", "chapter-03.context.json"]) {
    await write(join(book, "story", "runtime", name), name);
  }
  for (const [name, content] of [
    ["current_state.md", "state-3"], ["pending_hooks.md", "hooks-3"],
    ["chapter_summaries.md", "summaries-3"], ["particle_ledger.md", "ledger-3"],
  ]) {
    await write(join(book, "story", name!), content!);
  }
  await write(join(book, "story", "state", "manifest.json"), "manifest-3");
  await write(join(book, "story", "state", "extra.json"), "extra-3");
  await write(join(book, "story", "memory.db"), "db");
  await write(join(book, "story", "memory.db-wal"), "wal");
  const index = await manager.loadChapterIndex("rb");
  expect(index.map((meta) => meta.number)).toEqual([1, 2, 3]);
  await manager.saveChapterIndex("rb", index);
  return book;
}

/** 回滚到第 1 章之后仍在原位的文件（目标章及之前的产物、快照 0 与 1）。 */
const STILL_LIVE_AFTER_ROLLBACK_TO_1 = [
  "chapters/0001_风起.md",
  "story/runtime/chapter-01.trace.json",
  ...["chapter_summaries.md", "current_state.md", "pending_hooks.md", "state/manifest.json"].map((file) => `story/snapshots/0/${file}`),
  ...["chapter_summaries.md", "current_state.md", "pending_hooks.md", "state/manifest.json"].map((file) => `story/snapshots/1/${file}`),
];

const sorted = (values: string[]) => [...values].sort();

async function onlyTrashBatch(book: string): Promise<string> {
  const batches = await readdir(join(book, "chapters", ".trash"));
  expect(batches).toHaveLength(1);
  expect(batches[0]).toMatch(/^rollback-\d{14}-1$/);
  return join(book, "chapters", ".trash", batches[0]!);
}

describe("rollback keeps discarded content in the trash", () => {
  it("moves discarded chapters, drafts, snapshots, runtime artifacts, truth files, state and the index into one batch", async () => {
    const manager = new StateManager(await tempRoot());
    const book = await rollbackFixture(manager);
    const before = await tree(book);

    expect([...(await manager.rollbackToChapter("rb", 1))].sort()).toEqual([2, 3]);

    const batch = await onlyTrashBatch(book);
    const after = await tree(book);
    const trashed = await tree(batch);
    for (const kept of STILL_LIVE_AFTER_ROLLBACK_TO_1) {
      expect(after[kept]?.equals(before[kept]!), kept).toBe(true);
    }
    // 批次里恰是「回滚前有、回滚后不再原样在原位」的那些（派生物除外），逐字节等于回滚前。
    const expected = sorted(Object.keys(before).filter((rel) =>
      !STILL_LIVE_AFTER_ROLLBACK_TO_1.includes(rel) && !rel.startsWith("story/memory.db")));
    expect(sorted(Object.keys(trashed))).toEqual(expected);
    for (const rel of expected) expect(trashed[rel]!.equals(before[rel]!), rel).toBe(true);

    // 状态恢复到快照 1；快照 1 没有的可选摘要与多余的结构化状态文件不在了。
    expect(after["story/current_state.md"]?.toString()).toBe("state-1");
    expect(after["story/pending_hooks.md"]?.toString()).toBe("hooks-1");
    expect(after["story/chapter_summaries.md"]?.toString()).toBe("summaries-1");
    expect(after["story/state/manifest.json"]?.toString()).toBe("manifest-1");
    expect(after["story/state/extra.json"]).toBeUndefined();
    // 派生物直接删，不进回收站。
    expect(after["story/memory.db"]).toBeUndefined();
    expect(after["story/memory.db-wal"]).toBeUndefined();
    expect((await manager.loadChapterIndex("rb")).map((meta) => meta.number)).toEqual([1]);
  });

  it("leaves the book byte-for-byte unchanged when the commit fails", async () => {
    if (process.getuid?.() === 0) return;
    const manager = new StateManager(await tempRoot());
    const book = await rollbackFixture(manager);
    const before = await tree(book);
    // 原子文件集要把 `chapters/` 里的目标挪去备份——目录不可写时挪不动，整个提交回滚。
    const chapters = join(book, "chapters");
    await chmod(chapters, 0o555);
    await expect(manager.rollbackToChapter("rb", 1)).rejects.toThrow();
    await chmod(chapters, 0o755);
    expect(await tree(book)).toEqual(before);
    await expect(readdir(join(book, "chapters", ".trash"))).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("does not touch anything when the target snapshot is missing", async () => {
    const manager = new StateManager(await tempRoot());
    const book = await rollbackFixture(manager);
    await rm(join(book, "story", "snapshots", "1"), { recursive: true });
    const before = await tree(book);
    await expect(manager.rollbackToChapter("rb", 1)).rejects.toThrow('Cannot restore snapshot for chapter 1 in "rb"');
    expect(await tree(book)).toEqual(before);
  });
});

describe("trash naming", () => {
  it("matches the product engine rule (golden, both legs)", () => {
    // 2026-11-25T09:30:07.123Z —— 与 engine-rs state/trash.rs 的 T 同值。
    const t = 1_795_599_007_123;
    expect(trashBatchName(t, 3)).toBe("rollback-20261125093007-3");
    expect(trashedBookName("b-1", t)).toBe("b-1--20261125093007");
  });

  it("keeps book-relative paths, reads bytes as they are and skips what is not there", async () => {
    const book = join(await tempRoot(), "book");
    await write(join(book, "chapters", "0002_x.md"), "x");
    await write(join(book, "story", "snapshots", "2", "a.md"), "a");
    const batch = "chapters/.trash/rollback-20261125093007-1";
    const writes = await trashBatchWrites(book, batch, ["chapters/0002_x.md", "story/snapshots/2", "story/pending_hooks.md"]);
    expect(writes.map((entry) => entry.relativePath).sort()).toEqual([
      join(batch, "chapters", "0002_x.md"),
      join(batch, "story", "snapshots", "2", "a.md"),
    ]);
    expect(Buffer.from(writes[0]!.content as Uint8Array).toString()).toBe("x");
  });

  it("appends a suffix when a batch with the same name already exists", async () => {
    const book = join(await tempRoot(), "book");
    await write(join(book, "chapters", ".trash", "rollback-20261125093007-1", "keep.md"), "keep");
    expect(await reserveRollbackBatch(book, 1_795_599_007_123, 1)).toBe(join("chapters", ".trash", "rollback-20261125093007-1-2"));
    expect(await reserveRollbackBatch(book, 1_795_599_007_123, 2)).toBe(join("chapters", ".trash", "rollback-20261125093007-2"));
  });

  it("lists exactly the truth files the restore may overwrite or delete", () => {
    expect([...ROLLBACK_TRUTH_FILES]).toEqual([
      "current_state.md", "pending_hooks.md", "chapter_summaries.md", "particle_ledger.md",
      "subplot_board.md", "emotional_arcs.md", "character_matrix.md",
    ]);
  });
});

describe("deleting a book", () => {
  async function seedBook(root: string, id: string, profileId = "longform-novel"): Promise<string> {
    const initial = createInitialWorkManifestWrite({ workId: id, title: id, profileId, language: "zh", writes: [] });
    const workDir = join(root, "works", id);
    await write(join(root, initial.write.relativePath), initial.write.content);
    await write(join(workDir, "source", "book.json"), "{}");
    await write(join(workDir, "source", "chapters", "0001_风起.md"), "c1");
    await write(join(workDir, "source", "chapters", ".trash", "0000_old.md"), "old");
    await write(join(workDir, "exports", "b1.epub"), "epub");
    return workDir;
  }

  it("moves the whole work directory into the project trash and the book leaves the library", async () => {
    const root = await tempRoot();
    const workDir = await seedBook(root, "b1");
    const before = await tree(workDir);

    expect(await trashBook(root, "b1")).toBe("trashed");
    await expect(readdir(workDir)).rejects.toMatchObject({ code: "ENOENT" });
    const entries = await readdir(join(root, ".trash", "books"));
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatch(/^b1--\d{14}$/);
    // 回收站在 works 根之外：书从作品库里消失，清单也在回收站里。
    expect(await tree(join(root, ".trash", "books", entries[0]!))).toEqual(before);
    expect(await new StateManager(root).listBooks()).toEqual([]);
    expect(await readdir(join(root, "works"))).toEqual([]);

    // 书已不在：照旧不报错，不多出回收站条目。
    expect(await trashBook(root, "b1")).toBe("missing");
    expect(await readdir(join(root, ".trash", "books"))).toHaveLength(1);
  });

  it("refuses works that are not books", async () => {
    const root = await tempRoot();
    const workDir = await seedBook(root, "world-1", "interactive-world");
    await expect(trashBook(root, "world-1")).rejects.toMatchObject({ code: "NOT_A_BOOK" });
    expect((await tree(workDir))["source/book.json"]).toBeDefined();
  });

  it("still handles an orphan source directory without a manifest", async () => {
    const root = await tempRoot();
    const workDir = join(root, "works", "orphan");
    await write(join(workDir, "source", "book.json"), "{}");
    expect(await trashBook(root, "orphan")).toBe("trashed");
    const [entry] = await readdir(join(root, ".trash", "books"));
    expect(await readdir(join(root, ".trash", "books", entry!))).toEqual(["source"]);
    await expect(readdir(join(workDir, "source"))).rejects.toMatchObject({ code: "ENOENT" });
  });
});
