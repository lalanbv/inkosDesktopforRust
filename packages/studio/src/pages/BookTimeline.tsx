import { useMemo } from "react";
import { useApi } from "../hooks/use-api";
import { ArrowLeft } from "lucide-react";
import type { Theme } from "../hooks/use-theme";
import type { TFunction } from "../hooks/use-i18n";
import type { Nav } from "../lib/nav";

interface ChapterMeta {
  readonly number: number;
  readonly title: string;
  readonly status: string;
  readonly wordCount: number;
}

interface BookData {
  readonly book: {
    readonly id: string;
    readonly title: string;
  };
  readonly chapters: ReadonlyArray<ChapterMeta>;
  readonly nextChapter: number;
}

/** 展示色分组（179 号分解文档 C4-a：单线兜底，多线待 C4-b timeline.json）。 */
type Tone = "done" | "review" | "failed" | "wip" | "imported";

function statusTone(status: string): Tone {
  if (status === "approved" || status === "published") return "done";
  if (status === "ready-for-review" || status === "audit-passed") return "review";
  if (status === "audit-failed" || status === "rejected" || status === "state-degraded") return "failed";
  if (status === "imported") return "imported";
  return "wip";
}

const TONE_CLASS: Record<Tone, string> = {
  done: "bg-emerald-500/10 border-emerald-500/40 text-emerald-600 dark:text-emerald-400",
  review: "bg-amber-500/10 border-amber-500/40 text-amber-600 dark:text-amber-400",
  failed: "bg-red-500/10 border-red-500/40 text-red-600 dark:text-red-400",
  wip: "bg-sky-500/10 border-sky-500/40 text-sky-600 dark:text-sky-400",
  imported: "bg-muted border-border text-muted-foreground",
};

/**
 * 书籍时间线（180 号 W-C4-a，对标 Plottr 的回顾型只读网格）。
 *
 * 行 = 情节线，列 = 章。当前数据源只有 chapters/index.json（无情节线字段），
 * 单条「主线」兜底；C4-b 引入 story/timeline.json 后在此扩展为多线。
 */
export function BookTimeline({ bookId, nav, theme, t }: {
  bookId: string;
  nav: Nav;
  theme: Theme;
  t: TFunction;
}) {
  void theme; // 样式走语义色 token，无需调色板
  const { data, loading, error } = useApi<BookData>(`/books/${bookId}`);

  const chapters = useMemo(() => {
    const list = [...(data?.chapters ?? [])];
    list.sort((a, b) => a.number - b.number);
    return list;
  }, [data]);

  // 单线兜底（C4-b 起：从 story/timeline.json 读多线）。
  const plotlines = useMemo(
    () => [{ key: "main", label: t("timeline.mainPlotline"), chapters }],
    [chapters, t],
  );

  const totalWords = chapters.reduce((sum, ch) => sum + (ch.wordCount || 0), 0);

  return (
    <div className="max-w-6xl mx-auto px-6 py-10 md:px-12 fade-in" data-page="book-timeline">
      <div className="flex items-center gap-3 mb-6">
        <button
          onClick={() => nav.toBook(bookId)}
          className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
        >
          <ArrowLeft size={16} />
          <span>{data?.book.title ?? bookId}</span>
        </button>
      </div>

      <h1 className="text-2xl font-bold mb-1">{t("timeline.title")}</h1>
      {!loading && !error && (
        <p className="text-sm text-muted-foreground mb-6">
          {t("timeline.stats").replace("{chapters}", String(chapters.length)).replace("{words}", totalWords.toLocaleString())}
        </p>
      )}

      {loading && <div className="text-sm text-muted-foreground" data-loading="skeleton">{t("common.loading")}</div>}
      {error && <div className="text-sm text-red-500">{String(error)}</div>}

      {!loading && !error && chapters.length === 0 && (
        <div className="text-sm text-muted-foreground border border-border/50 rounded-xl px-5 py-8 text-center">
          {t("timeline.empty")}
        </div>
      )}

      {!loading && !error && chapters.length > 0 && (
        <div className="overflow-x-auto border border-border/50 rounded-xl">
          <div className="min-w-max">
            {/* 列头：章号 */}
            <div className="flex border-b border-border/40 bg-secondary/30 sticky top-0">
              <div className="w-36 shrink-0 px-4 py-2 text-xs font-semibold text-muted-foreground border-r border-border/40">
                {t("timeline.plotline")}
              </div>
              {chapters.map((ch) => (
                <div key={ch.number} className="w-36 shrink-0 px-3 py-2 text-xs font-semibold text-muted-foreground border-r border-border/30 last:border-r-0 text-center">
                  {t("timeline.chapter").replace("{n}", String(ch.number))}
                </div>
              ))}
            </div>
            {/* 情节线行 */}
            {plotlines.map((line) => (
              <div key={line.key} className="flex">
                <div className="w-36 shrink-0 px-4 py-3 text-sm font-medium border-r border-border/40 flex items-center">
                  {line.label}
                </div>
                {line.chapters.map((ch) => {
                  const tone = statusTone(ch.status);
                  return (
                    <button
                      key={ch.number}
                      onClick={() => nav.toChapter(bookId, ch.number)}
                      title={`${ch.title} · ${ch.status} · ${ch.wordCount}`}
                      className={`w-36 shrink-0 px-3 py-3 border-r border-border/30 last:border-r-0 border-b-0 text-left hover:scale-[1.03] transition-transform ${TONE_CLASS[tone]}`}
                      data-timeline-cell={ch.number}
                      data-tone={tone}
                    >
                      <div className="text-xs font-semibold truncate">{ch.title}</div>
                      <div className="text-[11px] opacity-70 mt-0.5">{ch.wordCount.toLocaleString()}</div>
                    </button>
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
