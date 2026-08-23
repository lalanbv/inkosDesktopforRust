import { useEffect, useRef, useState } from "react";
import { Bell, CheckCheck, Trash2, X } from "lucide-react";
import { tr } from "@/lib/app-language";
import { countUnread, useNotificationsStore } from "@/store/notifications";
import type { StudioNotification } from "@/store/notifications";

/**
 * 通知中心（P4-3）：右下角铃铛（未读徽标）+ 弹出面板。
 * 分级配色 info/warn/error；全部已读 / 清空；打开即全部已读。
 * 通知不阻塞（无模态），Esc / 点击外部关闭。
 */

const LEVEL_DOT: Record<StudioNotification["level"], string> = {
  info: "bg-primary/70",
  warn: "bg-amber-500",
  error: "bg-destructive",
};

function formatTime(at: number): string {
  const d = new Date(at);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

export function NotificationCenter() {
  const notifications = useNotificationsStore((s) => s.notifications);
  const markAllRead = useNotificationsStore((s) => s.markAllRead);
  const clearNotifications = useNotificationsStore((s) => s.clearNotifications);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const unread = countUnread(notifications);

  // 打开即视为全部已读
  useEffect(() => {
    if (open) markAllRead();
  }, [open, markAllRead]);

  // 点击外部 / Esc 关闭
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="fixed right-4 bottom-10 z-40 flex flex-col items-end">
      {open && (
        <div
          data-testid="notification-panel"
          className="mb-2 w-80 max-h-96 overflow-y-auto rounded-xl border border-border/50 bg-popover text-popover-foreground shadow-xl"
        >
          <div className="flex items-center justify-between border-b border-border/40 px-3 py-2">
            <span className="text-[13px] font-medium">{tr("通知", "Notifications")}</span>
            <div className="flex items-center gap-1">
              <button
                type="button"
                aria-label={tr("全部已读", "Mark all read")}
                onClick={markAllRead}
                className="rounded p-1 text-muted-foreground hover:text-foreground hover:bg-secondary/50"
              >
                <CheckCheck size={13} />
              </button>
              <button
                type="button"
                aria-label={tr("清空通知", "Clear notifications")}
                onClick={clearNotifications}
                className="rounded p-1 text-muted-foreground hover:text-foreground hover:bg-secondary/50"
              >
                <Trash2 size={13} />
              </button>
            </div>
          </div>
          {notifications.length === 0 ? (
            <p className="px-4 py-6 text-center text-[13px] text-muted-foreground/60 italic">
              {tr("暂无通知", "No notifications")}
            </p>
          ) : (
            notifications.map((item) => (
              <div key={item.id} className="flex gap-2 border-b border-border/30 px-3 py-2 last:border-0">
                <span className={`mt-1.5 size-1.5 shrink-0 rounded-full ${LEVEL_DOT[item.level]}`} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px] font-medium">{item.title}</div>
                  {item.detail && <div className="truncate text-xs text-muted-foreground/70">{item.detail}</div>}
                </div>
                <span className="shrink-0 text-[11px] text-muted-foreground/50">{formatTime(item.at)}</span>
              </div>
            ))
          )}
        </div>
      )}

      <button
        type="button"
        data-testid="notification-bell"
        aria-label={tr(unread > 0 ? `通知（${unread} 条未读）` : "通知", unread > 0 ? `Notifications (${unread} unread)` : "Notifications")}
        onClick={() => setOpen((value) => !value)}
        className="relative flex size-8 items-center justify-center rounded-lg border border-border/50 bg-card/80 text-muted-foreground shadow-sm hover:text-foreground transition-colors"
      >
        {open ? <X size={14} /> : <Bell size={14} />}
        {unread > 0 && (
          <span
            data-testid="notification-unread"
            className="absolute -right-1 -top-1 flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[9px] font-bold text-destructive-foreground"
          >
            {unread > 9 ? "9+" : unread}
          </span>
        )}
      </button>
    </div>
  );
}
