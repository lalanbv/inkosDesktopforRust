import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * 空态占位：居中大号弱色图标 + 标题 + 描述 + 可选引导动作。
 * 侧栏等紧凑区域用 `compact`；空态只在数据就绪后出现（与骨架互斥）。
 */
export function EmptyState({ icon, title, description, actionLabel, onAction, compact }: {
  icon: ReactNode;
  title: string;
  description?: string;
  actionLabel?: string;
  onAction?: () => void;
  compact?: boolean;
}) {
  return (
    <div
      data-slot="empty-state"
      className={cn(
        "flex flex-col items-center justify-center text-center fade-in",
        compact ? "px-3 py-6" : "py-10",
      )}
    >
      <div className={cn(
        "flex items-center justify-center rounded-full bg-primary/5 text-primary/30",
        compact ? "size-10 mb-3" : "size-16 mb-4",
      )}>
        {icon}
      </div>
      <div className={cn("font-medium text-foreground/80", compact ? "text-sm" : "text-base")}>
        {title}
      </div>
      {description && (
        <p className="mt-1 max-w-[240px] text-xs leading-relaxed text-muted-foreground">
          {description}
        </p>
      )}
      {actionLabel && onAction && (
        <button
          type="button"
          data-slot="empty-state-action"
          onClick={onAction}
          className={cn(
            "mt-4 inline-flex items-center gap-1.5 rounded-lg bg-primary font-medium text-primary-foreground transition-all hover:scale-[1.03] active:scale-95",
            compact ? "px-3 py-1.5 text-xs" : "px-4 py-2 text-sm",
          )}
        >
          {actionLabel}
        </button>
      )}
    </div>
  );
}
