import type { ReactNode } from "react";
import { X } from "lucide-react";
import { tr } from "@/lib/app-language";
import { usePanelWidth } from "@/hooks/use-panel-width";
import { useChatStore } from "@/store/chat";
import type { Theme } from "@/hooks/use-theme";
import type { TFunction } from "@/hooks/use-i18n";
import type { SSEMessage } from "@/hooks/use-sse";
import { ArtifactView } from "@/components/chat/BookSidebar";
import { ProgressSection } from "@/components/sidebar/ProgressSection";
import { ChaptersSection } from "@/components/sidebar/ChaptersSection";
import { CharacterSection } from "@/components/sidebar/CharacterSection";
import { FoundationSection } from "@/components/sidebar/FoundationSection";
import { SummarySection } from "@/components/sidebar/SummarySection";

/**
 * 右侧上下文 dock（P3-4）：BookSidebar 的泛化形态。
 * 面板注册表驱动（进度/大纲/设定/摘要），宽度持久记忆（280~700 独立键，
 * 分隔条贴右缘按视口右余量计算），开合经 App 的 Cmd+Shift+D 分发并按页记忆。
 */

export interface DockRenderContext {
  bookId: string;
  theme: Theme;
  t: TFunction;
  sse: { messages: ReadonlyArray<SSEMessage>; connected: boolean };
}

export interface ContextDockPanel {
  id: string;
  titleZh: string;
  titleEn: string;
  render: (context: DockRenderContext, isZh: boolean) => ReactNode;
}

/** 面板注册表（P3-4）：顺序即堆叠顺序；进度 → 大纲(章节) → 设定 → 摘要。 */
export const CONTEXT_DOCK_PANELS: ReadonlyArray<ContextDockPanel> = [
  {
    id: "progress",
    titleZh: "进度",
    titleEn: "Progress",
    render: (ctx) => <ProgressSection sse={ctx.sse} />,
  },
  {
    id: "outline",
    titleZh: "大纲",
    titleEn: "Outline",
    render: (ctx, isZh) => <ChaptersSection bookId={ctx.bookId} isZh={isZh} />,
  },
  {
    id: "settings",
    titleZh: "设定",
    titleEn: "Settings",
    render: (ctx) => (
      <>
        <CharacterSection bookId={ctx.bookId} />
        <FoundationSection bookId={ctx.bookId} />
      </>
    ),
  },
  {
    id: "summary",
    titleZh: "摘要",
    titleEn: "Summary",
    render: (ctx) => <SummarySection bookId={ctx.bookId} />,
  },
];

export const DOCK_WIDTH_STORAGE_KEY = "inkos:studio:dock-width";

function defaultDockWidth(): number {
  if (typeof window === "undefined") return 420;
  return Math.min(700, Math.max(280, Math.round(window.innerWidth * 0.4)));
}

export function ContextDock({ bookId, theme, t, sse, visible, onClose }: {
  bookId: string;
  theme: Theme;
  t: TFunction;
  sse: { messages: ReadonlyArray<SSEMessage>; connected: boolean };
  /** App 持有（Cmd+Shift+D 分发 + 按页记忆）。 */
  visible: boolean;
  onClose: () => void;
}) {
  const sidebarView = useChatStore((s) => s.sidebarView);
  const panel = usePanelWidth({
    min: 280,
    max: 700,
    defaultWidth: defaultDockWidth(),
    storageKey: DOCK_WIDTH_STORAGE_KEY,
    side: "right",
  });
  // 语言判定与 BookSidebar.PanelView 同款（t() 探针）
  const isZh = t("nav.connected") === "已连接";

  if (!visible) return null;

  return (
    <aside
      data-slot="context-dock"
      data-testid="context-dock"
      className="hidden lg:flex shrink-0 flex-col bg-background/30 backdrop-blur-sm overflow-y-auto relative border-l border-border/20"
      style={{ width: panel.width }}
    >
      {/* 拖拽分隔条：贴 dock 左缘，宽度=视口右余量（usePanelWidth side=right） */}
      <div
        data-testid="dock-resizer"
        role="separator"
        aria-orientation="vertical"
        onPointerDown={panel.beginResize}
        onPointerMove={panel.handleResizeMove}
        onPointerUp={panel.endResize}
        onDoubleClick={panel.resetWidth}
        className="absolute left-0 top-0 h-full w-1 cursor-col-resize hover:bg-primary/20 active:bg-primary/30 transition-colors z-10"
      />

      <div className="flex items-center justify-between px-3 py-2 border-b border-border/20 shrink-0">
        <span className="text-[13px] font-medium text-muted-foreground uppercase tracking-wider">
          {tr("上下文", "Context")}
        </span>
        <button
          type="button"
          aria-label={tr("关闭上下文面板", "Close context dock")}
          onClick={onClose}
          className="w-6 h-6 rounded-md flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-secondary/50 transition-colors"
        >
          <X size={13} />
        </button>
      </div>

      {sidebarView === "artifact" ? (
        <ArtifactView bookId={bookId} />
      ) : (
        <div className="flex flex-col gap-2 p-3">
          {CONTEXT_DOCK_PANELS.map((dockPanel) => (
            <div key={dockPanel.id} data-testid={`dock-panel-${dockPanel.id}`}>
              {dockPanel.render({ bookId, theme, t, sse }, isZh)}
            </div>
          ))}
        </div>
      )}
    </aside>
  );
}
