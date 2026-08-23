import { useRef } from "react";
import { Pin, PinOff, X } from "lucide-react";
import { tr } from "@/lib/app-language";
import type { TabState } from "@/lib/tabs-reducer";
import { cn } from "@/lib/utils";

/**
 * 标签条（P3-3）：溢出横向滚动；中键关闭；右键菜单（关闭/关闭其它/固定）；
 * 双击 = 固定转正（预览语义的"驻留"入口）。拖拽重排后置（151 号允许）。
 */
export function TabStrip({ tabs, activeId, onActivate, onClose, onCloseOthers, onPin }: {
  tabs: ReadonlyArray<TabState>;
  activeId: string | null;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onCloseOthers: (id: string) => void;
  onPin: (id: string, pinned: boolean) => void;
}) {
  const menuTargetRef = useRef<{ id: string; pinned: boolean } | null>(null);
  if (tabs.length === 0) return null;

  const openMenu = (event: React.MouseEvent, tab: TabState) => {
    // 原生 contextmenu：用动态定位的简单浮层（避免引 DropdownMenu 的 portal 复杂度）
    event.preventDefault();
    menuTargetRef.current = { id: tab.id, pinned: tab.pinned };
    const menu = document.getElementById("tabstrip-context-menu");
    if (!menu) return;
    menu.style.left = `${event.clientX}px`;
    menu.style.top = `${event.clientY}px`;
    menu.classList.remove("hidden");
  };

  return (
    <div
      data-slot="tab-strip"
      data-testid="tab-strip"
      className="h-9 shrink-0 flex items-stretch overflow-x-auto no-scrollbar border-b border-border/40 bg-background/50"
    >
      {tabs.map((tab, index) => {
        const isActive = tab.id === activeId;
        return (
          <div
            key={tab.id}
            data-testid={`tab-${tab.id}`}
            data-active={isActive ? "true" : undefined}
            className={cn(
              "group/tab relative flex max-w-44 min-w-28 shrink-0 cursor-default items-center gap-1.5 border-r border-border/40 px-3 text-[13px]",
              isActive
                ? "bg-secondary/60 text-foreground"
                : "text-muted-foreground hover:text-foreground hover:bg-secondary/30",
            )}
            onClick={() => onActivate(tab.id)}
            onDoubleClick={() => onPin(tab.id, true)}
            onAuxClick={(event) => {
              if (event.button === 1) onClose(tab.id);
            }}
            onContextMenu={(event) => openMenu(event, tab)}
            title={tab.title}
          >
            <span className="truncate flex-1 select-none">
              {tab.title}
              {tab.preview && (
                <span className="ml-1 italic text-muted-foreground/60">{tr("预览", "preview")}</span>
              )}
            </span>
            {tab.pinned && <Pin size={11} className="shrink-0 text-primary/70" />}
            <button
              type="button"
              aria-label={tr(`关闭 ${tab.title}`, `Close ${tab.title}`)}
              onClick={(event) => {
                event.stopPropagation();
                onClose(tab.id);
              }}
              className="shrink-0 rounded p-0.5 opacity-0 group-hover/tab:opacity-100 hover:bg-secondary text-muted-foreground hover:text-foreground transition-opacity"
            >
              <X size={12} />
            </button>
            {index < 9 && isActive && null}
          </div>
        );
      })}

      {/* 右键菜单（单例，点击任意处收起） */}
      <div
        id="tabstrip-context-menu"
        className="fixed z-50 hidden min-w-36 rounded-lg border border-border bg-popover py-1 text-[13px] shadow-lg"
        onClick={(event) => event.stopPropagation()}
      >
        <button
          type="button"
          className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-secondary/60"
          onClick={() => {
            if (menuTargetRef.current) onPin(menuTargetRef.current.id, true);
            document.getElementById("tabstrip-context-menu")?.classList.add("hidden");
          }}
        >
          <Pin size={13} />
          {tr("固定标签", "Pin tab")}
        </button>
        <button
          type="button"
          className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-secondary/60"
          onClick={() => {
            if (menuTargetRef.current?.pinned) onPin(menuTargetRef.current.id, false);
            document.getElementById("tabstrip-context-menu")?.classList.add("hidden");
          }}
        >
          <PinOff size={13} />
          {tr("取消固定", "Unpin tab")}
        </button>
        <div className="my-1 border-t border-border/60" />
        <button
          type="button"
          className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-secondary/60"
          onClick={() => {
            if (menuTargetRef.current) onClose(menuTargetRef.current.id);
            document.getElementById("tabstrip-context-menu")?.classList.add("hidden");
          }}
        >
          <X size={13} />
          {tr("关闭", "Close")}
        </button>
        <button
          type="button"
          className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-secondary/60"
          onClick={() => {
            if (menuTargetRef.current) onCloseOthers(menuTargetRef.current.id);
            document.getElementById("tabstrip-context-menu")?.classList.add("hidden");
          }}
        >
          <X size={13} />
          {tr("关闭其它", "Close others")}
        </button>
      </div>
    </div>
  );
}
