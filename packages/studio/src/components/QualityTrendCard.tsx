import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface TrendPoint {
  readonly chapter: number;
  readonly overallScore?: number;
  readonly passed: boolean;
  readonly issueCount: number;
}

interface QualityTrend {
  readonly points: ReadonlyArray<TrendPoint>;
  readonly scoredChapters: number;
  readonly averageScore?: number;
  readonly failingChapters: ReadonlyArray<number>;
}

interface PromiseUrgency {
  readonly hookId: string;
  readonly urgency: number;
  readonly level: "high" | "medium" | "low";
  readonly confidence: "explicit" | "inferred" | "unknown";
  readonly targetChapter?: number;
  readonly targetWindow?: { readonly start: number; readonly end: number };
}

interface Payload {
  readonly trend: QualityTrend;
  readonly urgency: ReadonlyArray<PromiseUrgency>;
  readonly currentChapter: number;
}

const W = 560;
const H = 120;
const PAD_X = 24;
const PAD_Y = 12;

/**
 * R3/361 号：质量趋势卡（书籍详情挂载）。
 * GET quality-trend → 审查分数折线（0–100，失败章红点）+ 承诺紧迫度
 * 汇总（resolvePromiseUrgency，high/medium/low 三档着色）。
 */
export function QualityTrendCard({ bookId }: { bookId: string }) {
  const [data, setData] = useState<Payload | null>(null);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    setBusy(true);
    try {
      const payload = await fetchJson<Payload>(`/books/${encodeURIComponent(bookId)}/quality-trend`);
      setData(payload);
    } catch {
      setData(null);
    }
    setBusy(false);
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bookId]);

  const points = data?.trend.points ?? [];
  const scored = points.filter((point) => typeof point.overallScore === "number");
  const min = scored.length > 0 ? scored[0]!.chapter : 1;
  const max = points.length > 0 ? points[points.length - 1]!.chapter : min;
  const span = Math.max(1, max - min);
  const x = (chapter: number) => PAD_X + ((chapter - min) / span) * (W - 2 * PAD_X);
  const y = (score: number) => H - PAD_Y - ((score - 60) / 40) * (H - 2 * PAD_Y);
  const linePath = scored
    .map((point, i) => `${i === 0 ? "M" : "L"}${x(point.chapter).toFixed(1)},${y(point.overallScore as number).toFixed(1)}`)
    .join(" ");
  const levelClass = (level: PromiseUrgency["level"]) =>
    level === "high"
      ? "border-rose-500/40 bg-rose-500/10 text-rose-700 dark:text-rose-400"
      : level === "medium"
        ? "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-400"
        : "border-border/60 bg-muted/30 text-muted-foreground";

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <div className="flex items-center justify-between">
        <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
          {tr("质量趋势", "Quality trend")}
        </h3>
        <button
          onClick={load}
          disabled={busy}
          className="px-2 py-1 text-[11px] rounded-md border border-border text-muted-foreground disabled:opacity-30"
        >
          {busy ? tr("加载中…", "Loading…") : tr("刷新", "Refresh")}
        </button>
      </div>
      {!data || points.length === 0 ? (
        <div className="text-xs text-muted-foreground italic">
          {tr(
            "暂无审查记录——跑完一章写作后这里会出现分数趋势。",
            "No review records yet — finish a writing run and score trends appear here.",
          )}
        </div>
      ) : (
        <div className="space-y-2">
          {scored.length > 0 ? (
            <svg viewBox={`0 0 ${W} ${H}`} className="w-full">
              {[60, 80, 100].map((level) => (
                <line
                  key={level}
                  x1={PAD_X}
                  x2={W - PAD_X}
                  y1={y(level)}
                  y2={y(level)}
                  className="stroke-border/60"
                  strokeDasharray={level === 80 ? "4 3" : undefined}
                />
              ))}
              <path d={linePath} fill="none" className="stroke-emerald-600" strokeWidth="1.8" />
              {points
                .filter((point) => !point.passed)
                .map((point) => (
                  <circle
                    key={point.chapter}
                    cx={x(point.chapter)}
                    cy={typeof point.overallScore === "number" ? y(point.overallScore) : H - PAD_Y}
                    r="3.5"
                    className="fill-rose-500"
                  />
                ))}
            </svg>
          ) : (
            <div className="text-xs text-muted-foreground italic">
              {tr("审查器未评分，仅记录通过状态。", "Auditor gave no scores; pass state only.")}
            </div>
          )}
          <div className="flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
            {typeof data.trend.averageScore === "number" && (
              <span>
                {tr("均分", "Avg")}: <span className="font-bold text-foreground">{data.trend.averageScore}</span>
              </span>
            )}
            <span>
              {tr("已记录章", "Chapters")}: <span className="font-bold text-foreground">{points.length}</span>
            </span>
            {data.trend.failingChapters.length > 0 && (
              <span className="text-rose-600 font-bold">
                {tr("未过章", "Failing")}: {data.trend.failingChapters.join(", ")}
              </span>
            )}
          </div>
          {data.urgency.length > 0 && (
            <div className="space-y-1">
              <div className="text-[11px] font-bold uppercase tracking-wider text-muted-foreground">
                {tr("承诺紧迫度（下一章 第" , "Promise urgency (next ch. ")}{data.currentChapter}{tr(" 章)", ")")}
              </div>
              <ul className="space-y-1">
                {data.urgency.slice(0, 6).map((entry) => (
                  <li key={entry.hookId} className={`flex items-center gap-2 text-xs rounded-md border px-2 py-1.5 ${levelClass(entry.level)}`}>
                    <span className="font-mono font-bold">{entry.hookId}</span>
                    <span className="font-bold">{entry.urgency}</span>
                    {typeof entry.targetWindow === "object" && (
                      <span className="text-muted-foreground">
                        {tr(`目标 ${entry.targetWindow.start}–${entry.targetWindow.end}`, `target ${entry.targetWindow.start}–${entry.targetWindow.end}`)}
                      </span>
                    )}
                    <span className="text-muted-foreground/70">
                      {entry.confidence === "explicit" ? tr("显式", "explicit") : entry.confidence === "inferred" ? tr("推断", "inferred") : tr("无目标", "no target")}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
