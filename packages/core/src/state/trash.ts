import { mkdir, readdir, readFile, rename, stat } from "node:fs/promises";
import { join } from "node:path";
import { loadWorkManifest, workDirectory } from "../harness/work-store.js";
import type { AtomicFileWrite } from "../utils/atomic-file-set.js";

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
