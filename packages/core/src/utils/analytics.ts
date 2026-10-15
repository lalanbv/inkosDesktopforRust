import type { ChapterMeta } from "../models/chapter.js";

export interface TokenStats {
  readonly totalPromptTokens: number;
  readonly totalCompletionTokens: number;
  readonly totalTokens: number;
  readonly avgTokensPerChapter: number;
  readonly recentTrend: ReadonlyArray<{ readonly chapter: number; readonly totalTokens: number }>;
}

export interface AnalyticsData {
  readonly bookId: string;
  readonly totalChapters: number;
  readonly totalWords: number;
  readonly avgWordsPerChapter: number;
  readonly observationCount: number;
  readonly chaptersWithObservations: number;
  // 717 号：审查通过率（fork 逐字 status 基计算；Rust analytics audit_pass_rate
  // 同字段——book-eval qualityScore 加权消费面）。
  readonly auditPassRate: number;
  readonly tokenStats?: TokenStats;
}

export function computeAnalytics(bookId: string, chapters: ReadonlyArray<ChapterMeta>): AnalyticsData {
  const totalChapters = chapters.length;
  const totalWords = chapters.reduce((sum, chapter) => sum + chapter.wordCount, 0);
  const chaptersWithUsage = chapters.filter((chapter) => chapter.tokenUsage);
  const totalTokens = chaptersWithUsage.reduce((sum, chapter) => sum + (chapter.tokenUsage?.totalTokens ?? 0), 0);
  const tokenStats = chaptersWithUsage.length > 0
    ? {
        totalPromptTokens: chaptersWithUsage.reduce((sum, chapter) => sum + (chapter.tokenUsage?.promptTokens ?? 0), 0),
        totalCompletionTokens: chaptersWithUsage.reduce((sum, chapter) => sum + (chapter.tokenUsage?.completionTokens ?? 0), 0),
        totalTokens,
        avgTokensPerChapter: Math.round(totalTokens / chaptersWithUsage.length),
        recentTrend: [...chaptersWithUsage]
          .sort((left, right) => left.number - right.number)
          .slice(-5)
          .map((chapter) => ({ chapter: chapter.number, totalTokens: chapter.tokenUsage?.totalTokens ?? 0 })),
      }
    : undefined;
  // 717 号：审查通过率（fork 逐字）——非草稿章中 status 进入通过态的比例。
  const passedStatuses = new Set(["ready-for-review", "approved", "published"]);
  const auditedChapters = chapters.filter(
    (chapter) => chapter.status !== "drafted" && chapter.status !== "drafting" && chapter.status !== "card-generated",
  );
  const passedChapters = auditedChapters.filter((chapter) => chapter.status !== undefined && passedStatuses.has(chapter.status));
  const auditPassRate = auditedChapters.length > 0
    ? Math.round((passedChapters.length / auditedChapters.length) * 100)
    : 100;
  return {
    bookId,
    totalChapters,
    totalWords,
    avgWordsPerChapter: totalChapters > 0 ? Math.round(totalWords / totalChapters) : 0,
    observationCount: chapters.reduce((sum, chapter) => sum + chapter.observations.length, 0),
    chaptersWithObservations: chapters.filter((chapter) => chapter.observations.length > 0).length,
    auditPassRate,
    ...(tokenStats ? { tokenStats } : {}),
  };
}
