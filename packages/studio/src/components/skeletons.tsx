import { useEffect, useState } from "react";
import { cn } from "@/lib/utils";
import { tr } from "@/lib/app-language";
import { Skeleton } from "@/components/ui/skeleton";

/** 骨架延迟出现的默认等待（毫秒）：快于该值的请求不闪骨架，慢于它才显示加载占位。 */
export const SKELETON_DELAY_MS = 200;

// 宽度错落的候选序列。Tailwind 按源码字面量收集类名，必须整串静态写在这里，
// 不能在渲染时动态拼接。
const ROW_TITLE_WIDTHS = ["w-3/5", "w-1/2", "w-2/3", "w-2/5"] as const;
const PARAGRAPH_TAIL_WIDTHS = ["w-4/5", "w-11/12", "w-full", "w-3/4"] as const;

/**
 * 骨架延迟出现逻辑的可测内核：到时回调一次，返回取消函数（幂等）。
 * useDelayedVisible 只是把它接进 React 生命周期。
 */
export function scheduleDelayedReveal(delayMs: number, onReveal: () => void): () => void {
  const timer = setTimeout(onReveal, delayMs);
  let cancelled = false;
  return () => {
    if (cancelled) return;
    cancelled = true;
    clearTimeout(timer);
  };
}

/**
 * 骨架出现前的等待：加载时长低于 delayMs 的请求直接从空白跳内容，
 * 不闪一下骨架。卸载/依赖变化时取消计时器。
 */
export function useDelayedVisible(delayMs: number = SKELETON_DELAY_MS): boolean {
  const [visible, setVisible] = useState(false);
  useEffect(() => scheduleDelayedReveal(delayMs, () => setVisible(true)), [delayMs]);
  return visible;
}

/** 列表行骨架（会话/章节/书籍行）：图标圆 + 两行文本条。 */
export function SkeletonRows({ count = 5, className }: { count?: number; className?: string }) {
  return (
    <div
      role="status"
      aria-busy="true"
      aria-label={tr("加载中", "Loading")}
      data-slot="skeleton-rows"
      className={cn("space-y-2.5", className)}
    >
      {Array.from({ length: count }, (_, index) => (
        <div key={index} className="flex items-center gap-3">
          <Skeleton className="size-8 shrink-0 rounded-full" />
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <Skeleton className={cn("h-3", ROW_TITLE_WIDTHS[index % ROW_TITLE_WIDTHS.length])} />
            <Skeleton className="h-2.5 w-2/5" />
          </div>
        </div>
      ))}
    </div>
  );
}

/** 卡片骨架（书架/仪表的整行大卡）：图标块 + 标题条 + 元信息短条。 */
export function SkeletonCards({ count = 3, className }: { count?: number; className?: string }) {
  return (
    <div
      role="status"
      aria-busy="true"
      aria-label={tr("加载中", "Loading")}
      data-slot="skeleton-cards"
      className={cn("grid gap-6", className)}
    >
      {Array.from({ length: count }, (_, index) => (
        <div key={index} data-slot="skeleton-card" className="rounded-2xl border border-border/40 bg-card/60 p-8">
          <div className="mb-3 flex items-center gap-3">
            <Skeleton className="size-10 shrink-0 rounded-lg" />
            <Skeleton className={cn("h-6", ROW_TITLE_WIDTHS[index % ROW_TITLE_WIDTHS.length])} />
          </div>
          <div className="flex items-center gap-3">
            <Skeleton className="h-4 w-20" />
            <Skeleton className="h-4 w-28" />
            <Skeleton className="h-4 w-16" />
          </div>
        </div>
      ))}
    </div>
  );
}

/** 正文段落骨架：每段三行，末行宽度错落，模拟排版节奏。 */
export function SkeletonParagraphs({ count = 4, className }: { count?: number; className?: string }) {
  return (
    <div
      role="status"
      aria-busy="true"
      aria-label={tr("加载中", "Loading")}
      data-slot="skeleton-paragraphs"
      className={cn("space-y-6", className)}
    >
      {Array.from({ length: count }, (_, index) => (
        <div key={index} className="space-y-3">
          <Skeleton className="h-3.5 w-full" />
          <Skeleton className="h-3.5 w-full" />
          <Skeleton className={cn("h-3.5", PARAGRAPH_TAIL_WIDTHS[index % PARAGRAPH_TAIL_WIDTHS.length])} />
        </div>
      ))}
    </div>
  );
}
