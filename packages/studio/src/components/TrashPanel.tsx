import { useCallback, useEffect, useRef, useState } from "react";
// 浏览器组件走独立子路径（纯算术模块）——别把 state/trash 的 node:fs 依赖链
// 经 CJS 桶拉进 vite 图（gate:ts build 腿红过的点）。
import { trashDaysLeft } from "@actalk/inkos-core/utils/trash-countdown";
import { fetchJson, invalidateApiPaths } from "../hooks/use-api";
import { tr } from "../lib/app-language";
import { ConfirmDialog } from "./ConfirmDialog";

// R60 第二批/1683 号：书级回收站面板（Dashboard 挂载，BackupPanel 同款外壳）。
// 恢复=把书搬回作品库（可逆，不需要确认）；彻底删除=真空删除（danger 两步确认）。
// 消费 1679 号的端点：GET /api/v1/trash/books、POST …/restore、POST …/purge。
// 409（书目录已被同 id 新书占用）与 404（条目已被别的窗口清掉）落错误条并重列。
// R60 第三批/1685 号：保留期提示——每条剩几天（≤7 天或已到期红色）；保留天数
// 来自 GET /project/trash-retention，取不到就只显示既有描述。

interface BookTrashEntry {
  readonly entry: string;
  readonly bookId: string;
  readonly trashedAt: string;
  readonly bytes: number;
  readonly fileCount: number;
  readonly title: string | null;
}

interface BookTrashError {
  readonly message: string;
}

const formatBytes = (bytes: number): string =>
  bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;

export function TrashPanel() {
  const [expanded, setExpanded] = useState(false);
  const [entries, setEntries] = useState<ReadonlyArray<BookTrashEntry> | null>(null);
  const [notice, setNotice] = useState<{ readonly text: string; readonly error: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [pendingPurge, setPendingPurge] = useState<BookTrashEntry | null>(null);

  // 1008 号惯例：响应竞态守卫（seq latest-wins）。
  const loadSeqRef = useRef(0);
  const loadEntries = useCallback(async () => {
    const seq = ++loadSeqRef.current;
    try {
      const result = await fetchJson<{ entries?: ReadonlyArray<BookTrashEntry> }>("/api/v1/trash/books");
      if (seq !== loadSeqRef.current) return;
      setEntries(result.entries ?? []);
    } catch (error) {
      if (seq !== loadSeqRef.current) return;
      setEntries([]);
      setNotice({ text: String(error), error: true });
    }
  }, []);
  useEffect(() => {
    void loadEntries();
  }, [loadEntries]);

  // 保留天数（GET 失败 = 不显示倒计时，只显示既有描述）。
  const [retentionDays, setRetentionDays] = useState<number | null>(null);
  useEffect(() => {
    void fetchJson<{ retentionDays: number }>("/api/v1/project/trash-retention")
      .then((payload) => setRetentionDays(Number.isInteger(payload.retentionDays) && payload.retentionDays > 0 ? payload.retentionDays : null))
      .catch(() => setRetentionDays(null));
  }, []);
  const expiryBadge = (trashedAt: string) => {
    if (retentionDays === null) return null;
    const left = trashDaysLeft(trashedAt, retentionDays, Date.now());
    if (left === null) return null;
    return left <= 0
      ? <span className="text-destructive ml-2">{tr("已到期，下次启动时清空", "Expired; purged on next launch")}</span>
      : <span className={`ml-2 ${left <= 7 ? "text-destructive" : "text-muted-foreground"}`}>{tr(`剩 ${left} 天`, `${left} ${left === 1 ? "day" : "days"} left`)}</span>;
  };

  // 955 号惯例：在途互斥（真人双击=两次请求）。
  const runExclusive = async (action: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await action();
    } catch (error) {
      setNotice({ text: String(error), error: true });
      void loadEntries();
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const restore = (book: BookTrashEntry) =>
    runExclusive(async () => {
      await fetchJson(`/api/v1/trash/books/restore`, { method: "POST", body: JSON.stringify({ entry: book.entry }) });
      setNotice({ text: tr(`已恢复「${book.title ?? book.bookId}」。`, `Restored "${book.title ?? book.bookId}".`), error: false });
      // 书架列表失效重取（服务端同时广播 book:restored）。
      invalidateApiPaths(["/api/v1/books"]);
      void loadEntries();
    });

  const purge = (book: BookTrashEntry) =>
    runExclusive(async () => {
      await fetchJson(`/api/v1/trash/books/purge`, { method: "POST", body: JSON.stringify({ entry: book.entry }) });
      setPendingPurge(null);
      setNotice({ text: tr("已永久删除。", "Deleted for good."), error: false });
      void loadEntries();
    });

  const count = entries?.length ?? 0;

  return (
    <div className="rounded-lg border border-border/60 px-5 py-4 mb-8">
      <div className="flex items-center justify-between gap-4 flex-wrap">
        <div>
          <div className="text-sm font-medium">
            {tr("回收站", "Trash")}
            {count > 0 && <span className="ml-2 text-xs text-destructive">{count}</span>}
          </div>
          <div className="text-xs text-muted-foreground mt-0.5">
            {tr("被删除的书在这里，可恢复或永久删除。", "Deleted books are kept here; restore them or delete them for good.")}
            {retentionDays !== null && (
              <span className="ml-1">
                {tr(`保留 ${retentionDays} 天，到期自动清空。`, `Kept for ${retentionDays} days, then purged automatically.`)}
              </span>
            )}
          </div>
        </div>
        <button
          onClick={() => setExpanded(!expanded)}
          className="px-4 py-2 text-xs rounded-lg border border-border font-medium cursor-pointer hover:bg-primary/10 transition-colors"
        >
          {expanded ? tr("收起", "Hide") : tr("查看回收站", "Show trash")}
        </button>
      </div>
      {expanded && (
        <div className="w-full space-y-2 border-t border-border/40 pt-3 mt-3">
          {notice && (
            <p className={`text-xs ${notice.error ? "text-destructive" : "text-primary"}`}>{notice.text}</p>
          )}
          {entries === null && <p className="text-xs text-muted-foreground">{tr("读取中…", "Loading…")}</p>}
          {entries !== null && count === 0 && (
            <p className="text-xs text-muted-foreground">{tr("回收站是空的。", "The trash is empty.")}</p>
          )}
          {entries?.map((book) => (
            <div key={book.entry} className="flex items-center justify-between gap-3 text-xs flex-wrap">
              <span className="min-w-0">
                <span className="font-medium">{book.title ?? book.bookId}</span>
                <span className="text-muted-foreground ml-2">
                  {new Date(book.trashedAt).toLocaleString()} · {book.fileCount} {tr("个文件", "files")} · {formatBytes(book.bytes)}
                  {expiryBadge(book.trashedAt)}
                </span>
              </span>
              <span className="flex items-center gap-2 shrink-0">
                <button
                  onClick={() => void restore(book)}
                  disabled={busy}
                  className="px-3 py-1.5 rounded-md bg-primary/10 text-primary font-medium disabled:opacity-50"
                >
                  {tr("恢复", "Restore")}
                </button>
                <button
                  onClick={() => setPendingPurge(book)}
                  disabled={busy}
                  className="px-3 py-1.5 rounded-md bg-destructive/10 text-destructive font-medium disabled:opacity-50"
                >
                  {tr("彻底删除", "Delete for good")}
                </button>
              </span>
            </div>
          ))}
        </div>
      )}
      <ConfirmDialog
        open={pendingPurge !== null}
        title={tr("永久删除", "Delete for good")}
        message={tr(
          `「${pendingPurge?.title ?? pendingPurge?.bookId ?? ""}」将被永久删除，无法恢复。`,
          `"${pendingPurge?.title ?? pendingPurge?.bookId ?? ""}" will be deleted for good. This cannot be undone.`,
        )}
        confirmLabel={tr("永久删除", "Delete for good")}
        cancelLabel={tr("取消", "Cancel")}
        variant="danger"
        onConfirm={() => {
          const book = pendingPurge;
          setPendingPurge(null);
          if (book) void purge(book);
        }}
        onCancel={() => setPendingPurge(null)}
      />
    </div>
  );
}

export type { BookTrashEntry, BookTrashError };
