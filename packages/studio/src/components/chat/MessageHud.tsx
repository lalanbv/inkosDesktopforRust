import { ArrowDown, ArrowUp, Timer, Zap } from "lucide-react";
import { tr } from "../../lib/app-language";
import type { MessageTimings, MessageUsage } from "../../store/chat/types";

/**
 * G8a/333 号聊天 AI 实况：消息级 token（输入/输出/合计）与首包/总耗时。
 * 紧凑单行 HUD（PlayHud 实况模式的聊天面轻量版），挂在助手消息尾部；
 * 双字段都缺省时整行不渲染。
 */
export function MessageHud({ usage, timings }: { usage?: MessageUsage; timings?: MessageTimings }) {
  if (!usage && !timings) return null;
  const hasUsage = Boolean(usage);
  const firstTokenSec = timings && timings.firstTokenMs > 0
    ? (timings.firstTokenMs / 1000).toFixed(1)
    : null;
  const totalSec = timings && timings.totalMs > 0
    ? (timings.totalMs / 1000).toFixed(1)
    : null;
  if (!hasUsage && !firstTokenSec && !totalSec) return null;

  return (
    <div
      className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-muted-foreground/80"
      data-testid="message-hud"
    >
      {usage ? (
        <span className="inline-flex items-center gap-1" title={tr("输入 / 输出 / 合计 tokens", "input / output / total tokens")}>
          <ArrowUp className="size-3" aria-hidden />
          {usage.input}
          <ArrowDown className="size-3 ml-1" aria-hidden />
          {usage.output}
          <span className="ml-1">{tr("合计", "total")} {usage.totalTokens}</span>
        </span>
      ) : null}
      {firstTokenSec ? (
        <span className="inline-flex items-center gap-1">
          <Zap className="size-3" aria-hidden />
          {tr("首包", "first token")} {firstTokenSec}s
        </span>
      ) : null}
      {totalSec ? (
        <span className="inline-flex items-center gap-1">
          <Timer className="size-3" aria-hidden />
          {tr("总耗时", "total time")} {totalSec}s
        </span>
      ) : null}
    </div>
  );
}
