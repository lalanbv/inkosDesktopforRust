import { InkosLogo } from "@/components/InkosLogo";
import { Skeleton } from "@/components/ui/skeleton";
import { SkeletonCards, SkeletonRows } from "@/components/skeletons";

/**
 * startupGate=loading 时的整壳骨架：顶栏 + 侧栏 + 主区三段与就绪后的
 * App 布局同构，内容到达时逐区填充而不是整屏 spinner 换整屏内容。
 */
export function AppShellSkeleton() {
  return (
    <div
      data-loading="shell"
      className="h-screen bg-background text-foreground flex overflow-hidden font-sans"
      aria-busy="true"
    >
      {/* 侧栏：Logo 呼吸 + 导航行占位 */}
      <aside data-slot="skeleton-sidebar" className="w-[260px] shrink-0 border-r border-border bg-background/80 flex flex-col">
        <div className="px-6 py-8">
          <div className="flex items-center gap-3">
            {/* 复用 chat-icon-glow 呼吸动画，与聊天区图标同一动效语言 */}
            <InkosLogo className="chat-icon-glow w-11 h-11 shrink-0" />
            <div className="flex flex-col gap-2">
              <Skeleton className="h-5 w-20" />
              <Skeleton className="h-2.5 w-12" />
            </div>
          </div>
        </div>
        <div className="flex-1 overflow-hidden px-4 py-2">
          <SkeletonRows count={6} />
        </div>
      </aside>

      {/* 主列：顶栏 + 内容区 */}
      <div className="flex-1 flex flex-col min-w-0">
        <header data-slot="skeleton-header" className="h-14 shrink-0 flex items-center justify-between px-8 border-b border-border/40">
          <Skeleton className="h-8 w-40 rounded-lg" />
          <div className="flex items-center gap-3">
            <Skeleton className="h-7 w-16 rounded-lg" />
            <Skeleton className="size-7 rounded-lg" />
          </div>
        </header>
        <main data-slot="skeleton-main" className="flex-1 px-6 py-12 md:px-12 lg:py-16">
          <SkeletonCards count={3} />
        </main>
      </div>
    </div>
  );
}
