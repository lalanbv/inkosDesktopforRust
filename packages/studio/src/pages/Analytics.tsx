import { useApi } from "../hooks/use-api";
import type { Theme } from "../hooks/use-theme";
import type { TFunction } from "../hooks/use-i18n";
import { useColors } from "../hooks/use-colors";

interface AnalyticsData {
  readonly bookId: string;
  readonly totalChapters: number;
  readonly totalWords: number;
  readonly avgWordsPerChapter: number;
  readonly auditPassRate: number;
  readonly topIssueCategories: ReadonlyArray<{ readonly category: string; readonly count: number }>;
  readonly chaptersWithMostIssues: ReadonlyArray<{ readonly chapter: number; readonly issueCount: number }>;
  readonly statusDistribution: Readonly<Record<string, number>>;
  readonly tokenStats?: {
    readonly totalPromptTokens: number;
    readonly totalCompletionTokens: number;
    readonly totalTokens: number;
    readonly avgTokensPerChapter: number;
  };
}

interface Nav {
  toBook: (id: string) => void;
  toDashboard: () => void;
}

export function Analytics({ bookId, nav, theme, t }: { bookId: string; nav: Nav; theme: Theme; t: TFunction }) {
  const c = useColors(theme);
  const { data, loading, error } = useApi<AnalyticsData>(`/books/${bookId}/analytics`);

  if (loading) return <div className={c.muted}>{t("common.loading")}</div>;
  if (error) return <div className="text-red-400">{t("common.error")}: {error}</div>;
  if (!data) return null;

  return (
    <div className="space-y-6">
      <div className={`flex items-center gap-2 text-sm ${c.muted}`}>
        <button onClick={nav.toDashboard} className={c.link}>{t("bread.books")}</button>
        <span>/</span>
        <button onClick={() => nav.toBook(bookId)} className={c.link}>{bookId}</button>
        <span>/</span>
        <span className={c.subtle}>{t("analytics.title")}</span>
      </div>

      <h1 className="text-2xl font-semibold">{t("analytics.title")}</h1>

      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        <StatCard label={t("analytics.totalChapters")} value={data.totalChapters.toString()} c={c} />
        <StatCard label={t("analytics.totalWords")} value={data.totalWords.toLocaleString()} c={c} />
        <StatCard label={t("analytics.avgWords")} value={data.avgWordsPerChapter.toLocaleString()} c={c} />
        <StatCard label={t("analytics.auditPassRate")} value={`${data.auditPassRate}%`} c={c} />
      </div>

      <div className={`border ${c.cardStatic} rounded-lg p-5`}>
        <h2 className={`text-sm font-medium ${c.subtle} mb-4`}>{t("analytics.statusDistribution")}</h2>
        <div className="flex flex-wrap gap-2">
          {Object.entries(data.statusDistribution).map(([status, count]) => (
            <span key={status} className={`border ${c.cardStatic} rounded px-2 py-1 text-xs`}>
              {status} × {count}
            </span>
          ))}
          {Object.keys(data.statusDistribution).length === 0 && <span className={c.muted}>-</span>}
        </div>
      </div>

      <div className={`border ${c.cardStatic} rounded-lg p-5`}>
        <h2 className={`text-sm font-medium ${c.subtle} mb-4`}>{t("analytics.topIssues")}</h2>
        {data.topIssueCategories.length === 0 && <div className={c.muted}>-</div>}
        <ul className="space-y-1 text-sm">
          {data.topIssueCategories.map((entry) => (
            <li key={entry.category} className="flex justify-between">
              <span>{entry.category}</span>
              <span className="tabular-nums">{entry.count}</span>
            </li>
          ))}
        </ul>
      </div>

      <div className={`border ${c.cardStatic} rounded-lg p-5`}>
        <h2 className={`text-sm font-medium ${c.subtle} mb-4`}>{t("analytics.chaptersWithMostIssues")}</h2>
        {data.chaptersWithMostIssues.length === 0 && <div className={c.muted}>-</div>}
        <ul className="space-y-1 text-sm">
          {data.chaptersWithMostIssues.map((entry) => (
            <li key={entry.chapter} className="flex justify-between">
              <span>{t("analytics.chapterN").replace("{n}", String(entry.chapter))}</span>
              <span className="tabular-nums">{entry.issueCount}</span>
            </li>
          ))}
        </ul>
      </div>

      <div className={`border ${c.cardStatic} rounded-lg p-5`}>
        <h2 className={`text-sm font-medium ${c.subtle} mb-4`}>{t("analytics.runtimeEvidence")}</h2>
        <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
          <StatCard label={t("analytics.promptTokens")} value={(data.tokenStats?.totalPromptTokens ?? 0).toLocaleString()} c={c} />
          <StatCard label={t("analytics.completionTokens")} value={(data.tokenStats?.totalCompletionTokens ?? 0).toLocaleString()} c={c} />
          <StatCard label={t("analytics.totalTokens")} value={(data.tokenStats?.totalTokens ?? 0).toLocaleString()} c={c} />
          <StatCard label={t("analytics.avgTokens")} value={(data.tokenStats?.avgTokensPerChapter ?? 0).toLocaleString()} c={c} />
        </div>
      </div>
    </div>
  );
}

function StatCard({ label, value, c }: { label: string; value: string; c: ReturnType<typeof useColors> }) {
  return (
    <div className={`border ${c.cardStatic} rounded-lg p-5`}>
      <div className={`text-sm ${c.muted} mb-1`}>{label}</div>
      <div className="text-2xl font-semibold tabular-nums">{value}</div>
    </div>
  );
}
