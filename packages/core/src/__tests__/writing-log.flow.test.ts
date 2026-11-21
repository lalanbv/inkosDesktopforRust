//! R51/1653 号：日产出账本写点活体——编辑保存事务（chapter-replace /
//! chapter-local-edit）wordCount Δ≠0 记账、Δ=0 不记、append 语义。
//! 纯函数面由 golden-writing-goal.test.ts 锁定；端点面由 studio
//! export-routes 同款行面测试覆盖。
import { mkdtemp, mkdir, rm, writeFile, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { it, expect } from "vitest";
import { StateManager } from "../state/manager.js";
import { executeEditTransaction, type EditExecutionDeps } from "../interaction/edit-controller.js";
import { loadWritingLog, writingLogPath } from "../utils/writing-log-store.js";
import { persistChapterArtifacts } from "../pipeline/chapter-persistence.js";
import type { WritingLogRow } from "../utils/writing-goal.js";
import type { ChapterMeta } from "../models/chapter.js";

async function seed(root: string): Promise<StateManager> {
  const state = new StateManager(root);
  const now = "2026-11-22T00:00:00.000Z";
  await state.saveBookConfig("b1", {
    id: "b1", title: "账本书", genre: "general", platform: "other", status: "active",
    targetChapters: 2, chapterWordCount: 1000, language: "zh", createdAt: now, updatedAt: now,
  } as Parameters<StateManager["saveBookConfig"]>[1]);
  await state.saveChapterIndex("b1", [{
    number: 1, title: "风起", status: "drafted", wordCount: 3,
    createdAt: now, updatedAt: now, auditIssues: [], lengthWarnings: [],
  } as Parameters<StateManager["saveChapterIndex"]>[1][number]]);
  const chaptersDir = join(state.bookDir("b1"), "chapters");
  await mkdir(chaptersDir, { recursive: true });
  await writeFile(join(chaptersDir, "0001_风起.md"), "# 第1章\n\n一二三");
  return state;
}

const deps = (state: StateManager): EditExecutionDeps => ({
  bookDir: (bookId) => state.bookDir(bookId),
  loadChapterIndex: (bookId) => state.loadChapterIndex(bookId),
  saveChapterIndex: (bookId, index) => state.saveChapterIndex(bookId, index),
});

it("chapter-replace 记 wordCount 净变化（Δ≠0 一行；Δ=0 不记；append 语义）", async () => {
  const root = await mkdtemp(join(tmpdir(), "inkos-writing-log-"));
  try {
    const state = await seed(root);
    const bookDir = state.bookDir("b1");
    // 初始无账本。
    expect(await loadWritingLog(bookDir)).toEqual([]);

    // ① 3 → 9（“一二三四五六七八九”），Δ=+6 记一行。
    await executeEditTransaction(deps(state), {
      kind: "chapter-replace", bookId: "b1", chapterNumber: 1,
      fullText: "# 第1章 风起\n\n一二三四五六七八九", versionSource: "manual",
    });
    let rows = await loadWritingLog(bookDir);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ chapterNum: 1, before: 3, after: 9 });
    expect(typeof rows[0]!.ts).toBe("string");
    // 落盘路径 = 书级 .inkos/writing-log.jsonl。
    expect(writingLogPath(bookDir).endsWith(join(".inkos", "writing-log.jsonl"))).toBe(true);

    // ② 同长度替换（9 → 9，Δ=0）不记。
    await executeEditTransaction(deps(state), {
      kind: "chapter-replace", bookId: "b1", chapterNumber: 1,
      fullText: "# 第1章 风起\n\n九八七六五四三二一", versionSource: "manual",
    });
    rows = await loadWritingLog(bookDir);
    expect(rows).toHaveLength(1);

    // ③ 再变（9 → 4，Δ=-5）追加第二行。
    await executeEditTransaction(deps(state), {
      kind: "chapter-replace", bookId: "b1", chapterNumber: 1,
      fullText: "# 第1章 风起\n\n一二三四", versionSource: "manual",
    });
    rows = await loadWritingLog(bookDir);
    expect(rows).toHaveLength(2);
    expect(rows[1]).toMatchObject({ chapterNum: 1, before: 9, after: 4 });
    // jsonl 形态（逐行 JSON）。
    const text = await readFile(writingLogPath(bookDir), "utf-8");
    expect(text.trim().split("\n")).toHaveLength(2);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

it("chapter-local-edit 同款写点语义", async () => {
  const root = await mkdtemp(join(tmpdir(), "inkos-writing-log-local-"));
  try {
    const state = await seed(root);
    const bookDir = state.bookDir("b1");
    // “一二三”（3 字）→ “一万三千”（4 字，+1）。
    await executeEditTransaction(deps(state), {
      kind: "chapter-local-edit", bookId: "b1", chapterNumber: 1,
      instruction: "把「一二三」替换为「一万三千」",
      targetText: "一二三", replacementText: "一万三千",
    });
    const rows = await loadWritingLog(bookDir);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ chapterNum: 1, before: 3, after: 4 });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// ── R51/1654 号：管线写章写点（persistChapterArtifacts）─────────────────

const auditResult = { observations: [], summary: "" } as Parameters<typeof persistChapterArtifacts>[0]["auditResult"];

it("persistChapterArtifacts 记 wordCount Δ（Δ≠0 一行；Δ=0 不记；新章 before 0）", async () => {
  const recorded: WritingLogRow[] = [];
  let index: ReadonlyArray<ChapterMeta> = [
    { number: 3, title: "旧章", status: "approved", wordCount: 1000,
      createdAt: "2020-01-01T00:00:00.000Z", updatedAt: "2020-01-01T00:00:00.000Z",
      observations: [], provenance: "generated" } as ChapterMeta,
  ];
  const persist = (finalWordCount: number) => persistChapterArtifacts({
    chapterNumber: 3, chapterTitle: "夜探", auditResult, finalWordCount,
    loadChapterIndex: async () => index,
    saveChapter: async (next) => { index = next; },
    markBookActiveIfNeeded: async () => {},
    now: () => "2026-08-15T00:00:00.000Z",
    appendWritingLog: async (row) => { recorded.push(row); },
  });

  // ① 1000 → 2400：记一行（ts=注入时刻）。
  await persist(2400);
  expect(recorded).toEqual([
    { ts: "2026-08-15T00:00:00.000Z", chapterNum: 3, before: 1000, after: 2400 },
  ]);

  // ② 2400 → 2400（Δ=0）不记。
  await persist(2400);
  expect(recorded).toHaveLength(1);

  // ③ 无 appendWritingLog（缺省可选）零破坏——直调不传钩子。
  const untouched = await persistChapterArtifacts({
    chapterNumber: 4, chapterTitle: "新章", auditResult, finalWordCount: 800,
    loadChapterIndex: async () => [],
    saveChapter: async () => {},
    markBookActiveIfNeeded: async () => {},
  });
  expect(untouched.entry.wordCount).toBe(800);
});
