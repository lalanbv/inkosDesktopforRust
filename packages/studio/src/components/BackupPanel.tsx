import { useRef, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface Preview {
  readonly fileCount: number;
  readonly overwriting: ReadonlyArray<string>;
  readonly newFiles: number;
}

/**
 * R18/386 号：备份与恢复卡（Dashboard 挂载）。
 * 导出=全量 tar.gz 下载；导入=两段式（预览 → 确认写回），恢复前自动快照
 * 至 backups/pre-restore-*.tar（回滚保险）。
 */
export function BackupPanel() {
  const [notice, setNotice] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const doExport = () => {
    window.open(`${location.origin}/api/v1/backup/export`, "_blank");
  };

  const onFile = async (file: File) => {
    setPendingFile(file);
    try {
      const raw = await file.arrayBuffer();
      const result = await fetchJson<Preview>("/backup/import", {
        method: "POST",
        body: raw as unknown as BodyInit,
      });
      setPreview(result);
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
      {notice && <p className="w-full text-[11px] text-emerald-600">{notice}</p>}
    </div>
  );
}
