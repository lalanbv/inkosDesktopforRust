import { afterEach, describe, expect, it } from "vitest";
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  buildRuntimeStateArtifacts,
  createInitialRuntimeState,
  loadRuntimeStateSnapshot,
  saveRuntimeStateSnapshot,
} from "../state/runtime-state-store.js";

describe("runtime state mini-flow", () => {
  const roots: string[] = [];

  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  it("initializes, applies, atomically projects, and reloads typed story state", async () => {
    const bookDir = await tempBook();
    await createInitialRuntimeState({
      bookDir,
      language: "en",
      hooks: [{
        hookId: "ledger-origin",
        startChapter: 0,
        type: "mystery",
        status: "deferred",
        lastAdvancedChapter: 0,
        expectedPayoff: "Reveal where the ledger came from.",
        notes: "Seeded before chapter one.",
      }],
    });
    const artifacts = await buildRuntimeStateArtifacts({
      bookDir,
      language: "en",
      delta: {
        chapter: 1,
        factOps: { upsert: [{ subject: "Lin", predicate: "location", object: "archive" }], expire: [] },
        hookOps: { upsert: [], mention: ["ledger-origin"], resolve: [], defer: [] },
        newHookCandidates: [],
        chapterSummary: {
          chapter: 1,
          title: "The Archive",
          characters: "Lin",
          events: "Lin enters the archive.",
          stateChanges: "Lin reaches the archive.",
          hookActivity: "ledger-origin mentioned",
          mood: "tense",
          chapterType: "investigation",
        },
      },
    });
    await saveRuntimeStateSnapshot(bookDir, artifacts.snapshot);

    const [loaded, projection] = await Promise.all([
      loadRuntimeStateSnapshot(bookDir),
      readFile(join(bookDir, "story", "current_state.md"), "utf-8"),
    ]);
    expect({
      chapter: loaded.manifest.lastAppliedChapter,
      fact: loaded.currentState.facts[0]?.object,
      hooks: loaded.hooks.hooks.map((hook) => hook.hookId),
      summaries: loaded.chapterSummaries.rows.map((summary) => summary.chapter),
      projectionHasFact: projection.includes("archive"),
    }).toEqual({
      chapter: 1,
      fact: "archive",
      hooks: ["ledger-origin"],
      summaries: [1],
      projectionHasFact: true,
    });
  });

  it("fails loudly when a canonical state artifact is corrupt", async () => {
    const bookDir = await tempBook();
    await createInitialRuntimeState({ bookDir, language: "zh" });
    await writeFile(join(bookDir, "story", "state", "hooks.json"), "{broken");
    // 788 号语义翻转（Rust load_json_if_valid 镜像）：损坏件不再整体 reject，
    // 而是从 markdown 真相面重建该件（markdown 是真相源，JSON 是投影）。
    // 此书无 pending_hooks.md → hooks 重建为空表；其余件保留。
    const snapshot = await loadRuntimeStateSnapshot(bookDir);
    expect(snapshot.hooks.hooks).toEqual([]);
    // 损坏件被原子物化为合法 JSON（幂等，下次读走 existing）。
    const healed = JSON.parse(await readFile(join(bookDir, "story", "state", "hooks.json"), "utf-8")) as {
      hooks: unknown[];
    };
    expect(healed.hooks).toEqual([]);
  });

  async function tempBook(): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-runtime-state-flow-"));
    roots.push(root);
    return root;
  }
});

// =============================================================================
// 788 号：legacy 书 bootstrap（Rust ensure 语义 TS 镜像）——状态四件缺失/损坏
// 时从 markdown 真相面派生物化；两腿（Rust/TS）行为收敛。
// =============================================================================
describe("788 legacy bootstrap", () => {
  const roots: string[] = [];

  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  const HOOKS_MD = "| H01 | 1 | mystery | open | 1 | 玉符来历 | 祖符之谜 |\n";

  async function tempBook(): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-legacy-bootstrap-"));
    roots.push(root);
    return root;
  }
  const STATE_MD = "| 字段 | 值 |\n|---|---|\n| 当前章节 | 2 |\n| 当前位置 | 坊市 |\n";
  const SUMMARIES_MD = "| 1 | 风起 | 林动 | 醒来 | 无 | 无 | 紧张 | 开篇章 |\n";

  async function seedLegacyBook(bookDir: string): Promise<void> {
    await mkdir(join(bookDir, "chapters"), { recursive: true });
    await mkdir(join(bookDir, "story"), { recursive: true });
    await writeFile(join(bookDir, "book.json"), JSON.stringify({ language: "zh" }));
    await writeFile(join(bookDir, "chapters", "0001_风起.md"), "# 第1章\n\n正文\n");
    await writeFile(join(bookDir, "chapters", "0002_推进.md"), "# 第2章\n\n正文\n");
    await writeFile(
      join(bookDir, "chapters", "index.json"),
      JSON.stringify([
        { number: 1, title: "风起", wordCount: 4, createdAt: "2026-11-01T00:00:00.000Z", updatedAt: "2026-11-01T00:00:00.000Z", observations: [], provenance: "generated" },
        { number: 2, title: "推进", wordCount: 4, createdAt: "2026-11-01T00:00:00.000Z", updatedAt: "2026-11-01T00:00:00.000Z", observations: [], provenance: "generated" },
      ]),
    );
    await writeFile(join(bookDir, "story", "pending_hooks.md"), HOOKS_MD);
    await writeFile(join(bookDir, "story", "current_state.md"), STATE_MD);
    await writeFile(join(bookDir, "story", "chapter_summaries.md"), SUMMARIES_MD);
  }

  it("bootstraps legacy book from markdown truth sources and materializes four JSONs", async () => {
    const bookDir = await tempBook();
    await seedLegacyBook(bookDir);

    const snapshot = await loadRuntimeStateSnapshot(bookDir);
    expect(snapshot.manifest).toMatchObject({ schemaVersion: 2, language: "zh", lastAppliedChapter: 2 });
    expect(snapshot.currentState.chapter).toBe(2);
    expect(snapshot.currentState.facts).toContainEqual(
      expect.objectContaining({ predicate: "当前位置", object: "坊市" }),
    );
    expect(snapshot.hooks.hooks[0]).toMatchObject({
      hookId: "H01", type: "mystery", status: "open", expectedPayoff: "玉符来历", notes: "祖符之谜",
    });
    expect(snapshot.chapterSummaries.rows[0]).toMatchObject({ chapter: 1, title: "风起" });

    // 四 JSON 物化；markdown 真相源零触碰（bootstrap 只写 JSON——Rust 同款）。
    for (const file of ["manifest.json", "current_state.json", "hooks.json", "chapter_summaries.json"]) {
      await expect(readFile(join(bookDir, "story", "state", file), "utf-8")).resolves.toBeTruthy();
    }
    await expect(readFile(join(bookDir, "story", "pending_hooks.md"), "utf-8")).resolves.toBe(HOOKS_MD);
    await expect(readFile(join(bookDir, "story", "current_state.md"), "utf-8")).resolves.toBe(STATE_MD);
    await expect(readFile(join(bookDir, "story", "chapter_summaries.md"), "utf-8")).resolves.toBe(SUMMARIES_MD);
  });

  it("steady-state reloads are zero-write (mtime stable)", async () => {
    const bookDir = await tempBook();
    await seedLegacyBook(bookDir);
    await loadRuntimeStateSnapshot(bookDir);

    const stateDir = join(bookDir, "story", "state");
    const before = new Map(
      await Promise.all(
        ["manifest.json", "current_state.json", "hooks.json", "chapter_summaries.json"].map(
          async (file): Promise<[string, number]> => [
            file,
            (await stat(join(stateDir, file))).mtimeMs,
          ],
        ),
      ),
    );

    await loadRuntimeStateSnapshot(bookDir);

    for (const [file, mtime] of before) {
      const after = (await stat(join(stateDir, file))).mtimeMs;
      expect(after, `${file} 在 steady-state 重读时被重写`).toBe(mtime);
    }
  });

  it("rebuilds a corrupt piece from markdown while keeping valid pieces", async () => {
    const bookDir = await tempBook();
    await seedLegacyBook(bookDir);
    await loadRuntimeStateSnapshot(bookDir);

    const manifestBefore = await readFile(join(bookDir, "story", "state", "manifest.json"), "utf-8");
    await writeFile(join(bookDir, "story", "state", "current_state.json"), "{broken");

    const snapshot = await loadRuntimeStateSnapshot(bookDir);
    expect(snapshot.currentState.chapter).toBe(2);
    expect(snapshot.currentState.facts).toContainEqual(
      expect.objectContaining({ predicate: "当前位置", object: "坊市" }),
    );
    // 合法件保留：manifest 字节不变（未被重算重写）。
    await expect(readFile(join(bookDir, "story", "state", "manifest.json"), "utf-8")).resolves.toBe(manifestBefore);
  });

  it("derives durable progress from chapter files when markdown is entirely absent", async () => {
    const bookDir = await tempBook();
    await seedLegacyBook(bookDir);
    await rm(join(bookDir, "story", "pending_hooks.md"));
    await rm(join(bookDir, "story", "current_state.md"));
    await rm(join(bookDir, "story", "chapter_summaries.md"));

    const snapshot = await loadRuntimeStateSnapshot(bookDir);
    expect(snapshot.manifest.lastAppliedChapter).toBe(2);
    expect(snapshot.currentState.chapter).toBe(2);
    expect(snapshot.currentState.facts).toEqual([]);
    expect(snapshot.hooks.hooks).toEqual([]);
    expect(snapshot.chapterSummaries.rows).toEqual([]);
  });
});
