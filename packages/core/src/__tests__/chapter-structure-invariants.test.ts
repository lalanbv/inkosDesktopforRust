import { mkdtempSync, mkdirSync, existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  mergeChapters,
  reorderChapters,
  splitChapter,
  contentFingerprint,
  structureInvariant,
  assertStructureInvariants,
  appendStructureOpsLog,
  undoEligible,
  undoLastStructureOperation,
  type ChapterStructureDeps,
  type ChapterFingerprint,
  type StructureOperationRecord,
} from "../state/chapter-structure.js";
import type { ChapterMeta } from "../models/chapter.js";

// R61 批一/1687 号：结构事务不变量守卫 + 操作日志。施工图（v11 深化二 §2 R61）：
// ①提交前不变量断言（前后指纹清单 → 通过/违反项）；②操作日志 append-only
// （story/structure-ops.jsonl，每行 {ts, opId, kind, before, after, undone}，只有
// 章号/文件名/指纹，无正文）；纯函数双端 golden 同读（含产品引擎 include_str!）。
// 撤销端点与 UI 归批二/三。

const goldenPath = new URL("../../../studio/src/__tests__/golden/structure-invariant-vectors.json", import.meta.url);
const golden = JSON.parse(readFileSync(goldenPath, "utf-8")) as {
  contentFingerprint: ReadonlyArray<{ content: string; fingerprint: string }>;
  structureInvariant: ReadonlyArray<{
    name: string;
    op: { kind: "reorder" } | { kind: "split"; at: number } | { kind: "merge"; removed: number };
    before: ReadonlyArray<ChapterFingerprint>;
    after: ReadonlyArray<ChapterFingerprint>;
    ok: boolean;
    violations: ReadonlyArray<string>;
  }>;
  undoEligible: ReadonlyArray<{
    name: string;
    tail: StructureOperationRecord | null;
    current: ReadonlyArray<ChapterFingerprint>;
    eligible: boolean;
    reason: string | null;
  }>;
};

const dirs: string[] = [];
afterEach(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function seedBook(): { bookDir: string; deps: ChapterStructureDeps } {
  const root = mkdtempSync(join(tmpdir(), "chapter-structure-inv-"));
  dirs.push(root);
  const bookDir = join(root, "works", "b1", "source");
  mkdirSync(join(bookDir, "chapters"), { recursive: true });
  const rows = [
    { number: 1, title: "风起", body: "第一章正文。" },
    { number: 2, title: "灵脉", body: "第二章正文乙。" },
    { number: 3, title: "收束", body: "第三章正文丙。" },
  ];
  for (const row of rows) {
    writeFileSync(
      join(bookDir, "chapters", `${String(row.number).padStart(4, "0")}_${row.title}.md`),
      `# ${row.title}\n\n${row.body}\n`,
    );
  }
  const index: ReadonlyArray<ChapterMeta> = rows.map((row) => ({
    number: row.number,
    title: row.title,
    status: "ready-for-review",
    wordCount: 10,
    createdAt: "2026-11-21T00:00:00.000Z",
    updatedAt: "2026-11-21T00:00:00.000Z",
    auditIssues: [],
    lengthWarnings: [],
  }));
  writeFileSync(join(bookDir, "chapters", "index.json"), JSON.stringify(index));
  const deps: ChapterStructureDeps = {
    bookDir,
    loadChapterIndex: async () =>
      JSON.parse(readFileSync(join(bookDir, "chapters", "index.json"), "utf-8")) as ReadonlyArray<ChapterMeta>,
  };
  return { bookDir, deps };
}

describe("contentFingerprint（golden 真哈希）", () => {
  for (const vector of golden.contentFingerprint) {
    it(`sha256[:16] of ${JSON.stringify(vector.content)}`, () => {
      expect(contentFingerprint(vector.content)).toBe(vector.fingerprint);
    });
  }
});

describe("structureInvariant（golden 判定向量）", () => {
  for (const vector of golden.structureInvariant) {
    it(vector.name, () => {
      const result = structureInvariant(vector.op, vector.before, vector.after);
      expect(result.violations).toEqual(vector.violations);
      expect(result.ok).toBe(vector.ok);
    });
  }
});

describe("assertStructureInvariants / appendStructureOpsLog（提交前守卫 + 操作日志）", () => {
  it("throws naming the missing member when the after set lost a chapter", () => {
    const before: ReadonlyArray<ChapterFingerprint> = [
      { number: 1, fileName: "0001_甲.md", fingerprint: "fp-1" },
      { number: 2, fileName: "0002_乙.md", fingerprint: "fp-2" },
    ];
    const after: ReadonlyArray<ChapterFingerprint> = [
      { number: 1, fileName: "0001_甲.md", fingerprint: "fp-1" },
    ];
    expect(() => assertStructureInvariants({ kind: "reorder" }, before, after, []))
      .toThrowError(/chapter 2/);
  });

  it("appends an undone:false line and returns the record on success", async () => {
    const root = mkdtempSync(join(tmpdir(), "chapter-ops-log-"));
    dirs.push(root);
    const before: ReadonlyArray<ChapterFingerprint> = [
      { number: 1, fileName: "0001_甲.md", fingerprint: "fp-1" },
    ];
    const after: ReadonlyArray<ChapterFingerprint> = [
      { number: 1, fileName: "0001_甲.md", fingerprint: "fp-1" },
    ];
    const record = await appendStructureOpsLog(root, "reorder", before, after, "2026-11-26T00:00:00.000Z");
    expect(record.undone).toBe(false);
    const line = JSON.parse(readFileSync(join(root, "story", "structure-ops.jsonl"), "utf-8")) as {
      ts: string;
      opId: string;
      kind: string;
      undone: boolean;
      before: ReadonlyArray<ChapterFingerprint>;
      after: ReadonlyArray<ChapterFingerprint>;
    };
    expect(line.kind).toBe("reorder");
    expect(line.undone).toBe(false);
    expect(line.ts).toBe("2026-11-26T00:00:00.000Z");
    expect(line.opId).not.toBe("");
    expect(line.before).toEqual(before);
    expect(line.after).toEqual(after);
  });
});

describe("undoEligible（R61 批二 golden 判定向量）", () => {
  for (const vector of golden.undoEligible) {
    it(vector.name, () => {
      const result = undoEligible(vector.tail, vector.current);
      expect(result.reason).toEqual(vector.reason);
      expect(result.eligible).toBe(vector.eligible);
    });
  }
});

describe("undoLastStructureOperation（R61 批二）", () => {
  it("undoes a split: chapters reassemble, log records the undo and marks the original", async () => {
    const { bookDir, deps } = seedBook();
    const originalChapter2 = readFileSync(join(bookDir, "chapters", "0002_灵脉.md"), "utf-8");
    await splitChapter(deps, "b1", 2, 6, undefined);
    const outcome = await undoLastStructureOperation(deps, "b1", undefined);
    expect(outcome.op).toBe("undo");
    // 盘面复原：三章、2 号原文回位、4 号消失。
    expect(existsSync(join(bookDir, "chapters", "0004_灵脉.md"))).toBe(false);
    expect(readFileSync(join(bookDir, "chapters", "0002_灵脉.md"), "utf-8")).toBe(originalChapter2);
    const restored = JSON.parse(readFileSync(join(bookDir, "chapters", "index.json"), "utf-8")) as ReadonlyArray<{ number: number }>;
    expect(restored.map((meta) => meta.number)).toEqual([1, 2, 3]);
    // 日志：两行——undo 行指向原笔，原笔 undone:true。
    const lines = readFileSync(join(bookDir, "story", "structure-ops.jsonl"), "utf-8")
      .split("\n")
      .filter((line) => line.trim() !== "")
      .map((line) => JSON.parse(line) as StructureOperationRecord);
    expect(lines).toHaveLength(2);
    expect(lines[0]!.undone).toBe(true);
    expect(lines[1]!.kind).toBe("undo");
    expect(lines[1]!.undoOf).toBe(lines[0]!.opId);
    expect(lines[1]!.undone).toBe(false);
    expect(lines[1]!.after.map((item) => item.number)).toEqual([1, 2, 3]);
    // 再撤销 → 拒绝（尾笔是 undo 行）。
    await expect(undoLastStructureOperation(deps, "b1", undefined)).rejects.toMatchObject({ status: 409 });
  });

  it("refuses when nothing is logged", async () => {
    const { deps } = seedBook();
    await expect(undoLastStructureOperation(deps, "b1", undefined)).rejects.toMatchObject({
      status: 409,
    });
  });

  it("refuses when the disk drifted after the last operation", async () => {
    const { bookDir, deps } = seedBook();
    await splitChapter(deps, "b1", 2, 6, undefined);
    // 事后编辑：盘面漂移 → after 指纹不符。
    writeFileSync(join(bookDir, "chapters", "0002_灵脉.md"), "# 灵脉\n\n被改过的正文\n");
    await expect(undoLastStructureOperation(deps, "b1", undefined)).rejects.toThrowError(/disk state changed/);
  });

  it("refuses naming the chapter when no restore source matches the target fingerprint", async () => {
    const { bookDir, deps } = seedBook();
    await splitChapter(deps, "b1", 2, 6, undefined);
    // 抹掉来源链：.versions 里拆分归档与 .trash 全部清掉 → 目标指纹无来源。
    rmSync(join(bookDir, "chapters", ".versions"), { recursive: true, force: true });
    await expect(undoLastStructureOperation(deps, "b1", undefined)).rejects.toThrowError(/cannot restore chapter/);
    // 零落盘：盘面保持 split 后状态（守卫抛在提交之前）。
    expect(existsSync(join(bookDir, "chapters", "0004_收束.md"))).toBe(true);
  });
});

describe("三事务成功后落操作日志（R61 批一）", () => {
  it("split writes a structure-ops line with before/after fingerprints", async () => {
    const { bookDir, deps } = seedBook();
    const outcome = await splitChapter(deps, "b1", 2, 6, undefined);
    expect(existsSync(join(bookDir, "story", "structure-ops.jsonl"))).toBe(true);
    const line = JSON.parse(readFileSync(join(bookDir, "story", "structure-ops.jsonl"), "utf-8")) as {
      kind: string;
      undone: boolean;
      before: ReadonlyArray<ChapterFingerprint>;
      after: ReadonlyArray<ChapterFingerprint>;
    };
    expect(line.kind).toBe("split");
    expect(line.undone).toBe(false);
    expect(line.before.map((item) => item.number)).toEqual([1, 2, 3]);
    expect(line.after.map((item) => item.number)).toEqual([1, 2, 3, 4]);
    // before 指纹必须来自操作前的真实内容（拆分后 2 号被截断——before 里仍应是原文指纹）。
    const original2 = contentFingerprint("# 灵脉\n\n第二章正文乙。\n");
    expect(line.before.find((item) => item.number === 2)?.fingerprint).toBe(original2);
    expect(outcome.revision).not.toBe("");
  });

  it("merge writes a structure-ops line", async () => {
    const { bookDir, deps } = seedBook();
    await mergeChapters(deps, "b1", 1, 2, undefined, undefined);
    const line = JSON.parse(readFileSync(join(bookDir, "story", "structure-ops.jsonl"), "utf-8")) as {
      kind: string;
      undone: boolean;
      before: ReadonlyArray<ChapterFingerprint>;
      after: ReadonlyArray<ChapterFingerprint>;
    };
    expect(line.kind).toBe("merge");
    expect(line.undone).toBe(false);
    expect(line.before).toHaveLength(3);
    expect(line.after).toHaveLength(2);
  });

  it("reorder writes a structure-ops line with an unchanged multiset", async () => {
    const { bookDir, deps } = seedBook();
    await reorderChapters(deps, "b1", [3, 1, 2], undefined);
    const line = JSON.parse(readFileSync(join(bookDir, "story", "structure-ops.jsonl"), "utf-8")) as {
      kind: string;
      undone: boolean;
      before: ReadonlyArray<ChapterFingerprint>;
      after: ReadonlyArray<ChapterFingerprint>;
    };
    expect(line.kind).toBe("reorder");
    expect(line.undone).toBe(false);
    expect([...line.before].map((item) => item.fingerprint).sort()).toEqual(
      [...line.after].map((item) => item.fingerprint).sort(),
    );
  });
});
