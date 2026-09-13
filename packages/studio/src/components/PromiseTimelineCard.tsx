import { useEffect, useState } from "react";
import { HOOK_KIND_IDS, hookKindLabel, type HookKind } from "@actalk/inkos-core/utils/hook-kind";
import { cn } from "../lib/utils";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface PromiseTimelineEntry {
  readonly hookId: string;
  readonly summary: string;
  readonly openedAt: number;
  readonly lastAdvancedAt?: number;
  readonly expectedPayoff: string;
  readonly state: "open" | "advancing" | "fulfilled" | "overdue";
  readonly kind?: HookKind; // R23/396 号：规范分类透传（存量无 kind 缺省）
}

interface PacingDebt {
  readonly hookId: string;
  readonly stalledChapters: number;
  readonly expectedPayoff: string;
}

interface WeakHookRun {
  readonly fromChapter: number;
  readonly toChapter: number;
  readonly length: number;
}

interface PromisesPayload {
  readonly timeline: ReadonlyArray<PromiseTimelineEntry>;
  readonly pacingDebts: ReadonlyArray<PacingDebt>;
  readonly weakRuns: { runs: ReadonlyArray<WeakHookRun>; longestRun: number };
  readonly currentChapter: number;
}

const STATE_STYLE: Record<PromiseTimelineEntry["state"], string> = {
  overdue: "bg-red-500/10 text-red-600",
  open: "bg-muted text-muted-foreground",
  advancing: "bg-sky-500/10 text-sky-600",
  fulfilled: "bg-emerald-500/10 text-emerald-600",
};

const STATE_LABEL: Record<PromiseTimelineEntry["state"], { zh: string; en: string }> = {
  overdue: { zh: "已逾期", en: "Overdue" },
  open: { zh: "已开启", en: "Opened" },
  advancing: { zh: "推进中", en: "Advancing" },
  fulfilled: { zh: "已兑付", en: "Fulfilled" },
};

// R23/396 号：7 类规范分类徽标配色（与 PendingHooksView 同映射）。
const HOOK_KIND_COLOR: Record<HookKind, string> = {
  promise: "bg-amber-500/15 text-amber-600 dark:text-amber-400",
  suspense: "bg-violet-500/15 text-violet-600 dark:text-violet-400",
  crisis: "bg-red-500/15 text-red-600 dark:text-red-400",
  artifact: "bg-lime-500/15 text-lime-600 dark:text-lime-400",
  information: "bg-sky-500/15 text-sky-600 dark:text-sky-400",
  emotion: "bg-pink-500/15 text-pink-600 dark:text-pink-400",
  worldview: "bg-teal-500/15 text-teal-600 dark:text-teal-400",
};

/**
 * G10/338 号：承诺账本运营视图（书籍详情挂载）。
 * 节奏债告警（core hook 搁置超 N 章）+ 连续弱钩段 + 承诺时间线
 * （开启/推进/兑付状态投影）。全空时整卡不渲染。
 */
export function PromiseTimelineCard({ bookId }: { bookId: string }) {
  const [payload, setPayload] = useState<PromisesPayload | null>(null);
  // R23/396 号：timeline 按类过滤（"all" = 不筛）。
  const [kindFilter, setKindFilter] = useState<HookKind | "all">("all");

  useEffect(() => {
    let cancelled = false;
    fetchJson<PromisesPayload>(`/books/${encodeURIComponent(bookId)}/promises`)
      .then((r) => {
        if (!cancelled) setPayload(r);
      })
      .catch(() => {
        if (!cancelled) setPayload(null);
      });
    return () => {
      cancelled = true;
    };
  }, [bookId]);

  if (!payload) return null;
  const { timeline, pacingDebts, weakRuns } = payload;
  if (timeline.length === 0 && pacingDebts.length === 0) return null;

  // 台账里实际出现的分类（保持词表序）——空类不占筛选位。
  const presentKinds = HOOK_KIND_IDS.filter((kind) =>
    timeline.some((entry) => entry.kind === kind),
  );
  const visibleTimeline = kindFilter === "all"
    ? timeline
    : timeline.filter((entry) => entry.kind === kindFilter);

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-3">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("承诺账本", "Promise ledger")}
      </h3>

      {presentKinds.length > 1 && (
        <div className="flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            onClick={() => setKindFilter("all")}
            className={cn(
              "text-[12px] px-2 py-0.5 rounded-full transition-colors",
              kindFilter === "all"
                ? "bg-foreground text-background"
                : "bg-secondary/60 text-muted-foreground hover:bg-secondary",
            )}
          >
            {tr("全部", "All")}
          </button>
          {presentKinds.map((kind) => (
            <button
              key={kind}
              type="button"
              onClick={() => setKindFilter(kindFilter === kind ? "all" : kind)}
              className={cn(
                "text-[12px] px-2 py-0.5 rounded-full transition-colors",
                kindFilter === kind
                  ? HOOK_KIND_COLOR[kind]
                  : "bg-secondary/60 text-muted-foreground hover:bg-secondary",
              )}
            >
              {tr(hookKindLabel(kind, "zh"), hookKindLabel(kind, "en"))}
            </button>
          ))}
        </div>
      )}

      {pacingDebts.length > 0 && (
        <div className="space-y-1">
          {pacingDebts.map((debt) => (
            <div key={debt.hookId} className="text-xs text-red-600 flex flex-wrap items-center gap-1.5">
              <span className="font-bold">{tr("节奏债", "Pacing debt")}</span>
              <span>{debt.hookId}</span>
              <span className="text-muted-foreground">
                {tr("核心承诺已搁置", "core promise stalled")} {debt.stalledChapters} {tr("章", "ch.")}
                {debt.expectedPayoff ? ` · ${tr("期望", "expect")} ${debt.expectedPayoff}` : ""}
              </span>
            </div>
          ))}
        </div>
      )}

      {weakRuns.longestRun > 0 && (
        <div className="text-xs text-amber-600 flex flex-wrap items-center gap-1.5">
          <span className="font-bold">{tr("连续弱钩", "Weak-hook run")}</span>
          {weakRuns.runs.map((run) => (
            <span key={`${run.fromChapter}-${run.toChapter}`} className="bg-amber-500/10 px-2 py-0.5 rounded-full">
              {tr("第", "Ch.")} {run.fromChapter}–{run.toChapter} {tr("章", "")} ({run.length})
            </span>
          ))}
        </div>
      )}

      <ul className="space-y-1.5">
        {visibleTimeline.map((entry) => (
          <li key={entry.hookId} className="flex flex-wrap items-center gap-2 text-xs">
            <span className={`px-2 py-0.5 rounded-full font-bold ${STATE_STYLE[entry.state]}`}>
              {tr(STATE_LABEL[entry.state].zh, STATE_LABEL[entry.state].en)}
            </span>
            <span className="font-medium text-foreground">{entry.hookId}</span>
            {entry.kind && (
              <span className={cn("px-1.5 py-0.5 rounded-full", HOOK_KIND_COLOR[entry.kind])}>
                {tr(hookKindLabel(entry.kind, "zh"), hookKindLabel(entry.kind, "en"))}
              </span>
            )}
            {entry.summary && <span className="text-muted-foreground line-clamp-1">{entry.summary}</span>}
            <span className="text-muted-foreground/70">
              {tr("开启", "opened")} {entry.openedAt || "—"}
              {entry.lastAdvancedAt !== undefined && ` · ${tr("推进", "advanced")} ${entry.lastAdvancedAt}`}
              {entry.expectedPayoff && ` · ${tr("期望", "expect")} ${entry.expectedPayoff}`}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
