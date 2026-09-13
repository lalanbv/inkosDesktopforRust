import { useEffect, useMemo, useState } from "react";
import { fetchJson, useApi } from "../hooks/use-api";
import { tr } from "../lib/app-language";
import { aggregateWritingStats, type ChapterStatRow } from "../lib/writing-stats";

/**
 * R13/382 号：写作数据面板（Dashboard 挂载）。
 * GET /writing-stats（TS 聚合）不可达时回退 Rust 端点 /writing-stats-rows
 * 同一纯函数聚合——产出节奏/通过率/token 成本一屏可读。
 */
export function WritingStatsCard() {
  const [stats, setStats] = useState<ReturnType<typeof aggregateWritingStats> | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const data = await fetchJson<unknown>("/writing-stats");
        // TS 端点直接返回聚合结果。
        setStats(data as ReturnType<typeof aggregateWritingStats>);
      } catch {
        // R13/411 号：Rust 引擎无聚合端点——回退 rows 行面，前端同一纯函数聚合。
        try {
          const payload = await fetchJson<{ rows: ReadonlyArray<ChapterStatRow> }>("/writing-stats-rows");
          setStats(aggregateWritingStats(payload.rows ?? [], new Date().toISOString()));
        } catch {
          setStats(null);
        }
      }
    })();
  }, []);

  const maxWords = useMemo(
    () => Math.max(1, ...(stats?.daily ?? []).map((bucket) => bucket.words)),
    [stats],
  );

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("写作数据", "Writing stats")}
      </h3>
      {!stats || stats.totalChapters === 0 ? (
        <p className="text-xs text-muted-foreground italic">
          {tr("还没有章节数据——写完第一章后这里会出现产出节奏。", "No chapters yet — your output rhythm appears after the first chapter.")}
        </p>
      ) : (
        <div className="space-y-2">
          <div className="flex items-end gap-1 h-16" data-slot="writing-stats-bars">
            {stats.daily.map((bucket) => (
              <div
                key={bucket.date}
                title={`${bucket.date}: ${bucket.words} 字 / ${bucket.chapters} 章`}
                className="flex-1 bg-primary/70 rounded-t"
                style={{ height: `${Math.max(6, Math.round((bucket.words / maxWords) * 60))}px` }}
              />
            ))}
          </div>
          <div className="flex flex-wrap items-center gap-3 text-[11px] text-muted-foreground">
            <span>
              {tr("近 30 天", "Last 30d")}:{" "}
              <span className="font-bold text-foreground">
                {stats.words30d.toLocaleString()} {tr("字", "words")}
              </span>
            </span>
            <span>
              {tr("完成章", "Chapters")}:{" "}
              <span className="font-bold text-foreground">{stats.chapters30d}</span>
            </span>
            {typeof stats.passRate === "number" && (
              <span>
                {tr("审查通过率", "Pass rate")}:{" "}
                <span className="font-bold text-foreground">{stats.passRate}%</span>
              </span>
            )}
            {stats.totalTokens > 0 && (
              <span>
                {tr("Token", "Tokens")}:{" "}
                <span className="font-bold text-foreground">{stats.totalTokens.toLocaleString()}</span>
              </span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

// R13：聚合纯函数（writing-stats.ts）在端点与本地回退共用；此处保留类型引用。
export type { ChapterStatRow };
