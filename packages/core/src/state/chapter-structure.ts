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
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import { commitAtomicFileSet } from "../utils/atomic-file-set.js";
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
  readonly op: "split" | "merge" | "reorder";
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
    writes.push({
      relativePath: `chapters/${chapterFileName(next, file.meta.title)}`,
      content: ensureFresh(await readFile(join(deps.bookDir, "chapters", file.fileName), "utf-8")),
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

  await commitAtomicFileSet({
    rootDir: deps.bookDir,
    writes: [...writes, { relativePath: "chapters/index.json", content: JSON.stringify(nextIndex) }],
    deletes,
  });

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
  writes.push({ relativePath: `chapters/${keepFile.fileName}`, content: `${merged}\n` });
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
  const nextIndex = buildNextIndex(index, mapping, wordOverrides)
    .filter((meta) => meta.number !== removed);

  await commitAtomicFileSet({
    rootDir: deps.bookDir,
    writes: [...writes, { relativePath: "chapters/index.json", content: JSON.stringify(nextIndex) }],
    deletes,
  });

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

  await commitAtomicFileSet({
    rootDir: deps.bookDir,
    writes: [...writes, { relativePath: "chapters/index.json", content: JSON.stringify(nextIndex) }],
    deletes,
  });

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
