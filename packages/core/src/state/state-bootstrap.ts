import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";

import {
  ChapterSummariesStateSchema,
  CurrentStateStateSchema,
  HookStatusSchema,
  HooksStateSchema,
  StateManifestSchema,
} from "../models/runtime-state.js";
import { commitAtomicFileSet } from "../utils/atomic-file-set.js";
import {
  isCurrentChapterLabel,
  parseChapterSummariesMarkdown,
  parseCurrentStateFacts,
  parseInteger,
  parseMarkdownTableRows,
  parsePendingHooksMarkdown,
} from "../utils/story-markdown.js";
import type { RuntimeStateSnapshot } from "./state-reducer.js";

export async function resolveDurableStoryProgress(params: {
  readonly bookDir: string;
}): Promise<number> {
  const chaptersDir = join(params.bookDir, "chapters");
  // 789 号：index 缺失/损坏不再整体降级为 0（旧行为会让 index-less legacy 书的
  // getNextChapterNumber 返回 1，write-next 覆盖既有第 1 章——788 号登记项），
  // 改为容忍降级到文件名 `N_` 前缀的连续前缀（durable 产物链权威 = 章节文件本身）。
  // index 有效时保留 2.0 一致性守卫（index/files 不一致即抛——fail-closed 完整性面，
  // 比 Rust 的无守卫合并更严，属刻意保留的不对称）。
  let indexNumbers: number[] | null = null;
  try {
    const indexRaw = await readFile(join(chaptersDir, "index.json"), "utf-8");
    const parsed = JSON.parse(indexRaw) as unknown;
    if (!Array.isArray(parsed)) {
      indexNumbers = null;
    } else {
      const numbers: number[] = [];
      let allValid = true;
      for (const entry of parsed) {
        const number = (entry as { number?: unknown } | null)?.number;
        if (!Number.isInteger(number) || (number as number) <= 0) {
          allValid = false;
          break;
        }
        numbers.push(number as number);
      }
      indexNumbers = allValid ? numbers : null;
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT" && !(error instanceof SyntaxError)) {
      throw error;
    }
    indexNumbers = null;
  }
  const entries = await readdir(chaptersDir).catch((error) => {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return [] as string[];
    throw error;
  });
  const fileNumbers = entries.flatMap((entry) => {
    const match = /^(\d+)_/u.exec(entry);
    return match ? [Number.parseInt(match[1]!, 10)] : [];
  });
  if (indexNumbers === null) {
    // index 缺失/损坏：纯文件前缀（真 fresh 书两表皆空 → 0，语义不变）。
    return resolveContiguousChapterPrefix(fileNumbers);
  }
  const indexed = new Set(indexNumbers);
  const files = new Set(fileNumbers);
  const orphaned = [...files].filter((number) => !indexed.has(number));
  const missing = [...indexed].filter((number) => !files.has(number));
  if (orphaned.length > 0 || missing.length > 0) {
    throw new Error(`Chapter index/files disagree (orphaned=${orphaned.join(",") || "none"}; missing=${missing.join(",") || "none"}).`);
  }
  return resolveContiguousChapterPrefix(indexNumbers);
}

export function resolveContiguousChapterPrefix(chapterNumbers: ReadonlyArray<number>): number {
  const chapters = new Set(
    chapterNumbers.filter((chapter): chapter is number => Number.isInteger(chapter) && chapter > 0),
  );
  let contiguousChapter = 0;
  while (chapters.has(contiguousChapter + 1)) contiguousChapter += 1;
  return contiguousChapter;
}

// =============================================================================
// 788 号：bootstrapStructuredStateFromMarkdown——Rust ensure 语义的 TS 镜像
//（engine-rs state_bootstrap.rs 对齐移植）。legacy 书（状态四件缺失或损坏）
// 从 markdown 真相面派生并物化四 JSON；物化一次后 TS 腿与 Rust 腿行为收敛。
//
// 与 saveRuntimeStateSnapshot 的关键差异：**只写四个 JSON，不重写 markdown
// 投影**（Rust bootstrap 同款）——markdown 是真相源，parse→render 往返会洗掉
// 解析器未覆盖的内容。四件全部已存在且 manifest 进度与 durable 链一致时零写
// 返回（785 号快路径镜像）。
// =============================================================================

/** 容忍读：ENOENT/损坏 JSON/schema 不符 → null（Rust load_json_if_valid 同语义：
 * 缺失或无效都视为待重建）；其余 IO 错误传播。 */
export async function readStateJsonTolerant<T>(
  path: string,
  schema: { parse(value: unknown): T },
): Promise<T | null> {
  let raw: string;
  try {
    raw = await readFile(path, "utf-8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
  try {
    return schema.parse(JSON.parse(raw));
  } catch (error) {
    if (error instanceof SyntaxError) return null;
    if (error instanceof Error && error.name === "ZodError") return null;
    throw error;
  }
}

/** 读 story/*.md 真相源；缺失 → 空串（Rust load_markdown_* 同语义）。 */
async function readBookMarkdown(bookDir: string, name: string): Promise<string> {
  try {
    return await readFile(join(bookDir, "story", name), "utf-8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return "";
    throw error;
  }
}

/** durable 产物前缀（容忍版）：index.json 缺失/损坏 → 空表，文件名 `N_` 前缀
 * 合并后取连续前缀（Rust load_durable_artifact_chapter_numbers 镜像）。
 * 与 [`resolveDurableStoryProgress`] 的差异：后者 index 缺失即 0 且 index/files
 * 不一致即抛——那是写链编号的完整性守卫语义（保留不动，788 号登记项）；
 * bootstrap 面对的是 legacy 半态书，须容忍合并。 */
async function resolveDurableArtifactProgressTolerant(bookDir: string): Promise<number> {
  const chaptersDir = join(bookDir, "chapters");
  const indexNumbers = await readFile(join(chaptersDir, "index.json"), "utf-8")
    .then((raw) => {
      try {
        const parsed = JSON.parse(raw) as unknown;
        if (!Array.isArray(parsed)) return [] as number[];
        return parsed.flatMap((entry) => {
          const number = (entry as { number?: unknown } | null)?.number;
          return typeof number === "number" && Number.isInteger(number) && number > 0
            ? [number]
            : [];
        });
      } catch {
        return [] as number[];
      }
    })
    .catch((error) => {
      if ((error as NodeJS.ErrnoException).code === "ENOENT") return [] as number[];
      throw error;
    });
  const entries = await readdir(chaptersDir).catch((error) => {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return [] as string[];
    throw error;
  });
  const fileNumbers = entries.flatMap((entry) => {
    const match = /^(\d+)_/u.exec(entry);
    return match ? [Number.parseInt(match[1]!, 10)] : [];
  });
  return resolveContiguousChapterPrefix([...indexNumbers, ...fileNumbers]);
}

/** language：existing manifest 合法值优先，否则 book.json（仅 "zh" 认定，其余
 * "en"）——Rust resolve_runtime_language 镜像。 */
async function resolveRuntimeLanguage(
  bookDir: string,
  existingLanguage: string | undefined,
): Promise<"zh" | "en"> {
  if (existingLanguage === "zh" || existingLanguage === "en") return existingLanguage;
  let raw = "";
  try {
    raw = await readFile(join(bookDir, "book.json"), "utf-8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw) as unknown;
  } catch {
    return "en";
  }
  const language = (parsed as { language?: unknown } | null)?.language;
  return language === "zh" ? "zh" : "en";
}

/** summaries 行去重（Rust deduplicate_summary_rows 镜像）：同 chapter 保留
 * 最后一次出现（后覆盖），顺序保持。 */
function deduplicateSummaryRows<T extends { readonly chapter: number }>(rows: ReadonlyArray<T>): T[] {
  const byChapter = new Map<number, T>();
  for (const row of rows) byChapter.set(row.chapter, row);
  return [...byChapter.values()];
}

export async function bootstrapStructuredStateFromMarkdown(params: {
  readonly bookDir: string;
}): Promise<RuntimeStateSnapshot> {
  const bookDir = params.bookDir;
  const stateDir = join(bookDir, "story", "state");

  const [existingManifest, existingCurrentState, existingHooks, existingSummaries] =
    await Promise.all([
      readStateJsonTolerant(join(stateDir, "manifest.json"), StateManifestSchema),
      readStateJsonTolerant(join(stateDir, "current_state.json"), CurrentStateStateSchema),
      readStateJsonTolerant(join(stateDir, "hooks.json"), HooksStateSchema),
      readStateJsonTolerant(join(stateDir, "chapter_summaries.json"), ChapterSummariesStateSchema),
    ]);

  const language = await resolveRuntimeLanguage(bookDir, existingManifest?.language);
  const durableChapter = await resolveDurableArtifactProgressTolerant(bookDir);

  // summaries：existing 去重（Rust deduplicate_summary_rows 镜像——同 chapter 后
  // 覆盖；去重命中即重写，对应 Rust fast-path 查重→慢路径去重的语义，790 号审计
  // 补齐）；缺失/损坏 → markdown 派生（chapter≥1 且标题非空——
  // ChapterSummaryRowSchema strict 约束；tension 列为 2.0 schema 外字段，剥离）。
  const rawSummaries =
    existingSummaries ??
    {
      rows: parseChapterSummariesMarkdown(await readBookMarkdown(bookDir, "chapter_summaries.md"))
        .filter((row) => row.chapter >= 1 && row.title.length > 0)
        .map(({ conflictLevel: _conflictLevel, revealLevel: _revealLevel, ...row }) => row),
    };
  const dedupedRows = deduplicateSummaryRows(rawSummaries.rows);
  const summaries = { rows: dedupedRows };
  const summariesRewritten =
    !existingSummaries || dedupedRows.length !== rawSummaries.rows.length;

  // hooks：existing 保留；缺失/损坏 → markdown 派生（type 空白 → "unspecified"、
  // status 词表外 → "open"——HookRecordSchema 约束镜像 Rust normalize_hook_type）。
  const hooks =
    existingHooks ??
    {
      hooks: parsePendingHooksMarkdown(await readBookMarkdown(bookDir, "pending_hooks.md")).map(
        (hook) => ({
          hookId: hook.hookId,
          startChapter: hook.startChapter,
          type: hook.type.trim().length > 0 ? hook.type.trim() : "unspecified",
          status: (HookStatusSchema.options as ReadonlyArray<string>).includes(hook.status)
            ? (hook.status as (typeof HookStatusSchema.options)[number])
            : ("open" as const),
          lastAdvancedChapter: hook.lastAdvancedChapter,
          expectedPayoff: hook.expectedPayoff,
          notes: hook.notes,
          ...(hook.dependsOn ? { dependsOn: [...hook.dependsOn] } : {}),
          ...(hook.paysOffInArc !== undefined ? { paysOffInArc: hook.paysOffInArc } : {}),
        }),
      ),
    };

  // currentState：existing 保留；缺失/损坏 → markdown 派生（chapter=「当前章节」
  // 行 || durable——镜像 parseCurrentStateFacts 内嵌的 parseInteger||fallback）。
  const currentMarkdown = await readBookMarkdown(bookDir, "current_state.md");
  const currentState = existingCurrentState ?? {
    chapter:
      parseInteger(
        parseMarkdownTableRows(currentMarkdown).find((row) => isCurrentChapterLabel(row[0] ?? ""))?.[1],
      ) || durableChapter,
    facts: parseCurrentStateFacts(currentMarkdown, durableChapter),
  };

  // manifest：fresh（schema2 由 parse 保证 + language 词表 + lastApplied==durable）
  // → 保留（785 快路径镜像）；否则按 durable 重算（projectionVersion 保留）。
  const manifest =
    existingManifest && existingManifest.lastAppliedChapter === durableChapter
      ? existingManifest
      : {
          schemaVersion: 2 as const,
          language,
          lastAppliedChapter: durableChapter,
          projectionVersion: existingManifest?.projectionVersion ?? 1,
        };

  // 只物化缺失/损坏的件 + 去重命中重写 + 重算过的 manifest；全 fresh → 零写
  // （不触 markdown）。
  const writes: Array<{ readonly relativePath: string; readonly content: string }> = [];
  if (summariesRewritten) {
    writes.push({
      relativePath: join("story", "state", "chapter_summaries.json"),
      content: `${JSON.stringify(summaries, null, 2)}\n`,
    });
  }
  if (!existingHooks) {
    writes.push({
      relativePath: join("story", "state", "hooks.json"),
      content: `${JSON.stringify(hooks, null, 2)}\n`,
    });
  }
  if (!existingCurrentState) {
    writes.push({
      relativePath: join("story", "state", "current_state.json"),
      content: `${JSON.stringify(currentState, null, 2)}\n`,
    });
  }
  if (manifest !== existingManifest) {
    writes.push({
      relativePath: join("story", "state", "manifest.json"),
      content: `${JSON.stringify(manifest, null, 2)}\n`,
    });
  }
  if (writes.length > 0) {
    await commitAtomicFileSet({ rootDir: bookDir, writes });
  }

  return { manifest, currentState, hooks, chapterSummaries: summaries };
}
