import { useState } from "react";
import { Sidebar } from "./Sidebar";
import { usePanelWidth } from "../hooks/use-panel-width";
import { tr } from "../lib/app-language";
import type { Nav } from "../lib/nav";
import type { NavSectionId } from "../lib/nav-sections";
import type { SSEMessage } from "../hooks/use-sse";
import type { TFunction } from "../hooks/use-i18n";

/**
 * 按区渲染的侧面板（P3-1/P3-2）。
 * 复用 Sidebar 的 zone 过滤（同一组件两种形态，双轨期零逻辑重复），
 * 外层补三件事：宽度拖拽（180~400 clamp + 记忆 + 双击复位）、
 * 创作区的树过滤输入、Cmd+B 隐藏（App 持有可见态，V2 下活动栏即图标条）。
 */
export function SidePanel({ nav, activePage, sse, t, zone, visible }: {
  nav: Nav;
  activePage: string;
  sse: { messages: ReadonlyArray<SSEMessage> };
  t: TFunction;
  zone: NavSectionId;
  visible: boolean;
}) {
  const panel = usePanelWidth();
  const [filterQuery, setFilterQuery] = useState("");

  if (!visible) return null;

  return (
    <div data-slot="side-panel" data-testid="side-panel" className="relative flex shrink-0 h-full" style={{ width: panel.width }}>
      <div className="flex min-w-0 flex-1 flex-col">
        {zone === "create" && (
          <div className="px-4 pt-3 pb-1">
            <input
              type="text"
              data-testid="panel-filter"
              value={filterQuery}
              onChange={(event) => setFilterQuery(event.target.value)}
              placeholder={tr("过滤书与会话…", "Filter books & sessions…")}
              className="w-full rounded-lg border border-border/50 bg-muted/40 px-2.5 py-1.5 text-[13px] text-foreground placeholder:text-muted-foreground/60 outline-none focus:border-border transition-colors"
            />
          </div>
        )}
        <Sidebar
          nav={nav}
          activePage={activePage}
          sse={sse}
          t={t}
          zone={zone}
          fillWidth
          filterQuery={filterQuery || undefined}
        />
      </div>

      {/* 宽度拖拽分隔条：拖动钳制 180~400，双击复位 260 */}
      <div
        data-testid="panel-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label={tr("调整面板宽度", "Resize panel")}
        onPointerDown={panel.beginResize}
        onPointerMove={panel.handleResizeMove}
        onPointerUp={panel.endResize}
        onDoubleClick={panel.resetWidth}
        className="w-1 shrink-0 cursor-col-resize bg-transparent hover:bg-primary/30 active:bg-primary/50 transition-colors"
      />
    </div>
  );
}
