// 682 号：章节文件名前导号解析——「能列出（索引宽松侧）即可打开（详情侧）」
// 的单一事实源。引擎写入侧恒 4 位补零（NNNN_title.md），但导入/手工放置的
// 章节文件位数不限；索引重建（manager.rebuildChapterIndexFromFiles）与 studio
// 详情路由必须消费同一解析，否则出现「列表可见、详情 404」缺口（681 号 P3
// 备案）。旧详情侧 startsWith(补零串) 的前缀碰撞（num=7 误配 00071_）按数值
// 等值比较在此根除。
export const CHAPTER_FILE_PATTERN = /^(\d+)[_-]?(.*)\.md$/;

export function parseChapterFileName(fileName: string): { number: number; base: string } | null {
  const match = CHAPTER_FILE_PATTERN.exec(fileName);
  if (!match) return null;
  const number = Number.parseInt(match[1]!, 10);
  if (!Number.isFinite(number) || number <= 0) return null;
  return { number, base: match[2] ?? "" };
}

export function findChapterFileByNumber(files: ReadonlyArray<string>, chapterNumber: number): string | undefined {
  const candidates = files
    .filter((file) => parseChapterFileName(file)?.number === chapterNumber)
    .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  // 同号多形态（0007/007/7 并存）确定性择优：4 位补零（引擎主形态）优先，其余字典序。
  return candidates.find((file) => /^0{3}\d/.test(file)) ?? candidates[0];
}
