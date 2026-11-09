import { useCallback, useEffect, useRef, useState } from "react";
import { fetchJson, invalidateApiPaths } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface Preview {
  readonly fileCount: number;
  readonly overwriting: ReadonlyArray<string>;
  readonly newFiles: number;
  /** 956 号：快照记录的恢复期间新增文件（回滚预览透传，可勾选一并删除）。 */
  readonly introduced?: ReadonlyArray<string>;
}

interface SnapshotItem {
  readonly name: string;
  readonly size: number;
}

/**
 * 954 号：从快照名（pre-restore-<millis>.tar）解析本地时间展示；解析失败回退原名。
 * 955 号：越界毫秒（16 位名可超 Date 上限 8.64e15 → Invalid Date）同样回退原名。
 */
function snapshotLabel(name: string): string {
  const digits = name.match(/^pre-restore-(\d{1,16})\.tar$/)?.[1];
  if (digits === undefined) return name;
  const date = new Date(Number(digits));
  return Number.isNaN(date.getTime()) ? name : date.toLocaleString();
}

/**
 * 955 号：列表标签——同秒快照（连续/并发恢复各留一份）秒级标签相同、大小也常
 * 相同，用户无从分辨该回滚哪份（真引擎走查实证两行同为「11:03:13 · 7168 字节」）；
 * 同标签者追加毫秒。
 */
function snapshotLabels(items: ReadonlyArray<SnapshotItem>): ReadonlyArray<string> {
  const labels = items.map((item) => snapshotLabel(item.name));
  const counts = new Map<string, number>();
  for (const label of labels) counts.set(label, (counts.get(label) ?? 0) + 1);
  return labels.map((label, index) => {
    if ((counts.get(label) ?? 0) < 2) return label;
    const digits = items[index]!.name.match(/^pre-restore-(\d{1,16})\.tar$/)?.[1];
    return digits === undefined ? label : `${label}.${digits.slice(-3).padStart(3, "0")}`;
  });
}

/**
 * R18/386 号：备份与恢复卡（Dashboard 挂载）。
 * 导出=全量 tar.gz 下载；导入=两段式（预览 → 确认写回），恢复前自动快照
 * 至 backups/pre-restore-*.tar（回滚保险）。954 号：快照可列举可回滚——
 * 回滚走服务端同一两段式（预览覆盖面 → 确认），恢复/回滚全程有保险。
 */
export function BackupPanel() {
  const [notice, setNotice] = useState<{ readonly text: string; readonly error: boolean } | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const [snapshots, setSnapshots] = useState<ReadonlyArray<SnapshotItem>>([]);
  const [rollback, setRollback] = useState<{ name: string; label: string; preview: Preview } | null>(null);
  // 956 号：完全回滚复选框——「同时删除恢复期间新增文件」默认不勾（保守回滚
  // 语义不变，删除是用户显式选择）。
  const [deleteIntroduced, setDeleteIntroduced] = useState(false);

  const loadSnapshots = useCallback(async () => {
    try {
      const result = await fetchJson<{ snapshots?: ReadonlyArray<SnapshotItem> }>("/backup/snapshots");
      setSnapshots(result.snapshots ?? []);
    } catch {
      setSnapshots([]); // 列举失败不阻断面板主功能（导出/导入不受影响）
    }
  }, []);
  useEffect(() => {
    void loadSnapshots();
  }, [loadSnapshots]);

  // 955 号：在途互斥——预览/确认请求未返回前操作按钮禁用+重入早退（真人双击
  // 「确认」=两次恢复请求与冗余快照）；ref 兜住 state 渲染前的同帧连击窗口。
  // 失败统一落 error 通知（954 前错误与成功同为绿色，失败看起来像成功），且
  // 失败后重列快照：盘面可能已变（快照被外部删除=列表残留死行；确认恢复在
  // 快照落盘后中途失败=新快照已在盘却不在列表，回滚保险对用户不可见）。
  const busyRef = useRef(false);
  const [busy, setBusy] = useState(false);
  const runExclusive = async (action: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await action();
    } catch (error) {
      setNotice({ text: String(error), error: true });
      void loadSnapshots();
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const doExport = () => {
    window.open(`${location.origin}/api/v1/backup/export`, "_blank");
  };

  const onFile = (file: File) => runExclusive(async () => {
    // 955 号：新选文件先收起旧预览（954 前选 B 失败时 A 的预览仍在、待恢复
    // 文件已换成 B——确认会按 A 的数字恢复 B）；导入预览与回滚预览互斥。
    setPreview(null);
    setRollback(null);
    setNotice(null);
    setPendingFile(file);
    const raw = await file.arrayBuffer();
    // 443 号：服务端线格式为 {preview:{...}}（TS/Rust 一致）——面板此前按
    // 平铺读取，预览数字全为 undefined，导入预览从未真正可读。
    const result = await fetchJson<{ preview: Preview }>("/backup/import", {
      method: "POST",
      body: raw as unknown as BodyInit,
    });
    setPreview(result.preview);
  });

  const confirmImport = () => runExclusive(async () => {
    if (!pendingFile) return;
    const raw = await pendingFile.arrayBuffer();
    const result = await fetchJson<{ ok: boolean; restored: number }>(
      "/backup/import?confirm=1",
      { method: "POST", body: raw as unknown as BodyInit },
    );
    setNotice({ text: tr(`已恢复 ${result.restored} 个文件（原文件快照在 backups/）`, `Restored ${result.restored} files (snapshot in backups/)`), error: false });
    setPreview(null);
    setPendingFile(null);
    // 956 号走查反挖（P9）：恢复会增删书/改书目内容——广播 /api/v1/books 失效
    // （postApi 的自动失效面不含 backup 路由，deriveInvalidationPaths 对其返回空），
    // 否则书籍列表停留在恢复前状态（真引擎走查实证：完全回滚删除 b2 后列表仍显示）。
    invalidateApiPaths(["/api/v1/books"]);
    void loadSnapshots();
  });

  const previewRollback = (name: string, label: string) => runExclusive(async () => {
    const result = await fetchJson<{ preview: Preview }>(
      `/backup/rollback?name=${encodeURIComponent(name)}`,
      { method: "POST" },
    );
    // 955 号：与导入预览互斥（并存时确认其一后另一份预览已过期）。
    setPreview(null);
    setPendingFile(null);
    setRollback({ name, label, preview: result.preview });
    // 956 号：新预览重置删除勾选（不同快照的 introduced 清单不同，勾选不得跨快照残留）。
    setDeleteIntroduced(false);
    setNotice(null);
  });

  const confirmRollback = () => runExclusive(async () => {
    if (!rollback) return;
    const suffix = deleteIntroduced && rollback.preview.introduced ? "&deleteIntroduced=1" : "";
    const result = await fetchJson<{ ok: boolean; restored: number; removed?: number; deleteFailed?: ReadonlyArray<{ path: string; error: string }> }>(
      `/backup/rollback?name=${encodeURIComponent(rollback.name)}&confirm=1${suffix}`,
      { method: "POST" },
    );
    // 956 号：完全回滚通知含删除计数；删除失败明细随通知展示（写回已成功，
    // 服务端保持 200——部分失败不谎报整体失败）。
    const deletedPart = result.removed !== undefined
      ? tr(`、删除 ${result.removed} 个恢复期间新增文件`, `, removed ${result.removed} files introduced by restore`)
      : "";
    const failedPart = result.deleteFailed?.length
      ? tr(`；${result.deleteFailed.length} 个删除失败（${result.deleteFailed.map((f) => f.path).slice(0, 3).join("、")}${result.deleteFailed.length > 3 ? "…" : ""}）`, `; ${result.deleteFailed.length} deletions failed`)
      : "";
    setNotice({
      text: tr(
        `已回滚到快照时刻（恢复 ${result.restored} 个文件${deletedPart}；回滚前状态已另存快照${failedPart}）`,
        `Rolled back (restored ${result.restored} files${deletedPart}; prior state snapshotted${failedPart})`,
      ),
      error: false,
    });
    setRollback(null);
    setDeleteIntroduced(false);
    // 956 号走查反挖（P9）：回滚（保守或完全）同样改变书目面——同上广播失效。
    invalidateApiPaths(["/api/v1/books"]);
    void loadSnapshots();
  });

  const labels = snapshotLabels(snapshots);

  return (
    <div className="rounded-lg border border-border/60 px-5 py-4 mb-8 flex items-center justify-between gap-4 flex-wrap">
      <div>
        <div className="text-sm font-medium">{tr("备份与恢复", "Backup & restore")}</div>
        <div className="text-xs text-muted-foreground mt-0.5">
          {tr("全量导出项目数据为 tar.gz；恢复前自动快照原文件。", "Export all project data as tar.gz; auto-snapshot before restore.")}
        </div>
      </div>
      <div className="flex items-center gap-2 shrink-0 flex-wrap">
        <button
          onClick={doExport}
          className="px-4 py-2 text-xs rounded-lg bg-primary text-primary-foreground font-medium hover:bg-primary/90 transition-colors"
        >
          {tr("导出备份", "Export backup")}
        </button>
        <label className="px-4 py-2 text-xs rounded-lg border border-border font-medium cursor-pointer hover:bg-primary/10 transition-colors">
          {tr("导入恢复", "Import restore")}
          <input
            ref={fileRef}
            type="file"
            accept=".tar.gz,.tgz,application/gzip"
            className="hidden"
            disabled={busy}
            onChange={(event) => {
              const file = event.target.files?.[0];
              if (file) void onFile(file);
              event.target.value = "";
            }}
          />
        </label>
      </div>
      {preview && pendingFile && (
        <div className="w-full space-y-1 border-t border-border/40 pt-2">
          <p className="text-xs">
            {tr(`包内 ${preview.fileCount} 个文件：新增 ${preview.newFiles}，将覆盖 ${preview.overwriting.length}`, `${preview.fileCount} files: +${preview.newFiles} new, ${preview.overwriting.length} overwrite`)}
          </p>
          {preview.overwriting.length > 0 && (
            <p className="text-[11px] text-amber-600">
              {tr("将覆盖：", "Overwriting: ")}
              {preview.overwriting.slice(0, 6).join("、")}
              {preview.overwriting.length > 6 ? "…" : ""}
            </p>
          )}
          <button
            onClick={() => void confirmImport()}
            disabled={busy}
            className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-50"
          >
            {tr("确认恢复", "Confirm restore")}
          </button>
        </div>
      )}
      {snapshots.length > 0 && (
        <div className="w-full space-y-1 border-t border-border/40 pt-2">
          <p className="text-xs font-medium">{tr("可回滚快照（恢复前自动留档）", "Rollback snapshots (auto-saved before restore)")}</p>
          {snapshots.map((snapshot, index) => (
            <div key={snapshot.name} className="flex items-center justify-between gap-3 text-xs">
              <span className="text-muted-foreground truncate">
                {labels[index]} · {snapshot.size}
                {tr(" 字节", " bytes")}
              </span>
              <button
                onClick={() => void previewRollback(snapshot.name, labels[index]!)}
                disabled={busy}
                className="px-3 py-1 shrink-0 rounded-md border border-border font-medium cursor-pointer hover:bg-primary/10 transition-colors disabled:opacity-50"
              >
                {tr("回滚到此快照", "Roll back to this snapshot")}
              </button>
            </div>
          ))}
        </div>
      )}
      {rollback && (
        <div className="w-full space-y-1 border-t border-border/40 pt-2">
          <p className="text-xs">
            {/* 955 号：预览点名所选快照（多行快照时确认前须知回滚到哪一份）。 */}
            {tr(`回滚该快照（${rollback.label}）：将覆盖 ${rollback.preview.overwriting.length} 个文件`, `Rollback (${rollback.label}): ${rollback.preview.overwriting.length} files to overwrite`)}
          </p>
          {rollback.preview.overwriting.length > 0 && (
            <p className="text-[11px] text-amber-600">
              {tr("将覆盖：", "Overwriting: ")}
              {rollback.preview.overwriting.slice(0, 6).join("、")}
              {rollback.preview.overwriting.length > 6 ? "…" : ""}
            </p>
          )}
          {rollback.preview.introduced && rollback.preview.introduced.length > 0 && (
            <div className="space-y-1">
              <p className="text-[11px] text-muted-foreground">
                {tr(
                  `恢复期间新增 ${rollback.preview.introduced.length} 个文件（勾选可一并删除，完全回到快照时刻）：`,
                  `${rollback.preview.introduced.length} files introduced by restore (check to also remove them for a full rollback): `,
                )}
                {rollback.preview.introduced.slice(0, 6).join("、")}
                {rollback.preview.introduced.length > 6 ? "…" : ""}
              </p>
              <label className="flex items-center gap-1.5 text-[11px] cursor-pointer">
                <input
                  type="checkbox"
                  checked={deleteIntroduced}
                  disabled={busy}
                  onChange={(event) => setDeleteIntroduced(event.target.checked)}
                />
                {tr("同时删除这些新增文件（删除前自动留档）", "Also remove these files (auto-snapshotted first)")}
              </label>
            </div>
          )}
          <button
            onClick={() => void confirmRollback()}
            disabled={busy}
            className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-50"
          >
            {tr("确认回滚", "Confirm rollback")}
          </button>
        </div>
      )}
      {notice && (
        <p className={`w-full text-[11px] ${notice.error ? "text-destructive" : "text-emerald-600"}`}>{notice.text}</p>
      )}
    </div>
  );
}
