import { useCallback, useEffect, useRef, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";
import { ConfirmDialog } from "./ConfirmDialog";

// R60 第二批/1683 号：章级回收站面板（BookDetail 挂载）。条目按回滚批次分组：
// 批次头可整批恢复/整批删除，行内可单条恢复/删除；底部「清空回收站」= 全部
// 列出来的条目真空删除。所有真空删除走 danger ConfirmDialog 两步确认。恢复
// 成功回调 onRestored（BookDetail refetch 章节列表——找回的章已由服务端登记
// 进章节目录）。消费 1679 号端点：GET /api/v1/books/:id/trash、POST …/restore
// （{path} 单条 | {batch} 整批）、POST …/purge（{path} | {batch} | {all:true}）。

interface ChapterTrashEntry {
  readonly path: string;
  readonly originalPath: string;
  readonly batch: string | null;
  readonly bytes: number;
  readonly modifiedAt: string;
  readonly trashedAt: string | null;
}

interface TrashGroup {
  readonly batch: string | null;
  readonly items: ChapterTrashEntry[];
}

/// 列表已按（trashedAt ?? modifiedAt）降序 + path 升序排好；相邻同批次聚合。
function groupByBatch(entries: ReadonlyArray<ChapterTrashEntry>): ReadonlyArray<TrashGroup> {
  const groups: TrashGroup[] = [];
  for (const entry of entries) {
    const last = groups[groups.length - 1];
    if (last && last.batch === entry.batch) {
      last.items.push(entry);
    } else {
      groups.push({ batch: entry.batch, items: [entry] });
    }
  }
  return groups;
}

export function ChapterTrashPanel({ bookId, onRestored }: { readonly bookId: string; readonly onRestored: () => void }) {
  const [expanded, setExpanded] = useState(false);
  const [entries, setEntries] = useState<ReadonlyArray<ChapterTrashEntry> | null>(null);
  const [notice, setNotice] = useState<{ readonly text: string; readonly error: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  // 待确认的真空删除：{ all } 清空、{ batch } 整批、{ path } 单条。
  const [pendingPurge, setPendingPurge] = useState<{ readonly label: string; readonly body: Record<string, unknown>; readonly affectsChapters: boolean } | null>(null);

  const base = `/api/v1/books/${bookId}/trash`;
  const loadSeqRef = useRef(0);
  const loadEntries = useCallback(async () => {
    const seq = ++loadSeqRef.current;
    try {
      const result = await fetchJson<{ entries?: ReadonlyArray<ChapterTrashEntry> }>(base);
      if (seq !== loadSeqRef.current) return;
      setEntries(result.entries ?? []);
    } catch (error) {
      if (seq !== loadSeqRef.current) return;
      setEntries([]);
      setNotice({ text: String(error), error: true });
    }
  }, [base]);
  useEffect(() => {
    if (expanded) void loadEntries();
  }, [expanded, loadEntries]);

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

  const restore = (body: Record<string, unknown>) =>
    runExclusive(async () => {
      await fetchJson(`${base}/restore`, { method: "POST", body: JSON.stringify(body) });
      setNotice({ text: tr("已恢复，章节列表已刷新。", "Restored; the chapter list has been refreshed."), error: false });
      onRestored();
      void loadEntries();
    });

  const purge = (body: Record<string, unknown>, affectsChapters: boolean) =>
    runExclusive(async () => {
      await fetchJson(`${base}/purge`, { method: "POST", body: JSON.stringify(body) });
      setPendingPurge(null);
      setNotice({ text: tr("已永久删除。", "Deleted for good."), error: false });
      if (affectsChapters) onRestored();
      void loadEntries();
    });

  const groups = entries ? groupByBatch(entries) : [];
  const count = entries?.length ?? 0;
  const when = (entry: ChapterTrashEntry): string =>
    new Date(entry.trashedAt ?? entry.modifiedAt).toLocaleString();

  return (
    <div className="rounded-lg border border-border/60 px-5 py-4">
      <div className="flex items-center justify-between gap-4 flex-wrap">
        <div>
          <div className="text-sm font-medium">
            {tr("章节回收站", "Chapter trash")}
            {count > 0 && <span className="ml-2 text-xs text-destructive">{count}</span>}
          </div>
          <div className="text-xs text-muted-foreground mt-0.5">
            {tr("被驳回/删除的章节内容在这里，可整批或单条找回。", "Rejected and deleted chapter content is kept here; restore it per entry or per rollback batch.")}
          </div>
        </div>
        <div className="flex items-center gap-2">
          {expanded && count > 0 && (
            <button
              onClick={() =>
                setPendingPurge({
                  label: tr(`回收站里的 ${count} 条内容`, `${count} entries in the trash`),
                  body: { all: true },
                  affectsChapters: true,
                })
              }
              disabled={busy}
              className="px-3 py-1.5 text-xs rounded-md bg-destructive/10 text-destructive font-medium disabled:opacity-50"
            >
              {tr("清空回收站", "Empty the trash")}
            </button>
          )}
          <button
            onClick={() => setExpanded(!expanded)}
            className="px-4 py-2 text-xs rounded-lg border border-border font-medium cursor-pointer hover:bg-primary/10 transition-colors"
          >
            {expanded ? tr("收起", "Hide") : tr("回收站", "Trash")}
          </button>
        </div>
      </div>
      {expanded && (
        <div className="w-full space-y-3 border-t border-border/40 pt-3 mt-3">
          {notice && (
            <p className={`text-xs ${notice.error ? "text-destructive" : "text-primary"}`}>{notice.text}</p>
          )}
          {entries === null && <p className="text-xs text-muted-foreground">{tr("读取中…", "Loading…")}</p>}
          {entries !== null && count === 0 && (
            <p className="text-xs text-muted-foreground">{tr("回收站是空的。", "The trash is empty.")}</p>
          )}
          {groups.map((group) => (
            <div key={group.batch ?? "flat"} className="space-y-1">
              {group.batch !== null && (
                <div className="flex items-center justify-between gap-3 text-xs pt-1">
                  <span className="text-muted-foreground font-medium">
                    {tr("回滚批次", "Rollback batch")} {group.batch} · {new Date(group.items[0]!.trashedAt ?? group.items[0]!.modifiedAt).toLocaleString()}
                  </span>
                  <span className="flex items-center gap-2 shrink-0">
                    <button
                      onClick={() => void restore({ batch: group.batch })}
                      disabled={busy}
                      className="px-2.5 py-1 rounded-md bg-primary/10 text-primary font-medium disabled:opacity-50"
                    >
                      {tr("整批恢复", "Restore batch")}
                    </button>
                    <button
                      onClick={() =>
                        setPendingPurge({
                          label: tr(`批次 ${group.batch} 的 ${group.items.length} 条内容`, `${group.items.length} entries of batch ${group.batch}`),
                          body: { batch: group.batch },
                          affectsChapters: group.items.some((item) => item.originalPath.startsWith("chapters/")),
                        })
                      }
                      disabled={busy}
                      className="px-2.5 py-1 rounded-md bg-destructive/10 text-destructive font-medium disabled:opacity-50"
                    >
                      {tr("整批删除", "Delete batch")}
                    </button>
                  </span>
                </div>
              )}
              {group.items.map((item) => (
                <div key={item.path} className="flex items-center justify-between gap-3 text-xs flex-wrap">
                  <span className="min-w-0">
                    <span className="font-medium">{item.originalPath}</span>
                    <span className="text-muted-foreground ml-2">
                      {when(item)} · {item.bytes} B
                      {item.batch === null && <span className="ml-1">· {tr("平铺条目", "loose entry")}</span>}
                    </span>
                  </span>
                  <span className="flex items-center gap-2 shrink-0">
                    <button
                      onClick={() => void restore({ path: item.path })}
                      disabled={busy}
                      className="px-3 py-1.5 rounded-md bg-primary/10 text-primary font-medium disabled:opacity-50"
                    >
                      {tr("恢复", "Restore")}
                    </button>
                    <button
                      onClick={() =>
                        setPendingPurge({
                          label: item.originalPath,
                          body: { path: item.path },
                          affectsChapters: item.originalPath.startsWith("chapters/"),
                        })
                      }
                      disabled={busy}
                      className="px-3 py-1.5 rounded-md bg-destructive/10 text-destructive font-medium disabled:opacity-50"
                    >
                      {tr("删除", "Delete")}
                    </button>
                  </span>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
      <ConfirmDialog
        open={pendingPurge !== null}
        title={tr("永久删除", "Delete for good")}
        message={tr(
          `「${pendingPurge?.label ?? ""}」将被永久删除，无法恢复。`,
          `"${pendingPurge?.label ?? ""}" will be deleted for good. This cannot be undone.`,
        )}
        confirmLabel={tr("永久删除", "Delete for good")}
        cancelLabel={tr("取消", "Cancel")}
        variant="danger"
        onConfirm={() => {
          const pending = pendingPurge;
          setPendingPurge(null);
          if (pending) void purge(pending.body, pending.affectsChapters);
        }}
        onCancel={() => setPendingPurge(null)}
      />
    </div>
  );
}
