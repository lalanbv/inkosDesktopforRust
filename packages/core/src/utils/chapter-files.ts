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
 * - 854 号双端恒等补锁：类内文件名序改码点序（Rust `&str` 字节序恒等）、章号上界对齐 u32、
 *   数字类显式 ASCII——此前三处恒等只在 ASCII/BMP 文件名与小章号下靠平台巧合成立。
 */

// 854 号：JS `\d` 恒为 ASCII `[0-9]`；Rust 侧 `manager::rebuild_file_re` 同步改用显式
// `[0-9]`（regex crate 的 `\d` 默认 Unicode 数字类），两端对同一文件名的前导号匹配恒等。
const CHAPTER_FILE_RE = /^([0-9]+)[_-]?(.*?)\.md$/u;
const CANONICAL_CHAPTER_FILE_RE = /^[0-9]+_.*\.md$/u;

/**
 * 854 号：章号上界与 Rust `leading_number` 的 `parse::<u32>` 恒等——超出 u32 的前导号在
 * Rust 侧解析失败即「非章文件」，TS 侧 `parseInt` 却给出 1e20 等浮点整数并列为章
 *（`Number.isInteger(1e20) === true`），双引擎索引分叉；此处按同一上界拒绝。
 */
const MAX_CHAPTER_NUMBER = 0xffff_ffff;

function leadingChapterNumber(match: RegExpMatchArray): number | undefined {
  const number = Number.parseInt(match[1]!, 10);
  return Number.isInteger(number) && number <= MAX_CHAPTER_NUMBER ? number : undefined;
}

/**
 * 854 号：与 Rust `&str` 字节序（= 码点序，UTF-8 保序）恒等的文件名比较器。JS `<` 按
 * UTF-16 码元序，增补平面字符（代理对 0xD800-0xDFFF，如 emoji）与 U+E000-U+FFFF
 *（含全角标点 `！（），：`）的相对次序与码点序**相反**——同章同类双文件
 * `0001_开局！.md` / `0001_开局🔥.md` 旧实现 TS 取 emoji 文件、Rust 取全角叹号文件，
 * 双引擎开不同章。逐码点比较，零中间分配（不展开数组、不转码）。
 */
function compareByCodePoint(a: string, b: string): number {
  const length = Math.min(a.length, b.length);
  let index = 0;
  while (index < length) {
    const codeA = a.codePointAt(index)!;
    const codeB = b.codePointAt(index)!;
    if (codeA !== codeB) return codeA - codeB;
    index += codeA > 0xffff ? 2 : 1;
  }
  return a.length - b.length;
}

/** 章节文件的前导章号；非章文件 / 超 u32 → undefined（0 号不在此过滤，由调用方裁决）。 */
export function chapterFileNumber(name: string): number | undefined {
  const match = name.match(CHAPTER_FILE_RE);
  return match ? leadingChapterNumber(match) : undefined;
}

/** 正典章节文件名（`NNNN_标题.md`，writer 落盘形态）。 */
export function isCanonicalChapterFile(name: string): boolean {
  return CANONICAL_CHAPTER_FILE_RE.test(name);
}

/** 682 号：前导号（须 > 0）+ 标题基名；非章文件 / 0 号 / 超 u32 → null。 */
export function parseChapterFileName(fileName: string): { number: number; base: string } | null {
  const match = fileName.match(CHAPTER_FILE_RE);
  if (!match) return null;
  const number = leadingChapterNumber(match);
  if (number === undefined || number <= 0) return null;
  return { number, base: match[2] ?? "" };
}

/**
 * 同章双形态择优——索引重建与读侧定位的唯一裁决：正典 `NNNN_` 优先，同类内文件名序小者
 * （码点序，与 Rust `chapter_file_candidate_wins` 的 `&str` 字节序恒等；见
 * {@link compareByCodePoint}——非 JS 默认的 UTF-16 码元序）。
 */
export function preferredChapterFile(current: string, candidate: string): string {
  const currentCanonical = isCanonicalChapterFile(current);
  const candidateCanonical = isCanonicalChapterFile(candidate);
  if (candidateCanonical !== currentCanonical) return candidateCanonical ? candidate : current;
  return compareByCodePoint(candidate, current) < 0 ? candidate : current;
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
