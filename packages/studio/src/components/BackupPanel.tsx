import { useCallback, useEffect, useRef, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface Preview {
  readonly fileCount: number;
  readonly overwriting: ReadonlyArray<string>;
  readonly newFiles: number;
}

interface SnapshotItem {
  readonly name: string;
  readonly size: number;
}

/** 954 号：从快照名（pre-restore-<millis>.tar）解析本地时间展示；解析失败回退原名。 */
function snapshotLabel(name: string): string {
  const millis = Number(name.match(/^pre-restore-(\d+)\.tar$/)?.[1] ?? NaN);
  return Number.isSafeInteger(millis) ? new Date(millis).toLocaleString() : name;
}

/**
 * R18/386 号：备份与恢复卡（Dashboard 挂载）。
 * 导出=全量 tar.gz 下载；导入=两段式（预览 → 确认写回），恢复前自动快照
 * 至 backups/pre-restore-*.tar（回滚保险）。954 号：快照可列举可回滚——
 * 回滚走服务端同一两段式（预览覆盖面 → 确认），恢复/回滚全程有保险。
 */
export function BackupPanel() {
  const [notice, setNotice] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const [snapshots, setSnapshots] = useState<ReadonlyArray<SnapshotItem>>([]);
  const [rollback, setRollback] = useState<{ name: string; preview: Preview } | null>(null);

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

  const doExport = () => {
    window.open(`${location.origin}/api/v1/backup/export`, "_blank");
  };

  const onFile = async (file: File) => {
    setPendingFile(file);
    try {
      const raw = await file.arrayBuffer();
      // 443 号：服务端线格式为 {preview:{...}}（TS/Rust 一致）——面板此前按
      // 平铺读取，预览数字全为 undefined，导入预览从未真正可读。
      const result = await fetchJson<{ preview: Preview }>("/backup/import", {
        method: "POST",
        body: raw as unknown as BodyInit,
      });
      setPreview(result.preview);
    } catch (error) {
      setNotice(String(error));
    }
  };

  const confirmImport = async () => {
    if (!pendingFile) return;
    try {
      const raw = await pendingFile.arrayBuffer();
      const result = await fetchJson<{ ok: boolean; restored: number }>(
        "/backup/import?confirm=1",
        { method: "POST", body: raw as unknown as BodyInit },
      );
      setNotice(tr(`已恢复 ${result.restored} 个文件（原文件快照在 backups/）`, `Restored ${result.restored} files (snapshot in backups/)`));
      setPreview(null);
      setPendingFile(null);
      void loadSnapshots();
    } catch (error) {
      setNotice(String(error));
    }
  };

  const previewRollback = async (name: string) => {
    try {
      const result = await fetchJson<{ preview: Preview }>(
        `/backup/rollback?name=${encodeURIComponent(name)}`,
        { method: "POST" },
      );
      setRollback({ name, preview: result.preview });
      setNotice("");
    } catch (error) {
      setNotice(String(error));
    }
  };

  const confirmRollback = async () => {
    if (!rollback) return;
    try {
      const result = await fetchJson<{ ok: boolean; restored: number }>(
        `/backup/rollback?name=${encodeURIComponent(rollback.name)}&confirm=1`,
        { method: "POST" },
      );
      setNotice(tr(`已回滚到快照时刻（恢复 ${result.restored} 个文件；回滚前状态已另存快照）`, `Rolled back (restored ${result.restored} files; prior state snapshotted)`));
      setRollback(null);
      void loadSnapshots();
    } catch (error) {
      setNotice(String(error));
    }
  };

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
            onClick={confirmImport}
            className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground"
          >
            {tr("确认恢复", "Confirm restore")}
          </button>
        </div>
      )}
      {snapshots.length > 0 && (
        <div className="w-full space-y-1 border-t border-border/40 pt-2">
          <p className="text-xs font-medium">{tr("可回滚快照（恢复前自动留档）", "Rollback snapshots (auto-saved before restore)")}</p>
          {snapshots.map((snapshot) => (
            <div key={snapshot.name} className="flex items-center justify-between gap-3 text-xs">
              <span className="text-muted-foreground truncate">
                {snapshotLabel(snapshot.name)} · {snapshot.size}
                {tr(" 字节", " bytes")}
              </span>
              <button
                onClick={() => void previewRollback(snapshot.name)}
                className="px-3 py-1 shrink-0 rounded-md border border-border font-medium cursor-pointer hover:bg-primary/10 transition-colors"
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
            {tr(`回滚该快照：将覆盖 ${rollback.preview.overwriting.length} 个文件`, `Rollback: ${rollback.preview.overwriting.length} files to overwrite`)}
          </p>
          {rollback.preview.overwriting.length > 0 && (
            <p className="text-[11px] text-amber-600">
              {tr("将覆盖：", "Overwriting: ")}
              {rollback.preview.overwriting.slice(0, 6).join("、")}
              {rollback.preview.overwriting.length > 6 ? "…" : ""}
            </p>
          )}
          <button
            onClick={confirmRollback}
            className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground"
          >
            {tr("确认回滚", "Confirm rollback")}
          </button>
        </div>
      )}
      {notice && <p className="w-full text-[11px] text-emerald-600">{notice}</p>}
    </div>
  );
}
