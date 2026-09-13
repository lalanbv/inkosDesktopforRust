import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface RunLogEntryView {
  readonly ts: string;
  readonly agent: string;
  readonly model: string;
  readonly durationMs: number;
  readonly ok: boolean;
  readonly attemptIndex: number;
  readonly round: number;
  readonly tookOver: boolean;
  readonly errorKind: string | null;
}

interface RunLogPayload {
  readonly total: number;
  readonly kept: number;
  readonly tookOverCount: number;
  readonly failureCount: number;
  readonly entries: ReadonlyArray<RunLogEntryView>;
}

/**
 * R26/403 号：调用级运行遥测面板（Dashboard 挂载）。
 * GET /run-log?limit=50 —— LLM 调用元数据环形缓冲（默认 200 条）纯读投影：
 * agent / model / 耗时 / 成败 / 接管（R25 联动）。只记元数据，无 prompt/正文。
 * 空缓冲整卡不渲染。
 */
export function RunLogPanel() {
  const [payload, setPayload] = useState<RunLogPayload | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchJson<RunLogPayload>("/run-log?limit=50")
      .then((data) => {
        if (!cancelled) setPayload(data);
      })
      .catch(() => {
        if (!cancelled) setPayload(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!payload || payload.entries.length === 0) return null;

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2" data-slot="run-log-panel">
      <div className="flex flex-wrap items-center gap-3">
        <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
          {tr("运行遥测", "Run log")}
        </h3>
        <span className="text-[11px] text-muted-foreground">
          {tr("累计", "Total")} <span className="font-bold text-foreground">{payload.total}</span>
          {" · "}
          {tr("失败", "Failures")} <span className={`font-bold ${payload.failureCount > 0 ? "text-red-600" : "text-foreground"}`}>{payload.failureCount}</span>
          {" · "}
          {tr("接管", "Takeovers")} <span className={`font-bold ${payload.tookOverCount > 0 ? "text-amber-600" : "text-foreground"}`}>{payload.tookOverCount}</span>
        </span>
      </div>
      <ul className="space-y-1">
        {[...payload.entries].reverse().map((entry, index) => (
          <li key={`${entry.ts}-${index}`} className="flex flex-wrap items-center gap-2 text-xs">
            <span className="text-muted-foreground/70">{entry.ts.slice(11, 19)}</span>
            <span className="font-medium text-foreground">{entry.agent}</span>
            <span className="text-muted-foreground">{entry.model}</span>
            <span className="text-muted-foreground/70">
              {entry.durationMs >= 1000 ? `${(entry.durationMs / 1000).toFixed(1)}s` : `${entry.durationMs}ms`}
            </span>
            <span
              className={`px-1.5 py-0.5 rounded-full font-bold ${
                entry.ok
                  ? "bg-emerald-500/10 text-emerald-600"
                  : entry.errorKind === "fatal"
                    ? "bg-red-500/10 text-red-600"
                    : "bg-amber-500/10 text-amber-600"
              }`}
            >
              {entry.ok ? tr("成功", "ok") : entry.errorKind === "fatal" ? tr("失败", "failed") : tr("瞬态", "retry")}
            </span>
            {entry.tookOver && (
              <span className="px-1.5 py-0.5 rounded-full bg-sky-500/10 text-sky-600">
                {tr("接管", "takeover")} · #{entry.attemptIndex}/R{entry.round}
              </span>
            )}
          </li>
        ))}
      </ul>
      {payload.total > payload.entries.length && (
        <p className="text-[11px] text-muted-foreground/60 italic">
          {tr("仅显示最近记录；更早的已被环形缓冲逐出。", "Showing the most recent entries; older ones were evicted.")}
        </p>
      )}
    </div>
  );
}
