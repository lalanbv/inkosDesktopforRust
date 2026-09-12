import { z } from "zod";

export const PlatformSchema = z.enum(["tomato", "feilu", "qidian", "other"]);
export type Platform = z.infer<typeof PlatformSchema>;

export function normalizePlatformId(platform: unknown): Platform | undefined {
  if (typeof platform !== "string") {
    return undefined;
  }

  const raw = platform.trim();
  if (!raw) {
    return undefined;
  }

  const lowered = raw.toLowerCase();
  const compact = lowered.replace(/[\s_-]+/g, "");

  if (compact === "tomato" || compact === "fanqie" || compact === "fanqienovel" || raw.includes("番茄")) {
    return "tomato";
  }
  if (compact === "qidian" || compact === "qidianzhongwenwang" || raw.includes("起点")) {
    return "qidian";
  }
  if (compact === "feilu" || raw.includes("飞卢")) {
    return "feilu";
  }
  if (compact === "other" || compact === "others" || raw.includes("其他") || raw.includes("其它")) {
    return "other";
  }

  return "other";
}

export function normalizePlatformOrOther(platform: unknown): Platform {
  return normalizePlatformId(platform) ?? "other";
}

export const GenreSchema = z.string().min(1);
export type Genre = z.infer<typeof GenreSchema>;

export const BookStatusSchema = z.enum([
  "incubating",
  "outlining",
  "active",
  "paused",
  "completed",
  "dropped",
]);
export type BookStatus = z.infer<typeof BookStatusSchema>;

export const FanficModeSchema = z.enum(["canon", "au", "ooc", "cp"]);
export type FanficMode = z.infer<typeof FanficModeSchema>;

/** 系列归属（180 号 C3-a）：丛书名 + 卷序。缺省省略（单本散书）。 */
export const BookSeriesSchema = z.object({
  name: z.string().min(1),
  order: z.number().int().min(1),
});

export type BookSeries = z.infer<typeof BookSeriesSchema>;

export const BookConfigSchema = z.object({
  id: z.string().min(1),
  title: z.string().min(1),
  platform: PlatformSchema,
  genre: GenreSchema,
  status: BookStatusSchema,
  targetChapters: z.number().int().min(1).default(200),
  chapterWordCount: z.number().int().min(1000).default(3000),
  language: z.enum(["zh", "en"]).optional(),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime(),
  parentBookId: z.string().optional(),
  fanficMode: FanficModeSchema.optional(),
  series: BookSeriesSchema.optional(),
  writing: z.object({
    reviewMode: z.enum(["auto", "manual"]).optional(),
    revisionGate: z.enum(["strict", "lenient", "always"]).optional(),
    /** 189 号：write-next 落盘后自动为本章沉淀时间线节拍（默认关）。 */
    autoTimelineBeats: z.boolean().optional(),
    /** R11/378 号：场景节拍驱动写作（planner 产节拍、writer 按拍推进；默认关）。 */
    sceneBeats: z.boolean().optional(),
  }).optional(),
  /** G3/337 号：每书质量治理方案（缺省回落 completion-first/上限 3）。 */
  governance: z.object({
    policy: z.enum(["completion-first", "quality-first"]).optional(),
    maxConsecutiveDebts: z.number().int().min(1).max(20).optional(),
    /** G11/371 号：多版选优（首版分数低于 minScore 时追加候选重生成）。 */
    bestOfN: z
      .object({
        enabled: z.boolean().optional(),
        candidates: z.number().int().min(2).max(3).optional(),
        minScore: z.number().int().min(0).max(100).optional(),
      })
      .optional(),
  }).optional(),
});

export type BookConfig = z.infer<typeof BookConfigSchema>;

export type ChapterReviewMode = "auto" | "manual";

/**
 * Resolve the effective chapter review mode for a book:
 * book-level `writing.reviewMode` (book.json) overrides the project-level
 * `writing.reviewMode` (inkos.json); both unset falls back to "auto".
 */
export function resolveChapterReviewMode(
  book: Pick<BookConfig, "writing">,
  projectWriting?: { readonly reviewMode?: ChapterReviewMode },
): ChapterReviewMode {
  return book.writing?.reviewMode ?? projectWriting?.reviewMode ?? "auto";
}

export type RevisionGate = "strict" | "lenient" | "always";

/**
 * Resolve the effective manual-revision gate for a book:
 * book-level `writing.revisionGate` (book.json) overrides the project-level
 * `writing.revisionGate` (inkos.json); both unset falls back to "strict".
 *
 * - "strict": apply only when audit counts do not worsen AND at least one of
 *   blocking/AI-tell improves (historical default behavior).
 * - "lenient": apply whenever audit counts do not worsen (no improvement required).
 * - "always": always apply manual revisions; audit counts are recorded only.
 */
export function resolveRevisionGate(
  book: Pick<BookConfig, "writing">,
  projectWriting?: { readonly revisionGate?: RevisionGate },
): RevisionGate {
  return book.writing?.revisionGate ?? projectWriting?.revisionGate ?? "strict";
}
