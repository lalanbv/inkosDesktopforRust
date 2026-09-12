import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface TensionPoint {
  readonly chapter: number;
  readonly conflictLevel: number;
  readonly revealLevel: number;
}

interface TensionCurvePayload {
  readonly points: ReadonlyArray<TensionPoint>;
  readonly scoredChapters: number;
  readonly unscoredChapters: number;
}

interface TensionWarning {
  readonly kind: "flat-middle" | "climax-crowding" | "weak-hook-streak";
  readonly chapters: ReadonlyArray<number>;
  readonly severity: "warning";
  readonly description: string;
  readonly suggestion: string;
}

interface Payload {
  readonly curve: TensionCurvePayload;
  readonly warnings: ReadonlyArray<TensionWarning>;
}

const W = 560;
const H = 120;
const PAD_X = 24;
const PAD_Y = 12;

function toPath(points: ReadonlyArray<TensionPoint>, level: (p: TensionPoint) => number): string {
  if (points.length === 0) return "";
  const min = points[0]!.chapter;
  const max = Math.max(min, points[points.length - 1]!.chapter);
  const span = Math.max(1, max - min);
  const x = (chapter: number) => PAD_X + ((chapter - min) / span) * (W - 2 * PAD_X);
  const y = (value: number) => H - PAD_Y - ((value - 1) / 9) * (H - 2 * PAD_Y);
  return points.map((p, i) => `${i === 0 ? "M" : "L"}${x(p.chapter).toFixed(1)},${y(level(p)).toFixed(1)}`).join(" ");
}

/**
 * R2/359 号：张力曲线卡（书籍详情挂载）。
 * GET tension-curve → 冲突/揭示双序列 SVG 折线 + 启发式告警行。
 * 缺分章自动跳过（曲线是展示层，允许 ±1 抖动）。
 */
export function TensionCurveCard({ bookId }: { bookId: string }) {
  const [data, setData] = useState<Payload | null>(null);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    setBusy(true);
    try {
      const payload = await fetchJson<Payload>(`/books/${encodeURIComponent(bookId)}/tension-curve`);
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

  const points = data?.curve.points ?? [];

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <div className="flex items-center justify-between">
        <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
          {tr("张力曲线", "Tension curve")}
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
            "暂无张力评分——新结算的章节会带 1–10 的冲突/揭示分。",
            "No tension scores yet — newly settled chapters carry 1–10 conflict/reveal levels.",
          )}
        </div>
      ) : (
        <div className="space-y-2">
          <svg viewBox={`0 0 ${W} ${H}`} className="w-full">
            {[1, 5, 10].map((level) => (
              <line
                key={level}
                x1={PAD_X}
                x2={W - PAD_X}
                y1={H - PAD_Y - ((level - 1) / 9) * (H - 2 * PAD_Y)}
                y2={H - PAD_Y - ((level - 1) / 9) * (H - 2 * PAD_Y)}
                className="stroke-border/60"
                strokeDasharray={level === 5 ? "4 3" : undefined}
              />
            ))}
            <path d={toPath(points, (p) => p.conflictLevel)} fill="none" className="stroke-rose-500" strokeWidth="1.8" />
            <path d={toPath(points, (p) => p.revealLevel)} fill="none" className="stroke-sky-500" strokeWidth="1.8" strokeDasharray="5 3" />
          </svg>
          <div className="flex items-center gap-4 text-[11px] text-muted-foreground">
            <span className="flex items-center gap-1">
              <span className="inline-block w-4 h-0.5 bg-rose-500" />
              {tr("冲突强度", "Conflict")}
            </span>
            <span className="flex items-center gap-1">
              <span className="inline-block w-4 h-0.5 bg-sky-500" />
              {tr("揭示强度", "Reveal")}
            </span>
            <span>
              {tr(
                `第 ${points[0]!.chapter}–${points[points.length - 1]!.chapter} 章（${data.curve.unscoredChapters} 章未评分）`,
                `ch. ${points[0]!.chapter}–${points[points.length - 1]!.chapter} (${data.curve.unscoredChapters} unscored)`,
              )}
            </span>
          </div>
          {data.warnings.length > 0 && (
            <ul className="space-y-1">
              {data.warnings.map((warning) => (
                <li key={warning.kind} className="text-xs rounded-md border border-amber-500/40 bg-amber-500/10 px-2 py-1.5">
                  <span className="font-bold text-amber-700 dark:text-amber-400">{warning.description}</span>
                  <span className="text-muted-foreground"> — {warning.suggestion}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
