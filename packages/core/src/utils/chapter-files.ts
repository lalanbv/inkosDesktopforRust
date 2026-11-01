/**
 * 813 号：章节文件识别的单一真源（TS 侧）——与 Rust `state::manager::leading_number` /
 * `select_chapter_files` 同口径。
 *
 * - 章文件形态：正典 `NNNN_标题.md`（writer 落盘）/ legacy `NNNN-标题.md` / 裸 `NNNN.md`
 *   （与索引重建、回滚删章、writer 同章旧文件清理共用同一集合——807 号原则「读侧认什么
 *   为章，删侧与写侧必须认同一集合」）。
 * - 同章双形态去重：正典优先，其次文件名序；章号 0 跳过。
 */

const CHAPTER_FILE_RE = /^(\d+)[_-]?(.*?)\.md$/u;
const CANONICAL_CHAPTER_FILE_RE = /^\d+_.*\.md$/u;

/** 章节文件的前导章号；非章文件 → undefined。 */
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

/**
 * 同章双形态去重：同一前导章号多个文件时正典 `NNNN_` 优先，其次文件名序；章号 0 跳过。
 * 返回按章号升序的唯一章文件名（Rust `select_chapter_files` 同策略）。
 */
export function selectChapterFiles(names: ReadonlyArray<string>): string[] {
  const byNumber = new Map<number, string>();
  for (const name of [...names].sort()) {
    const number = chapterFileNumber(name);
    if (number === undefined || number <= 0) continue;
    const current = byNumber.get(number);
    if (current === undefined || (!isCanonicalChapterFile(current) && isCanonicalChapterFile(name))) {
      byNumber.set(number, name);
    }
  }
  return [...byNumber.entries()].sort((a, b) => a[0] - b[0]).map(([, name]) => name);
}
