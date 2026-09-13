import { useState } from "react";
import { HOOK_KIND_IDS, hookKindLabel, type HookKind } from "@actalk/inkos-core";
import { cn } from "../../lib/utils";
import { tr } from "../../lib/app-language";
import { parsePendingHooks } from "../../lib/truth-display";

interface PendingHooksViewProps {
  readonly content: string;
}

const HOOK_TYPE_COLOR: Record<string, string> = {
  "主线伏笔": "bg-amber-500/15 text-amber-600 dark:text-amber-400",
  "角色前置": "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400",
  "情感线伏笔": "bg-pink-500/15 text-pink-600 dark:text-pink-400",
  "次要伏笔": "bg-blue-500/15 text-blue-600 dark:text-blue-400",
};

// R23/396 号：7 类规范分类徽标配色（与旧自由文本 type 徽标并排，互不替代）。
const HOOK_KIND_COLOR: Record<HookKind, string> = {
  promise: "bg-amber-500/15 text-amber-600 dark:text-amber-400",
  suspense: "bg-violet-500/15 text-violet-600 dark:text-violet-400",
  crisis: "bg-red-500/15 text-red-600 dark:text-red-400",
  artifact: "bg-lime-500/15 text-lime-600 dark:text-lime-400",
  information: "bg-sky-500/15 text-sky-600 dark:text-sky-400",
  emotion: "bg-pink-500/15 text-pink-600 dark:text-pink-400",
  worldview: "bg-teal-500/15 text-teal-600 dark:text-teal-400",
};

function hookTypeColor(type: string): string {
  return HOOK_TYPE_COLOR[type] ?? "bg-zinc-500/15 text-zinc-600 dark:text-zinc-400";
}

// Renders pending_hooks.md (a 14-column tracking table) as browsable cards:
// the actual foreshadow text up front, with kind / type / core / payoff as
// small tags, plus a kind filter (R23/396 号 — 选择器按分类筛选子集).
// Bookkeeping columns (half-life, dependencies, …) are intentionally dropped.
export function PendingHooksView({ content }: PendingHooksViewProps) {
  const hooks = parsePendingHooks(content);
  const [kindFilter, setKindFilter] = useState<HookKind | "all">("all");

  // 台账里实际出现的分类（保持词表序）——空类不占筛选位。
  const presentKinds = HOOK_KIND_IDS.filter((kind) =>
    hooks.some((hook) => hook.kind === kind),
  );
  const visibleHooks = kindFilter === "all"
    ? hooks
    : hooks.filter((hook) => hook.kind === kindFilter);

  if (hooks.length === 0) {
    return (
      <p className="text-[14px] leading-6 text-muted-foreground/60 italic">
        {tr("还没有埋下伏笔。", "No foreshadowing planted yet.")}
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-2">
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
      {visibleHooks.map((hook) => (
        <div key={hook.id} className="rounded-lg bg-secondary/30 px-3 py-2.5">
          <div className="flex items-center gap-1.5 mb-1.5 flex-wrap">
            {hook.promoted === false && (
              <span className="text-[12px] px-1.5 py-0.5 rounded-full bg-zinc-500/10 text-muted-foreground">
                {tr("种子", "Seed")}
              </span>
            )}
            {hook.promoted === true && (
              <span className="text-[12px] px-1.5 py-0.5 rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400">
                {tr("活跃", "Active")}
              </span>
            )}
            {hook.kind && (
              <span className={cn("text-[12px] px-1.5 py-0.5 rounded-full", HOOK_KIND_COLOR[hook.kind])}>
                {tr(hookKindLabel(hook.kind, "zh"), hookKindLabel(hook.kind, "en"))}
              </span>
            )}
            {hook.type && (
              <span className={cn("text-[12px] px-1.5 py-0.5 rounded-full", hookTypeColor(hook.type))}>
                {hook.type}
              </span>
            )}
            {hook.core && (
              <span className="text-[12px] px-1.5 py-0.5 rounded-full bg-amber-500/15 text-amber-600 dark:text-amber-400">
                {tr("核心", "Core")}
              </span>
            )}
            {hook.payoff && (
              <span className="text-[12px] text-muted-foreground/50 ml-auto">{tr("回收", "Payoff")} · {hook.payoff}</span>
            )}
          </div>
          <p className="text-[15px] text-foreground leading-7 font-['SimSun','Songti_SC','STSong',serif]">
            {hook.content}
          </p>
        </div>
      ))}
    </div>
  );
}
