/**
 * 813/845 号：章节文件识别与定位的单一真源（TS 侧）——与 Rust `state::manager::leading_number` /
 * `select_chapter_files` / `state::chapter_filename` 同口径。
 *
 * - 章文件形态：正典 `NNNN_标题.md`（writer 落盘）/ legacy `NNNN-标题.md` / 裸 `NNNN.md`
 *   （索引重建、回滚删章、writer 同章旧文件清理、读侧定位共用同一集合——807 号原则「读侧认什么
 *   为章，删侧与写侧必须认同一集合」）。
 * - 682 号「能列出即可打开」：引擎写入恒 4 位补零，导入/手工章节位数不限；读侧按前导号数值
 *   等值定位，旧 `startsWith(补零串)` 的前缀碰撞（num=7 误配 `00071_`）根除。
 * - 同章双形态择优（{@link preferredChapterFile}）：正典优先，同类内文件名序小者。索引重建
 *   （{@link selectChapterFiles}）与读侧定位（{@link findChapterFileByNumber}）共用这一个裁决——
 *   845 号前两者各持一套（682 读侧按 padded 类内字典序取 `'-'<'_'` 的连字符旧章，813 索引侧正典
 *   优先），同一本书出现「索引列 `0001_新`、详情开 `0001-旧`」缺口；此处合并为唯一真源。
 */

const CHAPTER_FILE_RE = /^(\d+)[_-]?(.*?)\.md$/u;
const CANONICAL_CHAPTER_FILE_RE = /^\d+_.*\.md$/u;

/** 章节文件的前导章号；非章文件 → undefined（0 号不在此过滤，由调用方裁决）。 */
export function chapterFileNumber(name: string): number | undefined {
  const match = CHAPTER_FILE_RE.exec(name);
  if (!match) return undefined;
  const number = Number.parseInt(match[1]!, 10);
  return Number.isInteger(number) ? number : undefined;
}

/** 正典章节文件名（`NNNN_标题.md`，writer 落盘形态）。 */
export function isCanonicalChapterFile(name: string): boolean {
  return CANONICAL_CHAPTER_FILE_RE.test(name);
}

/** 682 号：前导号（须 > 0）+ 标题基名；非章文件 / 0 号 → null。 */
export function parseChapterFileName(fileName: string): { number: number; base: string } | null {
  const match = CHAPTER_FILE_RE.exec(fileName);
  if (!match) return null;
  const number = Number.parseInt(match[1]!, 10);
  if (!Number.isInteger(number) || number <= 0) return null;
  return { number, base: match[2] ?? "" };
}

/**
 * 同章双形态择优——索引重建与读侧定位的唯一裁决：正典 `NNNN_` 优先，同类内文件名序小者
 * （与 `[...names].sort()` 同序，Rust `preferred_chapter_file` 同策略）。
 */
export function preferredChapterFile(current: string, candidate: string): string {
  const currentCanonical = isCanonicalChapterFile(current);
  const candidateCanonical = isCanonicalChapterFile(candidate);
  if (candidateCanonical !== currentCanonical) return candidateCanonical ? candidate : current;
  return candidate < current ? candidate : current;
}

/**
 * 同章双形态去重：同一前导章号多个文件时按 {@link preferredChapterFile} 择一；章号 0 跳过。
 * 返回按章号升序的唯一章文件名（Rust `select_chapter_files` 同策略）。
 */
export function selectChapterFiles(names: ReadonlyArray<string>): string[] {
  const byNumber = new Map<number, string>();
  for (const name of names) {
    const number = chapterFileNumber(name);
    if (number === undefined || number <= 0) continue;
    const current = byNumber.get(number);
    byNumber.set(number, current === undefined ? name : preferredChapterFile(current, name));
  }
  return [...byNumber.entries()].sort((a, b) => a[0] - b[0]).map(([, name]) => name);
}

/**
 * 682 号：按前导号数值等值定位单章文件，同号多形态按 {@link preferredChapterFile} 择一
 * （与 {@link selectChapterFiles} 对该章的胜者恒等）；无候选 / 非法章号 → undefined。
 * 单趟零中间分配，替代各读侧散落的 `files.find(startsWith(padded))`。
 */
export function findChapterFileByNumber(files: ReadonlyArray<string>, chapterNumber: number): string | undefined {
  if (!Number.isInteger(chapterNumber) || chapterNumber <= 0) return undefined;
  let best: string | undefined;
  for (const file of files) {
    if (chapterFileNumber(file) !== chapterNumber) continue;
    best = best === undefined ? file : preferredChapterFile(best, file);
  }
  return best;
}
