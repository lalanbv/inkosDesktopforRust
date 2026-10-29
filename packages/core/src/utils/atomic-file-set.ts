import {
  access,
  copyFile,
  cp,
  lstat,
  mkdir,
  mkdtemp,
  open,
  readFile,
  readdir,
  rename,
  rm,
} from "node:fs/promises";
import { dirname, isAbsolute, join, normalize, resolve, sep } from "node:path";
import { z } from "zod";

const PREFIX = ".inkos-file-txn-";
const JournalSchema = z.object({
  version: z.literal(1),
  pid: z.number().int().nonnegative(),
  phase: z.enum(["prepared", "committed"]),
  entries: z.array(z.object({ path: z.string(), existed: z.boolean() }).strict()),
}).strict();
type Journal = z.infer<typeof JournalSchema>;
const queues = new Map<string, Promise<unknown>>();

// 761 号：以下两个助手与 `./durable-fs.ts` 同源，此处就地内联而非导入——
// atomic-file-set.test 用 `node --experimental-strip-types` 直接加载本文件做
// 杀死写者恢复测试，Node 原生 TS 不映射相对 `.js` 导入到 `.ts`，本文件须
// 保持仅依赖内建/裸模块。

/** 写文件并 fsync：句柄写入 → sync → close。 */
async function writeFileDurable(path: string, data: string | Uint8Array): Promise<void> {
  const handle = await open(path, "w");
  try {
    await handle.writeFile(data);
    await handle.sync();
  } finally {
    await handle.close();
  }
}

/** 尽力 fsync 目录（rename 项落盘）；Windows/不支持的文件系统静默。 */
async function syncDirBestEffort(dir: string): Promise<void> {
  let handle: Awaited<ReturnType<typeof open>>;
  try {
    handle = await open(dir, "r");
  } catch {
    return;
  }
  try {
    await handle.sync();
  } catch {
    // 尽力语义
  } finally {
    await handle.close().catch(() => undefined);
  }
}

async function serialize<T>(root: string, task: () => Promise<T>): Promise<T> {
  const key = resolve(root);
  const prior = queues.get(key) ?? Promise.resolve();
  const next = prior.catch(() => undefined).then(task);
  queues.set(key, next);
  try { return await next; }
  finally { if (queues.get(key) === next) queues.delete(key); }
}

// 761 号：journal 内容既有 sync；rename 后再尽力 fsync 事务目录，让
// journal.json 的 rename 项本身落盘（journal 是掉电恢复的唯一依据）。
async function writeJournal(directory: string, journal: Journal): Promise<void> {
  const temporary = join(directory, "journal.next");
  const handle = await open(temporary, "w", 0o600);
  try { await handle.writeFile(JSON.stringify(journal)); await handle.sync(); }
  finally { await handle.close(); }
  await rename(temporary, join(directory, "journal.json"));
  await syncDirBestEffort(directory);
}

function processExists(pid: number): boolean {
  if (pid <= 0) return false;
  try { process.kill(pid, 0); return true; }
  catch (error) { return (error as NodeJS.ErrnoException).code !== "ESRCH"; }
}

async function rollback(root: string, directory: string, journal: Journal): Promise<void> {
  for (const entry of journal.entries) {
    const path = safeRelativePath(entry.path);
    const target = join(root, path);
    const backup = join(directory, "backup", path);
    if (await exists(backup)) {
      await mkdir(dirname(target), { recursive: true });
      // Keep the backup until the entire rollback completes, including if
      // recovery itself is interrupted and has to run again.
      if ((await lstat(backup)).isDirectory()) await cp(backup,target,{recursive:true,force:true});
      else await copyFile(backup, target);
    } else if (!entry.existed) {
      await rm(target, { force: true });
    }
  }
}

async function recoverRoot(root: string): Promise<number> {
  let entries;
  try { entries = await readdir(root, { withFileTypes: true }); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return 0;
    throw error;
  }
  let recovered = 0;
  for (const entry of entries) {
    if (!entry.isDirectory() || !entry.name.startsWith(PREFIX)) continue;
    const directory = join(root, entry.name);
    let journal: Journal;
    try { journal = JournalSchema.parse(JSON.parse(await readFile(join(directory, "journal.json"), "utf8"))); }
    catch (error) {
      // A transaction without a journal has not started replacing files.
      // Retain unrecognized old transactions for manual inspection.
      if ((error as NodeJS.ErrnoException).code === "ENOENT") continue;
      throw error;
    }
    if (processExists(journal.pid)) continue;
    if (journal.phase === "prepared") await rollback(root, directory, journal);
    await rm(directory, { recursive: true, force: true });
    recovered++;
  }
  return recovered;
}

/** Recover dead-owner file sets before reading a project after a restart. */
export async function recoverAtomicFileSets(root: string, recursive = false): Promise<number> {
  let count = await serialize(root, () => recoverRoot(root));
  if (!recursive || !(await exists(root))) return count;
  for (const entry of await readdir(root, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.name.startsWith(".") || entry.name === "node_modules") continue;
    count += await recoverAtomicFileSets(join(root, entry.name), true);
  }
  return count;
}

export interface AtomicFileWrite {
  readonly relativePath: string;
  readonly content: string | Uint8Array;
}

export interface AtomicFileSet {
  readonly rootDir: string;
  readonly writes: ReadonlyArray<AtomicFileWrite>;
  readonly deletes?: ReadonlyArray<string>;
  readonly renameFile?: (from: string, to: string) => Promise<void>;
}

function safeRelativePath(relativePath: string): string {
  const normalized = normalize(relativePath);
  if (
    !relativePath.trim()
    || isAbsolute(relativePath)
    || normalized === ".."
    || normalized.startsWith(`..${sep}`)
  ) {
    throw new Error(`Atomic file path must stay inside rootDir: ${relativePath}`);
  }
  return normalized;
}

async function exists(path: string): Promise<boolean> {
  try {
    await access(path);
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return false;
    throw error;
  }
}

export async function commitAtomicFileSet(input: AtomicFileSet): Promise<void> {
  return serialize(input.rootDir, async () => {
    await recoverRoot(input.rootDir);
    await commitFileSet(input);
  });
}

async function commitFileSet(input: AtomicFileSet): Promise<void> {
  const renameFile = input.renameFile ?? rename;
  const writes = input.writes.map((entry) => ({
    ...entry,
    relativePath: safeRelativePath(entry.relativePath),
  }));
  const deletes = (input.deletes ?? []).map(safeRelativePath);
  const writePaths = new Set(writes.map((entry) => entry.relativePath));
  if (writePaths.size !== writes.length) {
    throw new Error("Atomic file set contains duplicate write paths");
  }
  if (deletes.some((relativePath) => writePaths.has(relativePath))) {
    throw new Error("Atomic file set cannot write and delete the same path");
  }
  const allPaths=[...writePaths,...deletes];
  if (allPaths.some(path=>allPaths.some(other=>other!==path&&other.startsWith(path+sep)))) {
    throw Object.assign(new Error("Atomic file set cannot touch both a directory and its child paths"),{code:"ATOMIC_PATH_OVERLAP"});
  }

  await mkdir(input.rootDir, { recursive: true });
  const transactionDir = await mkdtemp(join(input.rootDir, PREFIX));
  const stagedDir = join(transactionDir, "staged");
  const backupDir = join(transactionDir, "backup");
  const touchedPaths = [...writePaths, ...deletes];
  const journal: Journal = {
    version: 1, pid: process.pid, phase: "prepared",
    entries: await Promise.all(touchedPaths.map(async (path) => ({ path, existed: await exists(join(input.rootDir, path)) }))),
  };
  let prepared = false;
  let cleanup = true;

  try {
    for (const entry of writes) {
      const stagedPath = join(stagedDir, entry.relativePath);
      await mkdir(dirname(stagedPath), { recursive: true });
      // 761 号：staged 内容 fsync 落盘后才进 rename 段——journal 恢复机制
      // 保证「回滚与否」的正确判定，但数据本身零长/半截它救不回来。
      await writeFileDurable(stagedPath, entry.content);
    }

    await writeJournal(transactionDir, journal);
    prepared = true;

    for (const relativePath of touchedPaths) {
      const target = join(input.rootDir, relativePath);
      await mkdir(dirname(target), { recursive: true });
      if (!(await exists(target))) continue;

      const backup = join(backupDir, relativePath);
      await mkdir(dirname(backup), { recursive: true });
      await renameFile(target, backup);
    }

    for (const entry of writes) {
      const target = join(input.rootDir, entry.relativePath);
      await renameFile(join(stagedDir, entry.relativePath), target);
    }
    await writeJournal(transactionDir, { ...journal, phase: "committed" });
    // 761 号：就位后尽力 fsync 各触碰目标的父目录（去重）——防掉电重放
    // 把「target→backup 已持久、staged→target 丢失」重放成目标缺失。
    const syncedDirs = new Set<string>();
    for (const relativePath of touchedPaths) {
      const parent = dirname(join(input.rootDir, relativePath));
      if (!syncedDirs.has(parent)) {
        syncedDirs.add(parent);
        await syncDirBestEffort(parent);
      }
    }
  } catch (error) {
    if (prepared) {
      try { await rollback(input.rootDir, transactionDir, journal); }
      catch (rollbackError) {
        cleanup = false;
        await writeJournal(transactionDir, { ...journal, pid: 0 });
        throw new AggregateError([error, rollbackError], "Atomic file commit failed; recovery journal retained");
      }
    }
    throw error;
  } finally {
    if (cleanup) await rm(transactionDir, { recursive: true, force: true }).catch(() => undefined);
  }
}
