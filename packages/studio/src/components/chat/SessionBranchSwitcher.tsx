import { useCallback, useState } from "react";
import { GitBranch, History, Loader2 } from "lucide-react";
import { fetchJson } from "../../hooks/use-api";
import { useChatStore } from "../../store/chat";
import type { SessionBranchPoint, SessionBranchPointsResponse } from "../../store/chat/types";
import { ConfirmDialog } from "../ConfirmDialog";
import { tr } from "../../lib/app-language";

/**
 * R36 分支切换器（638 号）：消费 GET /sessions/:id/branches 与
 * POST /sessions/:id/branch（637 号双端 REST）。挂在聊天输入状态条上——
 * 分支点 = 已提交轮的收尾提交点，切换 = head 指针移动（append-only，
 * 弃用路径零删改、可切回），成功后 store 重拉详情，active 链投影立即生效。
 */

function formatRelative(timestamp: number): string {
  const diff = Date.now() - timestamp;
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 1) return tr("刚刚", "just now");
  if (minutes < 60) return tr(`${minutes} 分钟前`, `${minutes}m ago`);
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return tr(`${hours} 小时前`, `${hours}h ago`);
  const days = Math.floor(hours / 24);
  return tr(`${days} 天前`, `${days}d ago`);
}

function previewText(point: SessionBranchPoint, isZh: boolean): string {
  const oneLine = point.preview.trim().replace(/\s+/g, " ");
  if (!oneLine) return isZh ? "（无文本预览）" : "(no text preview)";
  return oneLine.length > 42 ? `${oneLine.slice(0, 42)}…` : oneLine;
}

export function SessionBranchSwitcher({
  sessionId,
  isDraft,
  disabled,
  isZh,
}: {
  readonly sessionId: string | null;
  readonly isDraft: boolean;
  readonly disabled: boolean;
  readonly isZh: boolean;
}) {
  const branchSession = useChatStore((s) => s.branchSession);
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [points, setPoints] = useState<ReadonlyArray<SessionBranchPoint>>([]);
  const [branchCount, setBranchCount] = useState(0);
  const [confirmTarget, setConfirmTarget] = useState<SessionBranchPoint | null>(null);
  const [switching, setSwitching] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadPoints = useCallback(async (targetId: string) => {
    setLoading(true);
    setLoadFailed(false);
    try {
      const data = await fetchJson<SessionBranchPointsResponse>(
        `/sessions/${encodeURIComponent(targetId)}/branches`,
      );
      // 新轮在前（seq 降序）——最近对话离用户操作意图最近。
      setPoints([...data.points].sort((a, b) => b.seq - a.seq));
      setBranchCount(data.branchCount);
    } catch {
      setLoadFailed(true);
      setPoints([]);
    } finally {
      setLoading(false);
    }
  }, []);

  const toggleOpen = () => {
    const next = !open;
    setOpen(next);
    setError(null);
    if (next && sessionId) void loadPoints(sessionId);
  };

  const handleConfirmSwitch = async () => {
    if (!sessionId || !confirmTarget) return;
    setSwitching(true);
    setError(null);
    try {
      await branchSession(sessionId, confirmTarget.seq);
      setConfirmTarget(null);
      setOpen(false);
      setPoints([]);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setConfirmTarget(null);
    } finally {
      setSwitching(false);
    }
  };

  if (!sessionId || isDraft) return null;

  // 「当前」= 在链点位中 seq 最大者（head 至少推进到它；其后的元事件不改写对话）。
  const currentSeq = points.reduce<number | null>(
    (acc, point) => (point.onActiveChain && (acc === null || point.seq > acc) ? point.seq : acc),
    null,
  );

  return (
    <div className="relative" data-slot="session-branch-switcher">
      <button
        type="button"
        data-slot="session-branch-trigger"
        onClick={toggleOpen}
        disabled={disabled}
        className={`flex items-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-medium transition-colors disabled:opacity-40 ${
          open ? "bg-primary/10 text-primary" : "text-muted-foreground/70 hover:bg-muted hover:text-primary"
        }`}
        title={isZh ? "对话分支：回到某一轮重开" : "Conversation branches: rewind to a round"}
        aria-label={isZh ? "对话分支" : "Conversation branches"}
      >
        <GitBranch size={13} />
        {branchCount > 0 ? <span data-slot="session-branch-count">{branchCount}</span> : null}
      </button>

      {open ? (
        <div
          data-slot="session-branch-panel"
          className="absolute bottom-[calc(100%+10px)] right-0 z-40 w-80 overflow-hidden rounded-2xl border border-border/60 bg-card/95 shadow-2xl backdrop-blur"
        >
          <div className="border-b border-border/40 px-4 py-3">
            <div className="flex items-center gap-2 text-sm font-bold">
              <History size={14} className="text-primary" />
              {isZh ? "对话分支" : "Conversation branches"}
            </div>
            <p className="mt-0.5 text-xs leading-5 text-muted-foreground">
              {isZh
                ? "回到某一轮结束的位置继续；之后的对话不会删除，可随时切回。"
                : "Rewind to the end of any round; later turns are kept and can be restored."}
            </p>
          </div>
          <div className="max-h-[300px] overflow-y-auto p-2">
            {loading ? (
              <div className="flex items-center justify-center gap-2 px-2 py-6 text-sm text-muted-foreground">
                <Loader2 size={14} className="animate-spin" />
                {isZh ? "加载分支点..." : "Loading branch points..."}
              </div>
            ) : loadFailed ? (
              <div className="px-2 py-6 text-center text-sm text-muted-foreground">
                {isZh ? "分支点加载失败。" : "Failed to load branch points."}
              </div>
            ) : points.length === 0 ? (
              <div className="px-2 py-6 text-center text-sm text-muted-foreground">
                {isZh ? "还没有已完成的对话轮。" : "No committed rounds yet."}
              </div>
            ) : (
              points.map((point, index) => {
                const isCurrent = point.seq === currentSeq;
                return (
                  <button
                    key={point.seq}
                    type="button"
                    data-slot="session-branch-point"
                    disabled={isCurrent || switching}
                    onClick={() => setConfirmTarget(point)}
                    className={`w-full rounded-xl border px-3 py-2 text-left transition-all ${
                      isCurrent
                        ? "border-primary/40 bg-primary/5"
                        : "border-transparent hover:border-primary/30 hover:bg-secondary/35"
                    } disabled:cursor-default`}
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-xs font-semibold text-foreground">
                        {isZh ? `第 ${points.length - index} 轮` : `Round ${points.length - index}`}
                      </span>
                      {isCurrent ? (
                        <span
                          data-slot="session-branch-current"
                          className="rounded-full bg-primary/15 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-primary"
                        >
                          {isZh ? "当前" : "current"}
                        </span>
                      ) : !point.onActiveChain ? (
                        <span
                          data-slot="session-branch-abandoned"
                          className="rounded-full bg-secondary px-2 py-0.5 text-[10px] font-medium text-muted-foreground"
                        >
                          {isZh ? "已弃用分支" : "abandoned"}
                        </span>
                      ) : null}
                      <span className="ml-auto shrink-0 text-[11px] text-muted-foreground/50">
                        {formatRelative(point.timestamp)}
                      </span>
                    </div>
                    <p className="mt-0.5 truncate text-xs leading-5 text-muted-foreground">
                      {previewText(point, isZh)}
                    </p>
                  </button>
                );
              })
            )}
            {error ? (
              <div className="mt-1 rounded-xl bg-destructive/10 px-3 py-2 text-xs text-destructive" data-slot="session-branch-error">
                {error}
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      <ConfirmDialog
        open={confirmTarget !== null}
        title={isZh ? "切换对话分支" : "Switch branch"}
        message={
          confirmTarget
            ? (isZh
              ? `回到「${previewText(confirmTarget, isZh)}」之后继续对话？之后的轮次不会删除，随时可从本面板切回。`
              : `Continue from "${previewText(confirmTarget, isZh)}"? Later turns are kept and can be restored anytime.`)
            : ""
        }
        confirmLabel={isZh ? "切到此分支" : "Switch"}
        cancelLabel={isZh ? "取消" : "Cancel"}
        onConfirm={() => void handleConfirmSwitch()}
        onCancel={() => { if (!switching) setConfirmTarget(null); }}
      />
    </div>
  );
}
