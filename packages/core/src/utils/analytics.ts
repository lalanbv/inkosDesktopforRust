import type { ChapterMeta } from "../models/chapter.js";

export interface TokenStats {
  readonly totalPromptTokens: number;
  readonly totalCompletionTokens: number;
  readonly totalTokens: number;
  readonly avgTokensPerChapter: number;
  readonly recentTrend: ReadonlyArray<{ readonly chapter: number; readonly totalTokens: number }>;
}

// 792 号：对齐 Rust compute_analytics 权威形态（engine-rs utils/analytics.rs）——
// 剔除 observationCount/chaptersWithObservations（Rust 已删），补
// topIssueCategories/chaptersWithMostIssues/statusDistribution；avgTokensPerChapter
// 改整除截断（Rust u64 除法镜像，Math.round 四舍五入=contract-diff 活体分歧）。

export interface IssueCategoryCount {
  readonly category: string;
  readonly count: number;
}

export interface ChapterIssueCount {
  readonly chapter: number;
  readonly issueCount: number;
}

export interface AnalyticsData {
  readonly bookId: string;
  readonly totalChapters: number;
  readonly totalWords: number;
  readonly avgWordsPerChapter: number;
  // 717 号：审查通过率（fork 逐字 status 基计算；Rust analytics audit_pass_rate
  // 同字段——book-eval qualityScore 加权消费面）。
  readonly auditPassRate: number;
  readonly topIssueCategories: ReadonlyArray<IssueCategoryCount>;
  readonly chaptersWithMostIssues: ReadonlyArray<ChapterIssueCount>;
  readonly statusDistribution: Readonly<Record<string, number>>;
  readonly tokenStats?: TokenStats;
}

const PASSED_STATUSES = new Set(["ready-for-review", "approved", "published"]);
const EXCLUDED_FROM_AUDITED = new Set(["drafted", "drafting", "card-generated"]);
// Rust issue_category_re 镜像：`[级别] 分类[:：]`（全半角冒号都认）。
const ISSUE_CATEGORY_RE = /\[(?:critical|warning|info)\]\s*(.+?)[:：]/u;

export function computeAnalytics(bookId: string, chapters: ReadonlyArray<ChapterMeta>): AnalyticsData {
  const totalChapters = chapters.length;
  const totalWords = chapters.reduce((sum, chapter) => sum + chapter.wordCount, 0);
  const avgWordsPerChapter = totalChapters > 0 ? Math.round(totalWords / totalChapters) : 0;

  const audited = chapters.filter((chapter) => !EXCLUDED_FROM_AUDITED.has(chapter.status ?? ""));
  const passed = audited.filter((chapter) => PASSED_STATUSES.has(chapter.status ?? ""));
  const auditPassRate = audited.length > 0 ? Math.round((passed.length / audited.length) * 100) : 100;

  // 问题分类计数：插入序 + stable sort 处理并列（Rust Vec+stable sort_by 镜像）。
  const categoryCounts = new Map<string, number>();
  for (const chapter of chapters) {
    for (const issue of chapter.auditIssues ?? []) {
      const category = ISSUE_CATEGORY_RE.exec(issue)?.[1] ?? "未分类";
      categoryCounts.set(category, (categoryCounts.get(category) ?? 0) + 1);
    }
  }
  const topIssueCategories = [...categoryCounts.entries()]
    .sort((left, right) => right[1] - left[1])
    .slice(0, 10)
    .map(([category, count]) => ({ category, count }));

  // 问题最多的章节（issueCount desc，并列保原序——Array.sort stable 镜像）。
  const chaptersWithMostIssues = chapters
    .filter((chapter) => (chapter.auditIssues?.length ?? 0) > 0)
    .sort((left, right) => (right.auditIssues?.length ?? 0) - (left.auditIssues?.length ?? 0))
    .slice(0, 5)
    .map((chapter) => ({ chapter: chapter.number, issueCount: chapter.auditIssues?.length ?? 0 }));

  // 状态分布（Rust from_meta status unwrap_or_default → 空串镜像）。
  const statusDistribution: Record<string, number> = {};
  for (const chapter of chapters) {
    const status = chapter.status ?? "";
    statusDistribution[status] = (statusDistribution[status] ?? 0) + 1;
  }

  // token 统计（仅有 tokenUsage 的章节参与）。
  const chaptersWithUsage = chapters.filter((chapter) => chapter.tokenUsage);
  const totalTokens = chaptersWithUsage.reduce((sum, chapter) => sum + (chapter.tokenUsage?.totalTokens ?? 0), 0);
  const tokenStats = chaptersWithUsage.length > 0
    ? {
        totalPromptTokens: chaptersWithUsage.reduce((sum, chapter) => sum + (chapter.tokenUsage?.promptTokens ?? 0), 0),
        totalCompletionTokens: chaptersWithUsage.reduce((sum, chapter) => sum + (chapter.tokenUsage?.completionTokens ?? 0), 0),
        totalTokens,
        avgTokensPerChapter: Math.trunc(totalTokens / chaptersWithUsage.length),
        recentTrend: [...chaptersWithUsage]
          .sort((left, right) => left.number - right.number)
          .slice(-5)
          .map((chapter) => ({ chapter: chapter.number, totalTokens: chapter.tokenUsage?.totalTokens ?? 0 })),
      }
    : undefined;

  return {
    bookId,
    totalChapters,
    totalWords,
    avgWordsPerChapter,
    auditPassRate,
    topIssueCategories,
    chaptersWithMostIssues,
    statusDistribution,
    ...(tokenStats ? { tokenStats } : {}),
  };
}
