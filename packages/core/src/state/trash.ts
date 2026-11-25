import { lstat, mkdir, readdir, readFile, rename, rm, rmdir, stat } from "node:fs/promises";
import { basename, dirname, join, posix, sep } from "node:path";
import { loadWorkManifest, workDirectory } from "../harness/work-store.js";
import { ChapterMetaSchema, type ChapterMeta } from "../models/chapter.js";
import type { AtomicFileWrite } from "../utils/atomic-file-set.js";
import { isSafeBookId } from "../utils/book-id.js";
import { chapterFileNumber, compareByCodePoint, preferredChapterFile } from "../utils/chapter-files.js";
import { chapterMetaFromFile } from "./chapter-meta.js";

// R58/1676 号：破坏性操作护栏——回滚与删书不再硬删，被废内容移入回收站。
//
// - 回滚：被废的章正文、草稿、快照、runtime 工件，连同会被覆盖或删除的真相文件、
//   结构化状态与章节索引，整批进 `chapters/.trash/rollback-{ts}-{target}/`，书内相对
//   路径原样保留。sqlite 加速索引（可重建的派生物）不进回收站，照旧直接删。
// - 删书：整个作品目录进项目级回收站 `{projectRoot}/.trash/books/{bookId}--{ts}/`。
//   放在 books/works 根之外——作品发现、迁移、备份白名单都看不到它。
//
// 两个纯函数与产品引擎同一条规则（`engine-rs/src/state/trash.rs` 的
// `trash_batch_name` / `trashed_book_name`），对跑用同一份夹具钉两端条目集。

const CHAPTER_TRASH = join("chapters", ".trash");
const BOOK_TRASH = join(".trash", "books");

/** 回滚会覆盖或删除的真相文件（两腿恢复逻辑的并集；在场的才进回收站）。 */
export const ROLLBACK_TRUTH_FILES: ReadonlyArray<string> = [
  "current_state.md",
  "pending_hooks.md",
  "chapter_summaries.md",
  "particle_ledger.md",
  "subplot_board.md",
  "emotional_arcs.md",
  "character_matrix.md",
];

function compactTimestamp(timestampMillis: number): string {
  return new Date(timestampMillis).toISOString().replace(/[-:T]/g, "").slice(0, 14);
}

/** 回滚批次目录名：`rollback-{yyyyMMddHHmmss}-{target}`（UTC）。 */
export function trashBatchName(timestampMillis: number, targetChapter: number): string {
  return `rollback-${compactTimestamp(timestampMillis)}-${targetChapter}`;
}

/** 被删的书在项目级回收站里的目录名：`{bookId}--{yyyyMMddHHmmss}`（UTC）。 */
export function trashedBookName(bookId: string, timestampMillis: number): string {
  return `${bookId}--${compactTimestamp(timestampMillis)}`;
}

/** 目录已被占用时追加 `-2`、`-3`…（与章级回收站的重名后缀同一惯例）。 */
async function reserveTrashDir(parent: string, base: string): Promise<string> {
  for (let attempt = 1; ; attempt += 1) {
    const candidate = attempt === 1 ? base : `${base}-${attempt}`;
    const taken = await stat(join(parent, candidate)).then(
      () => true,
      (error: NodeJS.ErrnoException) => {
        if (error.code === "ENOENT") return false;
        throw error;
      },
    );
    if (!taken) return candidate;
  }
}

/**
 * 把 `paths`（书内相对路径；文件或目录）在回收站批次里的拷贝列成原子写入——内容
 * 按字节读出（`Uint8Array`），任何文件都逐字节等于原样。目录递归到文件；空目录
 * 不占条目。不存在的路径跳过。
 */
export async function trashBatchWrites(
  bookDir: string,
  batch: string,
  paths: ReadonlyArray<string>,
): Promise<AtomicFileWrite[]> {
  const writes: AtomicFileWrite[] = [];
  const copy = async (relative: string): Promise<void> => {
    const source = join(bookDir, relative);
    const info = await stat(source).catch((error: NodeJS.ErrnoException) => {
      if (error.code === "ENOENT") return null;
      throw error;
    });
    if (!info) return;
    if (info.isDirectory()) {
      for (const name of (await readdir(source)).sort()) {
        await copy(join(relative, name));
      }
      return;
    }
    if (!info.isFile()) return;
    writes.push({ relativePath: join(batch, relative), content: new Uint8Array(await readFile(source)) });
  };
  for (const path of paths) await copy(path);
  return writes;
}

/** 回滚批次的回收站相对路径（相对书目录）；同名批次已在时追加后缀。 */
export async function reserveRollbackBatch(bookDir: string, timestampMillis: number, targetChapter: number): Promise<string> {
  const name = await reserveTrashDir(join(bookDir, CHAPTER_TRASH), trashBatchName(timestampMillis, targetChapter));
  return join(CHAPTER_TRASH, name);
}

/**
 * 删书：整个作品目录移入项目级回收站。返回 `"trashed"`（移走了）或 `"missing"`
 * （没有这本书——与删除前的 `rm({ force: true })` 一样不报错）。
 *
 * 只动书：作品清单不是 `longform-novel` 的作品不走这条路（此前它会把任意作品的
 * `source/` 整个删掉）。没有清单、只剩 `source/` 的孤儿目录照旧处理（也进回收站）。
 * rename 失败直接抛——不降级为硬删。
 */
export async function trashBook(projectRoot: string, bookId: string): Promise<"trashed" | "missing"> {
  const workDir = workDirectory(projectRoot, bookId);
  const manifest = await loadWorkManifest(projectRoot, bookId).catch((error: NodeJS.ErrnoException) => {
    if (error.code === "ENOENT") return null;
    throw error;
  });
  if (manifest && manifest.profileId !== "longform-novel") {
    throw Object.assign(new Error(`Work is not a book: ${bookId}`), { code: "NOT_A_BOOK" });
  }
  const target = manifest ? workDir : join(workDir, "source");
  if (!(await stat(target).then(() => true, (error: NodeJS.ErrnoException) => {
    if (error.code === "ENOENT") return false;
    throw error;
  }))) {
    return "missing";
  }
  const trashRoot = join(projectRoot, BOOK_TRASH);
  await mkdir(trashRoot, { recursive: true });
  const entry = await reserveTrashDir(trashRoot, trashedBookName(bookId, Date.now()));
  const entryDir = join(trashRoot, entry);
  if (manifest) {
    await rename(target, entryDir);
  } else {
    // 孤儿目录也保持作品目录内的相对布局（`source/` 在回收站条目下）。
    await mkdir(entryDir);
    await rename(target, join(entryDir, "source"));
  }
  return "trashed";
}

// ── R60/1679 号：回收站的列出 / 恢复 / 清空 ────────────────────────────────────
//
// 回收站是「把文件找回来」，不是撤销操作。恢复只把文件移回原位，状态与快照不动。
// - 书级：`{projectRoot}/.trash/books/{bookId}--{ts}/`。两腿共用这一个目录（桌面默认
//   产品引擎、Node 是回退，同一个项目根）。本腿只认自己删出来的形态——根上有
//   `work.json`，或只剩 `source/` 的孤儿目录；产品引擎删的书根上是 `book.json`，由它
//   自己列、自己恢复。把另一种形态搬进本腿的书根，得到的是一本坏书。
// - 章级：`{bookDir}/chapters/.trash/`。平铺的章文件（重名时带 `-2`、`-3` 后缀）与
//   回滚批次 `rollback-{ts}-{target}/`（书内相对路径原样）。
//
// 恢复的底线：目标已在原位 → `TRASH_TARGET_EXISTS`，一个字节都不覆盖；整批恢复全有
// 或全无。条目识别的两个纯函数与产品引擎 `state::trash.rs` 同一条规则
// （`packages/studio/src/__tests__/golden/trash-entry-vectors.json` 两端同读）。

export type TrashErrorCode = "INVALID_TRASH_REQUEST" | "TRASH_ENTRY_NOT_FOUND" | "TRASH_TARGET_EXISTS";

export class TrashError extends Error {
  /** `TRASH_TARGET_EXISTS` 时：已在原位、挡住恢复的那些路径。 */
  readonly conflicts: readonly string[];
  constructor(
    readonly code: TrashErrorCode,
    message: string,
    conflicts: ReadonlyArray<string> = [],
  ) {
    super(message);
    this.name = "TrashError";
    this.conflicts = conflicts;
  }
}

const invalidRequest = (message: string): TrashError => new TrashError("INVALID_TRASH_REQUEST", message);
const notFound = (what: string): TrashError => new TrashError("TRASH_ENTRY_NOT_FOUND", `Trash entry not found: ${what}`);
const isMissing = (error: NodeJS.ErrnoException): boolean => error.code === "ENOENT";

async function lstatOrNull(path: string): Promise<{ size: number; mtimeMs: number; isDirectory(): boolean; isFile(): boolean } | null> {
  return lstat(path).catch((error: NodeJS.ErrnoException) => {
    if (isMissing(error)) return null;
    throw error;
  });
}

/** 目录下的全部普通文件（相对该目录的 POSIX 路径，排序）；符号链接与其它类型跳过。 */
async function regularFiles(dir: string, prefix = ""): Promise<{ path: string; bytes: number; mtimeMs: number }[]> {
  const out: { path: string; bytes: number; mtimeMs: number }[] = [];
  for (const name of (await readdir(dir)).sort()) {
    const full = join(dir, name);
    const info = await lstat(full);
    if (info.isDirectory()) {
      out.push(...(await regularFiles(full, posix.join(prefix, name))));
    } else if (info.isFile()) {
      out.push({ path: posix.join(prefix, name), bytes: info.size, mtimeMs: info.mtimeMs });
    }
  }
  return out;
}

const ROLLBACK_BATCH_RE = /^rollback-\d{14}-\d+(?:-\d+)?$/u;
const BOOK_ENTRY_RE = /^(.+)--(\d{14})(?:-\d+)?$/u;
const CHAPTER_TRASH_PREFIX = "chapters/.trash/";
// 条目表的文本排序一律走 compareByCodePoint（码点序 = 产品引擎 `String` 的字节序）——
// JS 的 `<` 是 UTF-16 码元序，增补平面字符与全角标点的相对次序是反的（854 号）。

/** 章级回收站条目（相对书目录）→ 它来自哪里。认不出布局（越界、空段、未知子目录）→ null。 */
export interface TrashPathInfo {
  readonly originalPath: string;
  /** 来自哪个回滚批次；平铺的条目是 null。 */
  readonly batch: string | null;
}

/**
 * 章级回收站条目的原路径。平铺条目去掉进回收站时加的重名后缀（`base-2.ext` → `base.ext`）；
 * 批次条目的原路径就是批次目录下那一串。只认这两种布局，路径里不许有 `..`、空段、反斜杠
 * 与 NUL——调用方拿到的原路径一定还是书目录里的相对路径。
 */
export function originalPathOfTrashEntry(path: string): TrashPathInfo | null {
  if (!path.startsWith(CHAPTER_TRASH_PREFIX) || /[\\\0]/u.test(path)) return null;
  const segments = path.slice(CHAPTER_TRASH_PREFIX.length).split("/");
  if (segments.some((segment) => segment === "" || segment === "." || segment === "..")) return null;
  const [first, ...rest] = segments as [string, ...string[]];
  if (ROLLBACK_BATCH_RE.test(first)) {
    return rest.length > 0 ? { originalPath: rest.join("/"), batch: first } : null;
  }
  if (rest.length > 0) return null;
  return { originalPath: `chapters/${first.replace(/-\d+(\.[^./]*)$/u, "$1")}`, batch: null };
}

/** `yyyyMMddHHmmss`（UTC）→ ISO；不是真实存在的时刻（月 13、时 24）→ null。 */
function compactToIso(digits: string): string | null {
  if (!/^\d{14}$/u.test(digits)) return null;
  const date = new Date(0);
  // 不用 Date.UTC：它把 0–99 当成 1900 起，产品引擎那边没有这个怪癖。
  date.setUTCFullYear(Number(digits.slice(0, 4)), Number(digits.slice(4, 6)) - 1, Number(digits.slice(6, 8)));
  date.setUTCHours(Number(digits.slice(8, 10)), Number(digits.slice(10, 12)), Number(digits.slice(12, 14)), 0);
  const iso = date.toISOString();
  return iso.replace(/[-:T]/g, "").slice(0, 14) === digits ? iso : null;
}

/** 回滚批次目录名里的时间（批次里的文件保留着回滚前的修改时间，删掉的时刻只在名字里）。 */
export function trashedAtOfRollbackBatch(batch: string): string | null {
  return ROLLBACK_BATCH_RE.test(batch) ? compactToIso(batch.slice("rollback-".length, "rollback-".length + 14)) : null;
}

/** 书级回收站条目名里的时间。 */
export function trashedAtOfTrashEntry(entry: string): string | null {
  const match = entry.match(BOOK_ENTRY_RE);
  return match ? compactToIso(match[2]!) : null;
}

/** 书级回收站条目名 → 书 id；名字不成形、时刻不存在、书 id 不是安全的路径段 → null。 */
export function bookIdOfTrashEntry(entry: string): string | null {
  if (/[\\/\0]/u.test(entry)) return null;
  const match = entry.match(BOOK_ENTRY_RE);
  const bookId = match?.[1];
  return bookId !== undefined && isSafeBookId(bookId) && trashedAtOfTrashEntry(entry) !== null ? bookId : null;
}

export interface BookTrashEntry {
  readonly entry: string;
  readonly bookId: string;
  readonly trashedAt: string;
  readonly bytes: number;
  readonly fileCount: number;
  /** 清单上的标题；读不出是 null。 */
  readonly title: string | null;
}

/** 本腿删出来的形态：根上有 `work.json`，或只剩 `source/` 目录。 */
async function isOwnBookEntry(entryDir: string): Promise<boolean> {
  const info = await lstatOrNull(entryDir);
  if (!info?.isDirectory()) return false;
  for (const name of ["work.json", "source"]) {
    const child = await lstatOrNull(join(entryDir, name));
    if (child?.isFile() || child?.isDirectory()) return true;
  }
  return false;
}

async function entryTitle(entryDir: string): Promise<string | null> {
  for (const file of [join(entryDir, "work.json"), join(entryDir, "source", "book.json")]) {
    const title = await readFile(file, "utf8")
      .then((raw) => (JSON.parse(raw) as { title?: unknown }).title, () => undefined)
      .catch(() => undefined);
    if (typeof title === "string" && title.length > 0) return title;
  }
  return null;
}

/** 列出本腿的书级回收站，删得晚的在前。目录不存在 → 空。 */
export async function listBookTrash(projectRoot: string): Promise<BookTrashEntry[]> {
  const trashRoot = join(projectRoot, BOOK_TRASH);
  const entries: BookTrashEntry[] = [];
  for (const name of (await readdir(trashRoot).catch((error: NodeJS.ErrnoException) => {
    if (isMissing(error)) return [];
    throw error;
  })).sort()) {
    const bookId = bookIdOfTrashEntry(name);
    if (!bookId) continue;
    const entryDir = join(trashRoot, name);
    if (!(await isOwnBookEntry(entryDir))) continue;
    const files = await regularFiles(entryDir);
    entries.push({
      entry: name,
      bookId,
      trashedAt: trashedAtOfTrashEntry(name)!,
      bytes: files.reduce((sum, file) => sum + file.bytes, 0),
      fileCount: files.length,
      title: await entryTitle(entryDir),
    });
  }
  return entries.sort((a, b) => compareByCodePoint(b.trashedAt, a.trashedAt) || compareByCodePoint(b.entry, a.entry));
}

/** 条目名必须是本腿认得的一个在册条目；否则按「请求不合法」或「找不到」拒绝。 */
async function ownBookEntryDir(projectRoot: string, entry: string): Promise<string> {
  if (bookIdOfTrashEntry(entry) === null) throw invalidRequest("Invalid trash entry");
  const entryDir = join(projectRoot, BOOK_TRASH, entry);
  if (!(await isOwnBookEntry(entryDir))) throw notFound(entry);
  return entryDir;
}

/** 把一本删掉的书搬回作品库。书目录已被占用（删掉之后又建了一本同 id 的）→ `TRASH_TARGET_EXISTS`。 */
export async function restoreBookFromTrash(projectRoot: string, entry: string): Promise<{ bookId: string }> {
  const entryDir = await ownBookEntryDir(projectRoot, entry);
  const bookId = bookIdOfTrashEntry(entry)!;
  const target = workDirectory(projectRoot, bookId);
  if (await lstat(target).then(() => true, (error: NodeJS.ErrnoException) => {
    if (isMissing(error)) return false;
    throw error;
  })) {
    throw new TrashError("TRASH_TARGET_EXISTS", `Book "${bookId}" already exists; the trashed copy was left in the trash.`, [`works/${bookId}`]);
  }
  await mkdir(dirname(target), { recursive: true });
  await rename(entryDir, target);
  return { bookId };
}

/** 真空删除一个书级回收站条目。 */
export async function purgeBookTrash(projectRoot: string, entry: string): Promise<{ entry: string }> {
  const entryDir = await ownBookEntryDir(projectRoot, entry);
  await rm(entryDir, { recursive: true });
  return { entry };
}

export interface ChapterTrashEntry {
  /** 回收站里的路径（相对书目录）。 */
  readonly path: string;
  readonly originalPath: string;
  readonly batch: string | null;
  readonly bytes: number;
  /** 文件自己的修改时间（进回收站是 rename，改不了它）。 */
  readonly modifiedAt: string;
  /** 删掉的时刻：批次条目取批次名里的时间；平铺的条目没有记录 → null。 */
  readonly trashedAt: string | null;
}

/** 列出一本书的章级回收站。回收站不存在 → 空。符号链接与认不出的布局跳过。 */
export async function listChapterTrash(bookDir: string): Promise<ChapterTrashEntry[]> {
  const trashDir = join(bookDir, CHAPTER_TRASH);
  const entries: ChapterTrashEntry[] = [];
  for (const name of (await readdir(trashDir).catch((error: NodeJS.ErrnoException) => {
    if (isMissing(error)) return [];
    throw error;
  })).sort()) {
    const info = await lstatOrNull(join(trashDir, name));
    if (!info) continue;
    if (info.isDirectory()) {
      const trashedAt = trashedAtOfRollbackBatch(name);
      if (!trashedAt) continue;
      for (const file of await regularFiles(join(trashDir, name))) {
        entries.push({
          path: posix.join(CHAPTER_TRASH, name, file.path),
          originalPath: file.path,
          batch: name,
          bytes: file.bytes,
          modifiedAt: new Date(file.mtimeMs).toISOString(),
          trashedAt,
        });
      }
    } else if (info.isFile()) {
      const path = posix.join(CHAPTER_TRASH, name);
      const parsed = originalPathOfTrashEntry(path);
      if (!parsed) continue;
      entries.push({
        path,
        originalPath: parsed.originalPath,
        batch: null,
        bytes: info.size,
        modifiedAt: new Date(info.mtimeMs).toISOString(),
        trashedAt: null,
      });
    }
  }
  const when = (entry: ChapterTrashEntry): string => entry.trashedAt ?? entry.modifiedAt;
  return entries.sort((a, b) => compareByCodePoint(when(b), when(a)) || compareByCodePoint(a.path, b.path));
}

export interface RestoredTrashEntry {
  readonly path: string;
  readonly originalPath: string;
  /** 回到 `chapters/` 下的章文件：它的章号与该登记进目录的条目；别的文件是 null。 */
  readonly chapter: { readonly number: number; readonly meta: ChapterMeta } | null;
}

const isChapterFile = (originalPath: string): boolean =>
  originalPath.startsWith("chapters/") && !originalPath.slice("chapters/".length).includes("/")
    && chapterFileNumber(basename(originalPath)) !== undefined && chapterFileNumber(basename(originalPath)) !== 0;

/**
 * 找回的章文件该带哪条目录条目：回滚批次里存着回滚前的目录，就从那里取这一章原来的
 * 条目（状态、词数都是回滚前的）；取不到就按文件现做一条（与索引重建同口径）。
 */
async function chapterOf(bookDir: string, entry: ChapterTrashEntry): Promise<RestoredTrashEntry["chapter"]> {
  if (!isChapterFile(entry.originalPath)) return null;
  const number = chapterFileNumber(basename(entry.originalPath))!;
  if (entry.batch) {
    // 回滚前的目录本身坏了就当没有，按文件现做条目。
    const previous = await readFile(join(bookDir, CHAPTER_TRASH, entry.batch, "chapters", "index.json"), "utf8")
      .then((raw) => ChapterMetaSchema.array().safeParse(JSON.parse(raw)), () => null)
      .catch(() => null);
    const meta = previous?.success ? previous.data.find((candidate) => candidate.number === number) : undefined;
    if (meta) return { number, meta };
  }
  const source = join(bookDir, entry.path);
  const content = await readFile(source, "utf8");
  const meta = chapterMetaFromFile(basename(entry.originalPath), content, new Date(entry.modifiedAt).toISOString());
  return meta ? { number, meta } : null;
}

/**
 * 文件移走、删掉之后，把回收站里空出来的目录摘掉：自下而上只摘空目录，碰到非空就停；
 * 回收站目录本身留着。批次是按书内相对路径存的，整批找回后 `story/drafts/` 这类空壳
 * 不该留在回收站里。
 */
async function pruneEmptyDirs(trashDir: string, vacated: ReadonlyArray<string>): Promise<void> {
  for (const start of new Set(vacated)) {
    for (let dir = start; dir.startsWith(trashDir + sep) && dir !== trashDir; dir = dirname(dir)) {
      if (!(await rmdir(dir).then(() => true, () => false))) break;
    }
  }
}

/** 选定、核对过的一条找回：目标不在原位，章条目已备好。 */
interface PlannedRestore {
  readonly entry: ChapterTrashEntry;
  readonly chapter: RestoredTrashEntry["chapter"];
}

/**
 * 同章双形态一起回到原位时（旧命名的章与重写后的正典章都在同一个回滚批次里），目录条目
 * 跟随读侧会打开的那一个文件——正典优先、同类内文件名序小者，与索引重建同一个裁决。
 */
function chapterMetasOf(planned: readonly PlannedRestore[]): ChapterMeta[] {
  const byNumber = new Map<number, PlannedRestore>();
  for (const item of planned) {
    if (!item.chapter) continue;
    const current = byNumber.get(item.chapter.number);
    // 择优看的是文件名（正典的判定锚在串首），不是整条路径。
    if (!current || preferredChapterFile(basename(current.entry.originalPath), basename(item.entry.originalPath)) === basename(item.entry.originalPath)) {
      byNumber.set(item.chapter.number, item);
    }
  }
  return [...byNumber.values()].map((item) => item.chapter!.meta);
}

/**
 * 把章级回收站里的条目移回原位：`{ path }` 一个条目，或 `{ batch }` 整个回滚批次。
 *
 * 目标已在原位 → `TRASH_TARGET_EXISTS`（`conflicts` 列出挡路的路径），一个都不移。
 * 条目的路径必须与列出来的逐字相同——列出来的才在回收站里。
 *
 * `register` 在搬文件**之前**调用，带上要登记进章节目录的条目（目录里已有的章号由它跳过）。
 * 先写目录后搬文件：目录写不下来（磁盘满最典型）就什么都不搬，回收站里原样留着、书架
 * 照常。反过来做的话文件回位而目录不列——这一章在界面上看不见，进度守卫把这本书判成
 * 孤儿，而重试会撞上「条目不在」，没法自愈。
 */
export async function restoreChapterTrash(
  bookDir: string,
  target: { readonly path?: unknown; readonly batch?: unknown },
  register: (chapters: readonly ChapterMeta[]) => Promise<void>,
): Promise<{ restored: RestoredTrashEntry[] }> {
  const entries = await listChapterTrash(bookDir);
  let picked: ChapterTrashEntry[];
  if (typeof target.batch === "string") {
    if (target.path !== undefined || !ROLLBACK_BATCH_RE.test(target.batch)) throw invalidRequest("Invalid trash batch");
    picked = entries.filter((entry) => entry.batch === target.batch);
    if (picked.length === 0) throw notFound(target.batch);
  } else if (typeof target.path === "string") {
    if (originalPathOfTrashEntry(target.path) === null) throw invalidRequest("Invalid trash path");
    picked = entries.filter((entry) => entry.path === target.path);
    if (picked.length === 0) throw notFound(target.path);
  } else {
    throw invalidRequest("Expected { path } or { batch }");
  }
  const inPlace = (await Promise.all(picked.map(async (entry) => ({
    entry,
    taken: await lstat(join(bookDir, entry.originalPath)).then(() => true, (error: NodeJS.ErrnoException) => {
      if (isMissing(error)) return false;
      throw error;
    }),
  })))).filter((item) => item.taken).map((item) => item.entry.originalPath);
  if (inPlace.length > 0) {
    throw new TrashError(
      "TRASH_TARGET_EXISTS",
      `Already in place, nothing was restored: ${inPlace.join(", ")}`,
      inPlace,
    );
  }
  const planned = await Promise.all(picked.map(async (entry) => ({ entry, chapter: await chapterOf(bookDir, entry) })));
  await register(chapterMetasOf(planned));
  const restored: RestoredTrashEntry[] = [];
  for (const { entry, chapter } of planned) {
    const destination = join(bookDir, entry.originalPath);
    await mkdir(dirname(destination), { recursive: true });
    await rename(join(bookDir, entry.path), destination);
    restored.push({ path: entry.path, originalPath: entry.originalPath, chapter });
  }
  await pruneEmptyDirs(join(bookDir, CHAPTER_TRASH), planned.map((item) => dirname(join(bookDir, item.entry.path))));
  return { restored };
}

/**
 * 真空删除章级回收站里的东西：`{ path }` 一个条目、`{ batch }` 一个回滚批次、`{ all: true }`
 * 全部列出来的条目（回收站目录本身留着）。返回删掉的条目数。
 *
 * 只删列出来的条目：符号链接、认不出的布局不碰——删掉的一定是界面上列过的。全部清空之后
 * 回收站目录里可能剩看不见的残余，不占界面也不占找回的路。
 */
export async function purgeChapterTrash(
  bookDir: string,
  target: { readonly path?: unknown; readonly batch?: unknown; readonly all?: unknown },
): Promise<{ removed: number }> {
  const given = [target.path, target.batch, target.all].filter((value) => value !== undefined);
  if (given.length !== 1) throw invalidRequest("Expected exactly one of { path }, { batch }, { all: true }");
  const trashDir = join(bookDir, CHAPTER_TRASH);
  const entries = await listChapterTrash(bookDir);
  let picked: ChapterTrashEntry[];
  if (target.all === true) {
    picked = entries;
  } else if (typeof target.batch === "string") {
    if (!ROLLBACK_BATCH_RE.test(target.batch)) throw invalidRequest("Invalid trash batch");
    picked = entries.filter((entry) => entry.batch === target.batch);
    if (picked.length === 0) throw notFound(target.batch);
  } else if (typeof target.path === "string") {
    if (originalPathOfTrashEntry(target.path) === null) throw invalidRequest("Invalid trash path");
    picked = entries.filter((entry) => entry.path === target.path);
    if (picked.length === 0) throw notFound(target.path);
  } else {
    throw invalidRequest("Expected { path }, { batch } or { all: true }");
  }
  for (const entry of picked) await rm(join(bookDir, entry.path));
  await pruneEmptyDirs(join(bookDir, CHAPTER_TRASH), picked.map((entry) => dirname(join(bookDir, entry.path))));
  return { removed: picked.length };
}
