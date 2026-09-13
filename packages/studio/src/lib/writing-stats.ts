/**
 * R13 写作数据面板（382 号，三轮 P1；v3 §3 R13）——纯聚合函数。
 *
 * 数据源（全部既有）：章节索引（wordCount/updatedAt/status/tokenUsage）。
 * 产出：日/周字数节奏、审查通过率、token 成本聚合。零 LLM、零新存储。
 *
 * 纯函数双端可镜像（TS 供 Dashboard 卡经 /writing-stats 端点消费）。
 */

export interface ChapterStatRow {
  /** ISO 更新时间（含日期部分即可）。 */
  readonly updatedAt: string;
  readonly wordCount: number;
  /** audit-passed / ready-for-review / approved / published 视为通过。 */
  readonly status: string;
  readonly totalTokens?: number;
}

export interface DailyBucket {
  readonly date: string;
  readonly words: number;
  readonly chapters: number;
}

export interface WritingStats {
  /** 近 30 天按日聚合（日期升序；无产出的日期不出现）。 */
  readonly daily: ReadonlyArray<DailyBucket>;
  /** 近 30 天总字数。 */
  readonly words30d: number;
  /** 近 30 天完成章数。 */
  readonly chapters30d: number;
  /** 审查通过率（passed / total，一位小数百分比 0–100；无章 undefined）。 */
  readonly passRate?: number;
  readonly totalWords: number;
  readonly totalTokens: number;
  readonly totalChapters: number;
}

const PASSING = new Set(["audit-passed", "ready-for-review", "approved", "published"]);
const DAY_MS = 86_400_000;

function dateOf(iso: string): string {
  return iso.slice(0, 10);
}

/** 写作数据聚合（纯函数）。windowDays 限定节奏窗口（缺省 30）。 */
export function aggregateWritingStats(
  rows: ReadonlyArray<ChapterStatRow>,
  nowIso: string,
  windowDays = 30,
): WritingStats {
  const today = dateOf(nowIso);
  const windowStart = new Date(new Date(`${today}T00:00:00Z`).getTime() - (windowDays - 1) * DAY_MS)
    .toISOString()
    .slice(0, 10);

  const dailyMap = new Map<string, { words: number; chapters: number }>();
  let words30d = 0;
  let chapters30d = 0;
  let passed = 0;
  let totalWords = 0;
  let totalTokens = 0;

  for (const row of rows) {
    const date = dateOf(row.updatedAt);
    const inWindow = date >= windowStart && date <= today;
    if (inWindow) {
      const bucket = dailyMap.get(date) ?? { words: 0, chapters: 0 };
      bucket.words += row.wordCount;
      bucket.chapters += 1;
      dailyMap.set(date, bucket);
      words30d += row.wordCount;
      chapters30d += 1;
    }
    totalWords += row.wordCount;
    totalTokens += row.totalTokens ?? 0;
    if (PASSING.has(row.status)) passed += 1;
  }

  const daily = [...dailyMap.entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([date, bucket]) => ({ date, words: bucket.words, chapters: bucket.chapters }));

  return {
    daily,
    words30d,
    chapters30d,
    passRate: rows.length > 0 ? Math.round((passed / rows.length) * 1000) / 10 : undefined,
    totalWords,
    totalTokens,
    totalChapters: rows.length,
  };
}
