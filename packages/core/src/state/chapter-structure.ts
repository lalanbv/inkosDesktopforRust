/**
 * 章节结构操作事务（R48a，1641 号）。
 *
 * Rust 镜像 = engine-rs/src/interaction/chapter_structure.rs（纯函数面 +
 * revision 指纹逐位对齐，golden chapter-structure-vectors.json 锁双端：
 * 重编号/拆分边界/合并分隔/revision hash）。
 *
 * 事务原子性：受影响章文件 + runtime/drafts 产物 + index.json 构造为
 * **单一 commitAtomicFileSet**（writes=新名文件与 index，deletes=旧名与被
 * 删章原路径；rename 语义 = write new + delete old）——两相 journal 保证
 * 失败恢复，零「写一半」窗口。拆/并前归档（structure 来源）先于提交，
 * 失败零落盘。快照（story/snapshots/）与 .versions/ 是冻结历史**不随事务
 * 迁移**（Rust 端同语义）；runtime/chapter-NNNN.* 与 drafts 前导号产物
 * 属当前章工作面**随号迁移**。被删章（合并）正文进 .trash/（永不硬删）。
 *
 * 乐观并发：`chapterSetRevision` = 章集合指纹（sha256 前 16 hex）；写请求
 * 携带 expectedRevision 不符 → 409 CHAPTER_SET_CONFLICT，缺省跳过校验。
 */

import { createHash } from "node:crypto";
import { mkdir, readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import { commitAtomicFileSet } from "../utils/atomic-file-set.js";
import { writeTextAtomic } from "../utils/atomic-write.js";
import type { ChapterMeta } from "../models/chapter.js";
import { archiveChapterVersion } from "./chapter-workspace.js";
import { findChapterFileByNumber } from "../utils/chapter-files.js";

export const REVISION_CONFLICT_CODE = "CHAPTER_SET_CONFLICT";
const REVISION_CONFLICT_MESSAGE = "Chapter set changed since last read (revision conflict).";
const TRASH_DIR = "chapters/.trash";

/** 结构事务错误（status 供端点层映射 HTTP 码）。 */
export class ChapterStructureError extends Error {
  readonly status: number;
  readonly code?: string;

  constructor(status: number, message: string, code?: string) {
    super(message);
    this.name = "ChapterStructureError";
    this.status = status;
    this.code = code;
  }
}

export interface RenumberedEntry {
  readonly from: number;
  readonly to: number;
}

export interface StructureOutcome {
  readonly op: "split" | "merge" | "reorder" | "undo";
  readonly bookId: string;
  readonly splitFrom?: number;
  readonly newChapter?: number;
  readonly mergedInto?: number;
  readonly removedChapter?: number;
  readonly renumbered: ReadonlyArray<RenumberedEntry>;
  readonly revision: string;
  readonly touchedFiles: ReadonlyArray<string>;
  readonly summary: string;
}

// ── 纯函数面（golden 锁双端）─────────────────────────────────────

/** 章集合指纹：`[[number, wordCount, updatedAt], ...]`（number 升序）的
 * JSON 串 sha256 前 16 hex。空索引 → sha256("[]")。与 Rust 逐位一致。 */
export function chapterSetRevision(index: ReadonlyArray<ChapterMeta>): string {
  const numbers = index.map((meta) => meta.number).sort((a, b) => a - b);
  const parts = numbers.map((number) => {
    const meta = index.find((candidate) => candidate.number === number)!;
    return [meta.number, meta.wordCount, meta.updatedAt];
  });
  const payload = JSON.stringify(parts);
  return createHash("sha256").update(payload, "utf-8").digest("hex").slice(0, 16);
}

/** 拆分重编号：`at` 之后全部 +1（新章占 at+1）。返回移位对（from 升序）。 */
export function renumberAfterSplit(
  numbers: ReadonlyArray<number>,
  at: number,
): ReadonlyArray<RenumberedEntry> {
  return numbers
    .filter((n) => n > at)
    .map((n) => ({ from: n, to: n + 1 }));
}

/** 合并重编号：`removed` 之后全部 -1。返回移位对（from 升序）。 */
export function renumberAfterMerge(
  numbers: ReadonlyArray<number>,
  removed: number,
): ReadonlyArray<RenumberedEntry> {
  return numbers
    .filter((n) => n > removed)
    .map((n) => ({ from: n, to: n - 1 }));
}

/** 重排映射：`order`（新序的章号排列）须为当前章号集合的双射。
 * 返回实际移位对（from 升序；恒位章不出现）。 */
export function reorderPlan(
  numbers: ReadonlyArray<number>,
  order: ReadonlyArray<number>,
): ReadonlyArray<RenumberedEntry> {
  const current = new Set(numbers);
  const requested = new Set(order);
  // 双射三重校验：长度相等 + 无重复（Set 去重后大小不变）+ 集合相等——
  // [1,2,2] 形态的重复数在 every(has) 下会漏过（Rust BTreeSet 同语义）。
  if (
    order.length !== current.size
    || requested.size !== order.length
    || !order.every((n) => current.has(n))
  ) {
    throw new ChapterStructureError(
      400,
      "Reorder payload must be a permutation of the current chapter numbers.",
    );
  }
  return order
    .map((old, index) => ({ from: old, to: index + 1 }))
    .filter((entry) => entry.from !== entry.to)
    .sort((a, b) => a.from - b.from);
}

/** 拆分切点（UTF-16 code unit 偏移，对齐前端光标语义）：
 * 返回 (前段, 后段)；offset 落在代理对中间 → 非法。 */
export function splitUtf16(
  content: string,
  offset: number,
): readonly [string, string] {
  const total = content.length;
  if (offset <= 0 || offset >= total) {
    throw new ChapterStructureError(
      400,
      "Split offset must be inside the chapter content.",
    );
  }
  // offset 处是低代理 = 高代理的第二 unit（切在 pair 中间）。
  const nextUnit = content.charCodeAt(offset);
  if (nextUnit >= 0xdc00 && nextUnit <= 0xdfff) {
    throw new ChapterStructureError(
      400,
      "Split offset falls inside a surrogate pair.",
    );
  }
  return [content.slice(0, offset), content.slice(offset)] as const;
}

/** 合并拼接：`a + separator + b`（separator 可为空串）。 */
export function mergeContent(a: string, b: string, separator: string): string {
  return `${a}${separator}${b}`;
}

// ── R61 批一（1687 号）：结构事务不变量守卫 + 操作日志 ─────────────
// 施工图（v11 深化二 §2）：①提交前不变量断言（操作前指纹清单 vs 提交面清单，
// 违反即抛——TS 单提交面下抛在 commitAtomicFileSet 之前=零落盘）；②操作日志
// append-only `{bookDir}/story/structure-ops.jsonl`（每行 {ts, opId, kind,
// before, after, undone}，只有章号/文件名/指纹，无正文；原子写与 writing-log
// 同族）。纯函数双端 golden 同读（golden/structure-invariant-vectors.json，
// 产品引擎 include_str!）。撤销端点与 undoEligible 归批二。

export interface ChapterFingerprint {
  readonly number: number;
  readonly fileName: string;
  readonly fingerprint: string;
}

export type StructureOp =
  | { readonly kind: "reorder" }
  | { readonly kind: "split"; readonly at: number }
  | { readonly kind: "merge"; readonly removed: number };

export interface StructureOperationRecord {
  readonly ts: string;
  readonly opId: string;
  readonly kind: "split" | "merge" | "reorder" | "undo";
  readonly before: ReadonlyArray<ChapterFingerprint>;
  readonly after: ReadonlyArray<ChapterFingerprint>;
  readonly undone: boolean;
  /** split 行：被拆章号（撤销推导逆 op 用；undo 行缺省）。 */
  readonly at?: number;
  /** merge 行：被并章号（同上）。 */
  readonly removed?: number;
  /** undo 行指向被撤销那笔的 opId（其余行缺省）。 */
  readonly undoOf?: string;
}

/** 章内容指纹：UTF-8 sha256 前 16 hex（chapterSetRevision 同族；只在结构操作时计算）。 */
export function contentFingerprint(content: string): string {
  return createHash("sha256").update(content, "utf-8").digest("hex").slice(0, 16);
}

/**
 * 清单级不变量判定（内容级校验——拆分两片拼回、合并拼接——由事务钩子携带内容做）：
 * reorder 全员保指纹；split 被拆号允许变、新号 at+1 出现、其余按 +1 移位保指纹；
 * merge 被并号消失、keep 号（removed-1）允许变、其余按 -1 移位保指纹。
 * 违反项逐条点名（章号 + before 文件名 + 前后指纹）。
 */
export function structureInvariant(
  op: StructureOp,
  before: ReadonlyArray<ChapterFingerprint>,
  after: ReadonlyArray<ChapterFingerprint>,
): { readonly ok: boolean; readonly violations: ReadonlyArray<string> } {
  const violations: string[] = [];
  const expectedCount = op.kind === "split"
    ? before.length + 1
    : op.kind === "merge"
      ? before.length - 1
      : before.length;
  if (after.length !== expectedCount) {
    const expectation = op.kind === "split"
      ? "split must add exactly one"
      : op.kind === "merge"
        ? "merge must remove exactly one"
        : "reorder must keep the count";
    violations.push(`chapter count changed: ${before.length} -> ${after.length} (${expectation})`);
  }
  const afterByNumber = new Map(after.map((item) => [item.number, item]));
  const changed = (item: ChapterFingerprint, counterpart: ChapterFingerprint, action: string): string =>
    `chapter ${item.number} (${item.fileName}) content changed (${item.fingerprint} -> ${counterpart.fingerprint}) but ${action}`;
  if (op.kind === "reorder") {
    // 施工图：重排 = {sha256} 多重集前后相等 + 章数相等（内容只换位不改写）。
    const beforeNumbers = new Set(before.map((item) => item.number));
    const afterNumbers = new Set(after.map((item) => item.number));
    for (const number of beforeNumbers) {
      if (!afterNumbers.has(number)) {
        violations.push(`missing chapter ${number} (${before.find((item) => item.number === number)!.fileName}) has no counterpart in the after set`);
      }
    }
    for (const number of afterNumbers) {
      if (!beforeNumbers.has(number)) {
        violations.push(`unexpected chapter ${number} (${after.find((item) => item.number === number)!.fileName}) appeared in the after set`);
      }
    }
    const beforeCounts = new Map<string, number>();
    for (const item of before) beforeCounts.set(item.fingerprint, (beforeCounts.get(item.fingerprint) ?? 0) + 1);
    const afterCounts = new Map<string, number>();
    for (const item of after) afterCounts.set(item.fingerprint, (afterCounts.get(item.fingerprint) ?? 0) + 1);
    // 迭代按指纹字典序（Rust BTreeMap 同序——多指纹违反的两腿消息逐字一致）。
    for (const fingerprint of [...beforeCounts.keys()].sort()) {
      const count = beforeCounts.get(fingerprint)!;
      if ((afterCounts.get(fingerprint) ?? 0) < count) {
        const owners = before.filter((item) => item.fingerprint === fingerprint)
          .map((item) => `${item.number} (${item.fileName})`).join(", ");
        violations.push(`fingerprint ${fingerprint} (chapter ${owners}) is missing from the after set but reorder must not rewrite any chapter`);
      }
    }
    for (const fingerprint of [...afterCounts.keys()].sort()) {
      const count = afterCounts.get(fingerprint)!;
      if ((beforeCounts.get(fingerprint) ?? 0) < count) {
        const owners = after.filter((item) => item.fingerprint === fingerprint)
          .map((item) => `${item.number} (${item.fileName})`).join(", ");
        violations.push(`fingerprint ${fingerprint} (after chapter ${owners}) matches no before chapter`);
      }
    }
  } else if (op.kind === "split") {
    const expectedNumbers = new Set<number>([op.at, op.at + 1]);
    for (const item of before) {
      if (item.number === op.at) continue; // 被拆章允许变（截断成前段）
      const target = item.number > op.at ? item.number + 1 : item.number;
      expectedNumbers.add(target);
      const counterpart = afterByNumber.get(target);
      if (!counterpart) {
        violations.push(`missing chapter ${target} (${item.fileName}) after split renumbering`);
      } else if (counterpart.fingerprint !== item.fingerprint) {
        violations.push(changed(item, counterpart, "split only renumbers it"));
      }
    }
    for (const item of after) {
      if (!expectedNumbers.has(item.number)) {
        violations.push(`unexpected chapter ${item.number} (${item.fileName}) appeared in the after set`);
      }
    }
  } else {
    const expectedNumbers = new Set<number>();
    for (const item of before) {
      if (item.number === op.removed) continue; // 被并章消失
      const target = item.number > op.removed ? item.number - 1 : item.number;
      expectedNumbers.add(target);
      const counterpart = afterByNumber.get(target);
      if (!counterpart) {
        violations.push(`missing chapter ${target} (${item.fileName}) after merge renumbering`);
      } else if (item.number !== op.removed - 1 && counterpart.fingerprint !== item.fingerprint) {
        // keep 号（removed-1）被合并覆写，允许变。
        violations.push(changed(item, counterpart, "merge only renumbers it"));
      }
    }
    for (const item of after) {
      if (!expectedNumbers.has(item.number)) {
        violations.push(`unexpected chapter ${item.number} (${item.fileName}) survived the merge`);
      }
    }
  }
  return { ok: violations.length === 0, violations };
}

/** 提交前守卫：清单级违反或内容级额外违反非空即抛（调用方保证此刻零落盘）。 */
export function assertStructureInvariants(
  op: StructureOp,
  before: ReadonlyArray<ChapterFingerprint>,
  after: ReadonlyArray<ChapterFingerprint>,
  extraViolations: ReadonlyArray<string>,
): void {
  const { violations } = structureInvariant(op, before, after);
  const all = [...violations, ...extraViolations];
  if (all.length > 0) {
    throw new ChapterStructureError(500, `Structure invariant violated: ${all.join("; ")}`);
  }
}

/** 成功提交后追加操作日志（append-only；读改写整文件 + 原子写，writing-log 同族）。 */
export async function appendStructureOpsLog(
  bookDir: string,
  kind: StructureOperationRecord["kind"],
  before: ReadonlyArray<ChapterFingerprint>,
  after: ReadonlyArray<ChapterFingerprint>,
  ts: string,
  extra?: { readonly undoOf?: string; readonly at?: number; readonly removed?: number },
): Promise<StructureOperationRecord> {
  const record: StructureOperationRecord = {
    ts,
    opId: `${kind}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    kind,
    before,
    after,
    undone: false,
    ...(extra?.undoOf !== undefined ? { undoOf: extra.undoOf } : {}),
    ...(extra?.at !== undefined ? { at: extra.at } : {}),
    ...(extra?.removed !== undefined ? { removed: extra.removed } : {}),
  };
  const path = join(bookDir, "story", "structure-ops.jsonl");
  await mkdir(join(bookDir, "story"), { recursive: true });
  let lines: string[] = [];
  try {
    lines = (await readFile(path, "utf-8")).split("\n").filter((line) => line.trim() !== "");
  } catch {
    // 首笔：文件尚不存在。
  }
  lines.push(JSON.stringify(record));
  await writeTextAtomic(path, `${lines.join("\n")}\n`);
  return record;
}

/** 成功提交后追加操作日志（append-only；读改写整文件 + 原子写，writing-log 同族）。 */
async function loadStructureOpsLog(bookDir: string): Promise<ReadonlyArray<StructureOperationRecord>> {
  try {
    const raw = await readFile(join(bookDir, "story", "structure-ops.jsonl"), "utf-8");
    return raw
      .split("\n")
      .filter((line) => line.trim() !== "")
      .map((line) => JSON.parse(line) as StructureOperationRecord);
  } catch {
    return [];
  }
}

/** 原笔标已撤销（读改写整文件；行按 opId 匹配）。 */
async function markUndone(bookDir: string, opId: string): Promise<void> {
  const rows = await loadStructureOpsLog(bookDir);
  const lines = rows.map((row) =>
    row.opId === opId ? ({ ...row, undone: true } satisfies StructureOperationRecord) : row,
  );
  await writeTextAtomic(
    join(bookDir, "story", "structure-ops.jsonl"),
    `${lines.map((line) => JSON.stringify(line)).join("\n")}\n`,
  );
}

/**
 * 可撤销判定（R61 批二）：当且仅当尾笔存在、undone:false、kind 不是 undo、
 * 且尾笔 after 与当前盘面（号集合 + 指纹多重集）一致——其后没有编辑改写。
 */
export function undoEligible(
  tail: StructureOperationRecord | null,
  current: ReadonlyArray<ChapterFingerprint>,
): { readonly eligible: boolean; readonly reason: string | null } {
  if (!tail) return { eligible: false, reason: "no structure operations logged" };
  if (tail.undone) return { eligible: false, reason: "last operation is already undone" };
  if (tail.kind === "undo") return { eligible: false, reason: "last entry is itself an undo" };
  const sameNumbers = tail.after.length === current.length
    && tail.after.every((item) => current.some((candidate) => candidate.number === item.number))
    && current.every((candidate) => tail.after.some((item) => item.number === candidate.number));
  const multiset = (list: ReadonlyArray<ChapterFingerprint>): ReadonlyArray<string> =>
    list.map((item) => `${item.number}\u0000${item.fingerprint}`).sort();
  if (!sameNumbers || multiset(tail.after).join("|") !== multiset(current).join("|")) {
    return { eligible: false, reason: "disk state changed since the last operation" };
  }
  return { eligible: true, reason: null };
}

/**
 * 撤销上一笔结构事务（R61 批二）：把盘面恢复到尾笔 before 清单。内容来源链=
 * 现盘（指纹配对，reorder 撤销/未动章）→ `.trash`（merge 被删章）→
 * `.versions/<号>/` structure 存档（split/merge 操作前旧文）；三处都没有 →
 * 拒绝并点名章。撤销本身是一笔新结构事务：逆 op（split↔Merge、merge↔Split、
 * reorder↔Reorder）过同一守卫，成功后记 undo 行并把原笔标 undone:true。
 */
export async function undoLastStructureOperation(
  deps: ChapterStructureDeps,
  bookId: string,
  expectedRevision: string | undefined,
): Promise<StructureOutcome> {
  const index = await deps.loadChapterIndex();
  checkRevision(index, expectedRevision);
  const numbers = sortedNumbers(index);
  const rows = await loadStructureOpsLog(deps.bookDir);
  const tail = rows.at(-1) ?? null;
  const current = await collectFingerprints(deps.bookDir, numbers, index);
  const verdict = undoEligible(tail, current);
  if (!verdict.eligible || !tail) {
    throw new ChapterStructureError(409, `Undo refused: ${verdict.reason}`);
  }

  // 逆 op 从日志行的操作参数推导（行携带 at/removed——指纹推导不唯一：
  // split 后 head/tail 两个指纹都不在 before 多重集里，号差集又含移位章）。
  const inverseOp: StructureOp = tail.kind === "reorder"
    ? { kind: "reorder" }
    : tail.kind === "split"
      ? { kind: "merge", removed: (tail.at ?? 0) + 1 }
      : { kind: "split", at: (tail.removed ?? 1) - 1 };

  const files = await collectChapterFiles(deps.bookDir, numbers, index);
  // 当前内容的指纹索引（回位时「内容只搬位置」的来源）。
  const currentByFingerprint = new Map<string, { readonly fileName: string; readonly content: string }>();
  for (const item of current) {
    const file = files.get(item.number);
    if (!file) continue;
    currentByFingerprint.set(item.fingerprint, {
      fileName: file.fileName,
      content: await readFile(join(deps.bookDir, "chapters", file.fileName), "utf-8"),
    });
  }

  // 目标章内容来源链：现盘 → .trash（根文件）→ .versions/<号>/ structure 存档。
  const resolveTarget = async (target: ChapterFingerprint): Promise<string> => {
    const onDisk = currentByFingerprint.get(target.fingerprint);
    if (onDisk) return onDisk.content;
    const trashDir = join(deps.bookDir, TRASH_DIR);
    try {
      for (const name of await readdir(trashDir)) {
        const path = join(trashDir, name);
        if (!name.endsWith(".md")) continue;
        const content = await readFile(path, "utf-8");
        if (contentFingerprint(content) === target.fingerprint) return content;
      }
    } catch {
      // 无 trash。
    }
    const versionsDirPath = join(deps.bookDir, "chapters", ".versions", String(target.number).padStart(4, "0"));
    try {
      for (const name of (await readdir(versionsDirPath)).sort().reverse()) {
        if (!name.endsWith(".md")) continue;
        const content = await readFile(join(versionsDirPath, name), "utf-8");
        if (contentFingerprint(content) === target.fingerprint) return content;
      }
    } catch {
      // 无该号存档。
    }
    throw new ChapterStructureError(
      409,
      `Undo refused: cannot restore chapter ${target.number} (${target.fileName}): no matching content in disk, .trash or .versions`,
    );
  };

  // 目标清单解析（任何一个来源缺失即整体拒绝——守卫抛在提交之前=零落盘）。
  const resolved = new Map<number, string>();
  const missing: string[] = [];
  for (const target of tail.before) {
    try {
      resolved.set(target.number, await resolveTarget(target));
    } catch (error) {
      missing.push(error instanceof ChapterStructureError ? error.message : String(error));
    }
  }
  if (missing.length > 0) {
    throw new ChapterStructureError(409, missing.join("; "));
  }

  // 写面：目标号 → 新名写目标内容；当前同号旧名 ≠ 目标名 → rename 语义删旧名；
  // 当前号 ∉ 目标号集 → 该章内容并入别处（指纹已配对），旧名删除。
  const writes: Array<{ relativePath: string; content: string }> = [];
  const deletes: string[] = [];
  const targetNumbers = new Set(tail.before.map((item) => item.number));
  for (const target of tail.before) {
    const fileName = target.fileName;
    const content = resolved.get(target.number)!;
    const currentFile = files.get(target.number);
    if (!currentFile || currentFile.fileName !== fileName) {
      writes.push({ relativePath: `chapters/${fileName}`, content });
    } else {
      const onDisk = await readFile(join(deps.bookDir, "chapters", fileName), "utf-8");
      if (onDisk !== content) writes.push({ relativePath: `chapters/${fileName}`, content });
    }
    if (currentFile && currentFile.fileName !== fileName) {
      deletes.push(`chapters/${currentFile.fileName}`);
    }
  }
  for (const item of current) {
    if (targetNumbers.has(item.number)) continue;
    const file = files.get(item.number)!;
    deletes.push(`chapters/${file.fileName}`);
  }

  // index 回位：当前行按号落到目标集（title 保留），目标有而当前无的号按
  // before 文件名解析 title 新建行（ReadyForReview 惯例）；当前有而目标无的行删除。
  const titleOf = (fileName: string): string => {
    const stem = fileName.replace(/\.md$/u, "");
    const dash = stem.indexOf("_");
    return dash >= 0 ? stem.slice(dash + 1) : stem;
  };
  const nowIso = new Date().toISOString();
  const nextIndex: ChapterMeta[] = [];
  for (const target of tail.before) {
    const existing = index.find((meta) => meta.number === target.number);
    const content = resolved.get(target.number)!;
    if (existing) {
      nextIndex.push({
        ...existing,
        wordCount: content.replace(/\s+/gu, "").length,
        updatedAt: nowIso,
      });
    } else {
      nextIndex.push({
        number: target.number,
        title: titleOf(target.fileName),
        status: "ready-for-review",
        wordCount: content.replace(/\s+/gu, "").length,
        createdAt: nowIso,
        updatedAt: nowIso,
        auditIssues: [],
        lengthWarnings: [],
      });
    }
  }
  nextIndex.sort((a, b) => a.number - b.number);

  // 守卫（逆 op 过同一清单判定）→ 提交 → 日志（undo 行 + 原笔标已撤销）。
  assertStructureInvariants(inverseOp, current, tail.before, []);

  await commitAtomicFileSet({
    rootDir: deps.bookDir,
    writes: [...writes, { relativePath: "chapters/index.json", content: JSON.stringify(nextIndex) }],
    deletes,
  });
  await appendStructureOpsLog(deps.bookDir, "undo", current, tail.before, nowIso, { undoOf: tail.opId });
  await markUndone(deps.bookDir, tail.opId);

  return {
    op: "undo",
    bookId,
    renumbered: [],
    revision: chapterSetRevision(nextIndex),
    touchedFiles: ["chapters/index.json"],
    summary: `Undid ${tail.kind} (${tail.opId}).`,
  };
}

/**
 * 可撤销状态查询（R61 批三：GET 端点/UI「可撤销上一笔」的装配面）：
 * 读日志尾笔 + 当前盘面 → undoEligible 判定 + 尾笔元数据。
 */
export async function structureUndoStatus(
  deps: ChapterStructureDeps,
): Promise<{
  readonly eligible: boolean;
  readonly reason: string | null;
  readonly lastKind: string | null;
  readonly lastOpId: string | null;
}> {
  const index = await deps.loadChapterIndex();
  const numbers = sortedNumbers(index);
  const current = await collectFingerprints(deps.bookDir, numbers, index);
  const rows = await loadStructureOpsLog(deps.bookDir);
  const tail = rows.at(-1) ?? null;
  const verdict = undoEligible(tail, current);
  return {
    eligible: verdict.eligible,
    reason: verdict.reason,
    lastKind: tail?.kind ?? null,
    lastOpId: tail?.opId ?? null,
  };
}

/** 操作前全书指纹清单（撤销批二的全量回位依据；结构操作低频，逐章重读可承受）。 */
async function collectFingerprints(
  bookDir: string,
  numbers: ReadonlyArray<number>,
  index: ReadonlyArray<ChapterMeta>,
): Promise<ReadonlyArray<ChapterFingerprint>> {
  const files = await collectChapterFiles(bookDir, numbers, index);
  const out: ChapterFingerprint[] = [];
  for (const number of [...numbers].sort((a, b) => a - b)) {
    const file = files.get(number)!;
    const content = await readFile(join(bookDir, "chapters", file.fileName), "utf-8");
    out.push({ number, fileName: file.fileName, fingerprint: contentFingerprint(content) });
  }
  return out;
}

/** 提交面指纹清单：nextIndex 逐行取 planned 写入内容，未动章回读现盘。 */
async function plannedFingerprints(
  bookDir: string,
  nextIndex: ReadonlyArray<ChapterMeta>,
  writes: ReadonlyArray<{ readonly relativePath: string; readonly content: string }>,
): Promise<ReadonlyArray<ChapterFingerprint>> {
  const planned = new Map(writes.map((write) => [write.relativePath, write.content]));
  const out: ChapterFingerprint[] = [];
  for (const meta of [...nextIndex].sort((a, b) => a.number - b.number)) {
    const fileName = chapterFileName(meta.number, meta.title);
    const content = planned.get(`chapters/${fileName}`)
      ?? await readFile(join(bookDir, "chapters", fileName), "utf-8");
    out.push({ number: meta.number, fileName, fingerprint: contentFingerprint(content) });
  }
  return out;
}

// ── 事务 ─────────────────────────────────────────────────────────

export interface ChapterStructureDeps {
  readonly bookDir: string;
  loadChapterIndex(): Promise<ReadonlyArray<ChapterMeta>>;
}

interface ChapterFileMeta {
  readonly number: number;
  readonly meta: ChapterMeta;
  readonly fileName: string;
}

const chapterFileName = (number: number, title: string): string =>
  `${String(number).padStart(4, "0")}_${title}.md`;

const paddedRuntimePrefix = (number: number): string =>
  `chapter-${String(number).padStart(4, "0")}.`;

const parseLeadingNumber = (name: string): number | null => {
  const match = name.match(/^(\d+)/);
  if (!match) return null;
  const value = Number.parseInt(match[1]!, 10);
  return value > 0 ? value : null;
};

async function collectChapterFiles(
  bookDir: string,
  numbers: ReadonlyArray<number>,
  index: ReadonlyArray<ChapterMeta>,
): Promise<Map<number, ChapterFileMeta>> {
  let files: string[] = [];
  try {
    files = await readdir(join(bookDir, "chapters"));
  } catch (error) {
    throw new ChapterStructureError(400, `chapters dir unreadable: ${String(error)}`);
  }
  const map = new Map<number, ChapterFileMeta>();
  for (const number of numbers) {
    const meta = index.find((candidate) => candidate.number === number);
    if (!meta) {
      throw new ChapterStructureError(404, `Chapter ${number} not found`);
    }
    const fileName = findChapterFileByNumber(files, number);
    if (!fileName) {
      throw new ChapterStructureError(404, `Chapter ${number} file not found`);
    }
    map.set(number, { number, meta, fileName });
  }
  return map;
}

/** 受影响章的 runtime/drafts 产物清单（chapter-NNNN.* 前缀 / NNNN_*.md 前导号）。 */
async function collectArtifactFiles(
  bookDir: string,
  numbers: ReadonlyArray<number>,
): Promise<ReadonlyArray<{ readonly relativePath: string }>> {
  const out: Array<{ readonly relativePath: string }> = [];
  for (const sub of ["story/runtime", "story/drafts"]) {
    const dir = join(bookDir, sub);
    let entries: string[];
    try {
      entries = await readdir(dir);
    } catch {
      continue;
    }
    for (const name of entries) {
      const hit = numbers.some((n) =>
        sub.endsWith("runtime")
          ? name.startsWith(paddedRuntimePrefix(n))
          : parseLeadingNumber(name) === n,
      );
      if (hit) out.push({ relativePath: `${sub}/${name}` });
    }
  }
  return out;
}

function checkRevision(
  index: ReadonlyArray<ChapterMeta>,
  expectedRevision: string | undefined,
): void {
  if (
    expectedRevision !== undefined
    && chapterSetRevision(index) !== expectedRevision
  ) {
    throw new ChapterStructureError(409, REVISION_CONFLICT_MESSAGE, REVISION_CONFLICT_CODE);
  }
}

const sortedNumbers = (index: ReadonlyArray<ChapterMeta>): number[] =>
  index.map((meta) => meta.number).sort((a, b) => a - b);

const ensureFresh = (content: string): string =>
  content.endsWith("\n") ? content : `${content}\n`;

/** 拆分：`at` 号章在 UTF-16 offset 处切两章，新章占 at+1，后章起全量重编号。 */
export async function splitChapter(
  deps: ChapterStructureDeps,
  bookId: string,
  chapterNumber: number,
  offset: number,
  expectedRevision: string | undefined,
): Promise<StructureOutcome> {
  const index = await deps.loadChapterIndex();
  checkRevision(index, expectedRevision);
  const numbers = sortedNumbers(index);
  const files = await collectChapterFiles(deps.bookDir, [chapterNumber], index);
  const source = files.get(chapterNumber)!;
  const content = await readFile(join(deps.bookDir, "chapters", source.fileName), "utf-8");
  // R61：操作前全书指纹清单（staging/归档之前采集，内容未动）。
  const beforeList = await collectFingerprints(deps.bookDir, numbers, index);
  const [head, tail] = splitUtf16(content, offset);
  const renumber = renumberAfterSplit(numbers, chapterNumber);
  const newChapter = chapterNumber + 1;

  // 归档先于提交（失败零落盘）：旧号下旧文 + 新号下后段（structure 来源）。
  await archiveChapterVersion(deps.bookDir, chapterNumber, content, "structure");
  await archiveChapterVersion(deps.bookDir, newChapter, tail, "structure");

  const affected = [
    chapterNumber,
    ...numbers.filter((n) => n > chapterNumber),
  ];
  const affectedFiles = await collectChapterFiles(deps.bookDir, affected, index);
  const artifacts = await collectArtifactFiles(deps.bookDir, affected);

  const mapping = new Map(renumber.map((entry) => [entry.from, entry.to]));
  const writes: Array<{ relativePath: string; content: string }> = [];
  const deletes: string[] = [];
  const renumbered: RenumberedEntry[] = [];
  for (const [old, file] of affectedFiles) {
    const next = mapping.get(old);
    if (next === undefined || next === old) continue;
    // 1666 号：重编号只改名，正文逐字节不动（Rust 纯 rename、reorder 同款）——
    // 此前顺手补结尾换行，等于在一次「编号调整」里改写了未被操作的章正文，
    // 两腿同一份书经同一次拆分后字节不再一致。
    writes.push({
      relativePath: `chapters/${chapterFileName(next, file.meta.title)}`,
      content: await readFile(join(deps.bookDir, "chapters", file.fileName), "utf-8"),
    });
    deletes.push(`chapters/${file.fileName}`);
    renumbered.push({ from: old, to: next });
  }
  // 前段覆写旧号原文件（rename 语义：write 同路径 = 原地覆写）。
  writes.push({ relativePath: `chapters/${source.fileName}`, content: ensureFresh(head) });
  // 新章 = 拆出的后段（title 沿用原章，读侧空 title 回退章号显示）。
  writes.push({
    relativePath: `chapters/${chapterFileName(newChapter, source.meta.title)}`,
    content: ensureFresh(tail),
  });
  // runtime/drafts 产物按映射迁移（write 新名 + delete 旧名）。
  for (const artifact of artifacts) {
    const relativePath = artifact.relativePath;
    const sub = relativePath.startsWith("story/runtime") ? "runtime" : "drafts";
    const name = relativePath.split("/").pop()!;
    const oldNumber = name.startsWith("chapter-")
      ? Number.parseInt(name.slice(8, 12), 10)
      : (parseLeadingNumber(name) ?? 0);
    const next = mapping.get(oldNumber) ?? oldNumber;
    if (next === oldNumber) continue; // 不变号产物留原位（write+delete 同路径被 atomic set 拒绝）
    const destName = name.startsWith("chapter-")
      ? `${paddedRuntimePrefix(next)}${name.slice(12)}`
      : name.replace(String(oldNumber).padStart(4, "0"), String(next).padStart(4, "0"));
    writes.push({
      relativePath: `story/${sub}/${destName}`,
      content: await readFile(join(deps.bookDir, relativePath), "utf-8"),
    });
    deletes.push(relativePath);
  }

  const wordOverrides = new Map<number, number>([
    [chapterNumber, head.trim().length],
    [newChapter, tail.trim().length],
  ]);
  const nextIndex = buildNextIndex(index, mapping, wordOverrides);
  nextIndex.push({
    ...source.meta,
    number: newChapter,
    status: "ready-for-review",
    wordCount: tail.trim().length,
    updatedAt: new Date().toISOString(),
    createdAt: new Date().toISOString(),
    auditIssues: [],
    lengthWarnings: [],
  });
  nextIndex.sort((a, b) => a.number - b.number);

  // R61：提交前不变量守卫（抛在 commitAtomicFileSet 之前=零落盘）。
  const afterList = await plannedFingerprints(deps.bookDir, nextIndex, writes);
  const contentChecks: string[] = [];
  // 拼回校验用未规范化的原始切分（head+tail 恒等于 content——slice 无损）；
  // 写入侧的补尾换行是展示规范化，不参与「两片拼回」语义。
  if (head + tail !== content) {
    contentChecks.push("split pieces do not reassemble the chapter content");
  }
  assertStructureInvariants({ kind: "split", at: chapterNumber }, beforeList, afterList, contentChecks);

  await commitAtomicFileSet({
    rootDir: deps.bookDir,
    writes: [...writes, { relativePath: "chapters/index.json", content: JSON.stringify(nextIndex) }],
    deletes,
  });
  await appendStructureOpsLog(deps.bookDir, "split", beforeList, afterList, new Date().toISOString(), { at: chapterNumber });

  return {
    op: "split",
    bookId,
    splitFrom: chapterNumber,
    newChapter,
    renumbered,
    revision: chapterSetRevision(nextIndex),
    touchedFiles: ["chapters/index.json"],
    summary: `Split chapter ${chapterNumber} into ${chapterNumber}+${newChapter}.`,
  };
}

/** 合并：两章须相邻，序号保留较小者、删较大者（正文进 .trash），后续前移。 */
export async function mergeChapters(
  deps: ChapterStructureDeps,
  bookId: string,
  chapterNumber: number,
  mergeWith: number,
  separator: string | undefined,
  expectedRevision: string | undefined,
): Promise<StructureOutcome> {
  if (chapterNumber === mergeWith) {
    throw new ChapterStructureError(400, "Merge requires two distinct adjacent chapters.");
  }
  if (Math.abs(chapterNumber - mergeWith) !== 1) {
    throw new ChapterStructureError(400, "Merge requires adjacent chapters.");
  }
  const index = await deps.loadChapterIndex();
  checkRevision(index, expectedRevision);
  const keep = Math.min(chapterNumber, mergeWith);
  const removed = Math.max(chapterNumber, mergeWith);
  const numbers = sortedNumbers(index);
  const files = await collectChapterFiles(deps.bookDir, [keep, removed], index);
  const keepFile = files.get(keep)!;
  const removedFile = files.get(removed)!;
  const keepContent = await readFile(join(deps.bookDir, "chapters", keepFile.fileName), "utf-8");
  const removedContent = await readFile(join(deps.bookDir, "chapters", removedFile.fileName), "utf-8");
  const merged = mergeContent(keepContent.trimEnd(), removedContent.trimStart(), separator ?? "\n\n");
  const renumber = renumberAfterMerge(numbers, removed);
  // R61：操作前全书指纹清单（归档/提交之前采集）。
  const beforeList = await collectFingerprints(deps.bookDir, numbers, index);

  await archiveChapterVersion(deps.bookDir, keep, keepContent, "structure");

  const affected = [keep, removed, ...numbers.filter((n) => n > removed)];
  const affectedFiles = await collectChapterFiles(deps.bookDir, affected, index);
  const artifacts = await collectArtifactFiles(deps.bookDir, affected);

  const mapping = new Map(renumber.map((entry) => [entry.from, entry.to]));
  const writes: Array<{ relativePath: string; content: string }> = [];
  const deletes: string[] = [];
  const renumbered: RenumberedEntry[] = [];
  // 被删章正文 → .trash（永不硬删）。
  const trashName = `${removedFile.fileName}`;
  writes.push({
    relativePath: `${TRASH_DIR}/${trashName}`,
    content: removedContent,
  });
  deletes.push(`chapters/${removedFile.fileName}`);
  for (const [old, file] of affectedFiles) {
    if (old === removed) continue;
    const next = mapping.get(old);
    if (next === undefined || next === old) continue;
    writes.push({
      relativePath: `chapters/${chapterFileName(next, file.meta.title)}`,
      content: await readFile(join(deps.bookDir, "chapters", file.fileName), "utf-8"),
    });
    deletes.push(`chapters/${file.fileName}`);
    renumbered.push({ from: old, to: next });
  }
  // 1666 号：合并产物恒以单个换行收尾（Rust 同款同修）。此前直接补 `\n`——被并
  // 章以换行收尾（写入惯例）时其尾部换行与补的收尾换行叠加成一个多余空行；被并
  // 章为空白时分隔符的尾随空白同样在此剪掉。
  writes.push({ relativePath: `chapters/${keepFile.fileName}`, content: `${merged.trimEnd()}\n` });
  for (const artifact of artifacts) {
    const relativePath = artifact.relativePath;
    const sub = relativePath.startsWith("story/runtime") ? "runtime" : "drafts";
    const name = relativePath.split("/").pop()!;
    const oldNumber = name.startsWith("chapter-")
      ? Number.parseInt(name.slice(8, 12), 10)
      : (parseLeadingNumber(name) ?? 0);
    if (oldNumber === removed) {
      // 被删章产物随正文进 trash（永不硬删语义）。
      writes.push({ relativePath: `${TRASH_DIR}/${sub}/${name}`, content: await readFile(join(deps.bookDir, relativePath), "utf-8") });
      deletes.push(relativePath);
      continue;
    }
    const next = mapping.get(oldNumber) ?? oldNumber;
    if (next === oldNumber) continue; // 不变号产物留原位（write+delete 同路径被 atomic set 拒绝）
    const destName = name.startsWith("chapter-")
      ? `${paddedRuntimePrefix(next)}${name.slice(12)}`
      : name.replace(String(oldNumber).padStart(4, "0"), String(next).padStart(4, "0"));
    writes.push({
      relativePath: `story/${sub}/${destName}`,
      content: await readFile(join(deps.bookDir, relativePath), "utf-8"),
    });
    deletes.push(relativePath);
  }

  const wordOverrides = new Map<number, number>([[keep, merged.trim().length]]);
  // 1647 号 bug 修复：先滤掉被删章行、再应用重编号映射（与 Rust merge 同款
  // 镜像 bug——此前先映射后过滤，重编后恰好落到 removed 号的章被一并滤杀，
  // index 丢章而文件面正确=双面发散）。
  const nextIndex = buildNextIndex(
    index.filter((meta) => meta.number !== removed),
    mapping,
    wordOverrides,
  );

  // R61：提交前不变量守卫（抛在 commitAtomicFileSet 之前=零落盘）。
  const afterList = await plannedFingerprints(deps.bookDir, nextIndex, writes);
  const contentChecks: string[] = [];
  const expectedKeep = `${mergeContent(keepContent.trimEnd(), removedContent.trimStart(), separator ?? "\n\n").trimEnd()}\n`;
  const keepWrite = writes.find((write) => write.relativePath === `chapters/${keepFile.fileName}`);
  if (!keepWrite || keepWrite.content !== expectedKeep) {
    contentChecks.push("merged chapter content does not match merge_content(keep, removed)");
  }
  assertStructureInvariants({ kind: "merge", removed }, beforeList, afterList, contentChecks);

  await commitAtomicFileSet({
    rootDir: deps.bookDir,
    writes: [...writes, { relativePath: "chapters/index.json", content: JSON.stringify(nextIndex) }],
    deletes,
  });
  await appendStructureOpsLog(deps.bookDir, "merge", beforeList, afterList, new Date().toISOString(), { removed });

  return {
    op: "merge",
    bookId,
    mergedInto: keep,
    removedChapter: removed,
    renumbered,
    revision: chapterSetRevision(nextIndex),
    touchedFiles: ["chapters/index.json"],
    summary: `Merged chapter ${removed} into ${keep}.`,
  };
}

/** 重排：`order`（新序章号全排列）→ 批量重编号事务。 */
export async function reorderChapters(
  deps: ChapterStructureDeps,
  bookId: string,
  order: ReadonlyArray<number>,
  expectedRevision: string | undefined,
): Promise<StructureOutcome> {
  const index = await deps.loadChapterIndex();
  checkRevision(index, expectedRevision);
  const numbers = sortedNumbers(index);
  const renumber = reorderPlan(numbers, order);
  if (renumber.length === 0) {
    throw new ChapterStructureError(400, "Reorder produces no change.");
  }
  const files = await collectChapterFiles(deps.bookDir, numbers, index);
  const artifacts = await collectArtifactFiles(deps.bookDir, numbers);
  // R61：操作前全书指纹清单（提交之前采集）。
  const beforeList = await collectFingerprints(deps.bookDir, numbers, index);

  const mapping = new Map(renumber.map((entry) => [entry.from, entry.to]));
  const writes: Array<{ relativePath: string; content: string }> = [];
  const deletes: string[] = [];
  for (const [old, file] of files) {
    const next = mapping.get(old);
    if (next === undefined || next === old) continue;
    writes.push({
      relativePath: `chapters/${chapterFileName(next, file.meta.title)}`,
      content: await readFile(join(deps.bookDir, "chapters", file.fileName), "utf-8"),
    });
    deletes.push(`chapters/${file.fileName}`);
  }
  for (const artifact of artifacts) {
    const relativePath = artifact.relativePath;
    const sub = relativePath.startsWith("story/runtime") ? "runtime" : "drafts";
    const name = relativePath.split("/").pop()!;
    const oldNumber = name.startsWith("chapter-")
      ? Number.parseInt(name.slice(8, 12), 10)
      : (parseLeadingNumber(name) ?? 0);
    const next = mapping.get(oldNumber) ?? oldNumber;
    if (next === oldNumber) continue; // 不变号产物留原位（write+delete 同路径被 atomic set 拒绝）
    const destName = name.startsWith("chapter-")
      ? `${paddedRuntimePrefix(next)}${name.slice(12)}`
      : name.replace(String(oldNumber).padStart(4, "0"), String(next).padStart(4, "0"));
    writes.push({
      relativePath: `story/${sub}/${destName}`,
      content: await readFile(join(deps.bookDir, relativePath), "utf-8"),
    });
    deletes.push(relativePath);
  }

  const nextIndex = buildNextIndex(index, mapping);

  // R61：提交前不变量守卫（reorder 不得改写任何章正文）。
  const afterList = await plannedFingerprints(deps.bookDir, nextIndex, writes);
  assertStructureInvariants({ kind: "reorder" }, beforeList, afterList, []);

  await commitAtomicFileSet({
    rootDir: deps.bookDir,
    writes: [...writes, { relativePath: "chapters/index.json", content: JSON.stringify(nextIndex) }],
    deletes,
  });
  await appendStructureOpsLog(deps.bookDir, "reorder", beforeList, afterList, new Date().toISOString());

  return {
    op: "reorder",
    bookId,
    renumbered: renumber,
    revision: chapterSetRevision(nextIndex),
    touchedFiles: ["chapters/index.json"],
    summary: `Reordered ${order.length} chapters.`,
  };
}

/** 索引行重编号应用：映射 number + 移位/字数覆写行 updatedAt 刷新。 */
function buildNextIndex(
  index: ReadonlyArray<ChapterMeta>,
  mapping: Map<number, number>,
  wordOverrides?: Map<number, number>,
): ChapterMeta[] {
  const nowIso = new Date().toISOString();
  return index
    .map((meta) => {
      const nextNumber = mapping.get(meta.number) ?? meta.number;
      const next: ChapterMeta = { ...meta };
      if (next.number !== nextNumber) {
        (next as { number: number }).number = nextNumber;
        (next as { updatedAt: string }).updatedAt = nowIso;
      }
      if (wordOverrides?.has(nextNumber)) {
        (next as { wordCount: number }).wordCount = wordOverrides.get(nextNumber)!;
        (next as { updatedAt: string }).updatedAt = nowIso;
      }
      return next;
    })
    .sort((a, b) => a.number - b.number);
}
