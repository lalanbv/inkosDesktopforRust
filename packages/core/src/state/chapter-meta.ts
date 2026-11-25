import type { ChapterMeta } from "../models/chapter.js";
import { parseChapterFileName } from "../utils/chapter-files.js";

/**
 * 从章文件名与正文生成一条目录条目（1679 号抽出：索引重建与回收站找回共用——此前这
 * 段只在 `StateManager.rebuildChapterIndexFromFilesAt` 里内联）。
 *
 * 与产品引擎 `state::manager::rebuild_chapter_index_from_files_at` 同口径：标题取文件名
 * 余部（去前导 `_`、`_`→空格，空 → `第N章`），词数 = 去全部空白后的码元数，时间戳由
 * 调用方给（重建用文件修改时间）。不是章文件 / 0 号 → null。
 */
export function chapterMetaFromFile(name: string, content: string, timestamp: string): ChapterMeta | null {
  const parsed = parseChapterFileName(name);
  if (!parsed) return null;
  const { number } = parsed;
  const stripped = parsed.base.startsWith("_") ? parsed.base.slice(1) : parsed.base;
  const spaced = stripped.replace(/_/g, " ").trim();
  return {
    number,
    title: spaced.length > 0 ? spaced : `第${number}章`,
    status: "ready-for-review",
    wordCount: content.replace(/\s+/g, "").length,
    createdAt: timestamp,
    updatedAt: timestamp,
    auditIssues: [],
    lengthWarnings: [],
  };
}
