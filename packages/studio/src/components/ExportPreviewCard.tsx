import { useMemo, useState } from "react";
import { tr } from "../lib/app-language";

interface ChapterRow {
  readonly number: number;
  readonly title?: string;
  readonly status?: string;
}

type ExportFormatChoice = "txt" | "md" | "epub" | "md-zip";

const FORMATS: ReadonlyArray<{ id: ExportFormatChoice; labelZh: string; labelEn: string }> = [
  { id: "md-zip", labelZh: "Markdown 打包（zip）", labelEn: "Markdown bundle (zip)" },
  { id: "md", labelZh: "Markdown 单文件", labelEn: "Markdown (single)" },
  { id: "txt", labelZh: "纯文本", labelEn: "Plain text" },
  { id: "epub", labelZh: "EPUB", labelEn: "EPUB" },
];

/**
 * R50/1645 号：导出预览面（Scrivener Compile 语义子集）——格式选择 +
 * 章节勾选子集（缺省全选）+ GET /export 回流下载（chapters CSV 参数）。
 * approved-only 语义由既有 approvedOnly 参数承接。
 */
export function ExportPreviewCard({
  bookId,
  chapters,
}: {
  bookId: string;
  chapters: ReadonlyArray<ChapterRow>;
}) {
  const [format, setFormat] = useState<ExportFormatChoice>("md-zip");
  const [selected, setSelected] = useState<ReadonlySet<number>>(
    () => new Set(chapters.map((chapter) => chapter.number)),
  );
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");

  const ordered = useMemo(
    () => [...chapters].sort((a, b) => a.number - b.number),
    [chapters],
  );
  const selectedCount = selected.size;

  const toggle = (number: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(number)) next.delete(number);
      else next.add(number);
      return next;
    });
  };

  const exportBook = async () => {
    if (selectedCount === 0) {
      setNotice(tr("请至少勾选一章。", "Select at least one chapter."));
      return;
    }
    setBusy(true);
    setNotice("");
    try {
      const query = new URLSearchParams({ format });
      query.set("chapters", [...selected].sort((a, b) => a - b).join(","));
      const response = await fetch(`/api/v1/books/${encodeURIComponent(bookId)}/export?${query.toString()}`);
      if (!response.ok) {
        setNotice(tr("导出失败", "Export failed"));
        return;
      }
      const blob = await response.blob();
      const disposition = response.headers.get("Content-Disposition") ?? "";
      const starMatch = /filename\*=UTF-8''([^;]+)/i.exec(disposition);
      const plainMatch = /filename="?([^";]+)"?/i.exec(disposition);
      const fileName = starMatch
        ? decodeURIComponent(starMatch[1]!)
        : (plainMatch?.[1] ?? `${bookId}-export`);
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = fileName;
      anchor.click();
      URL.revokeObjectURL(url);
      setNotice(tr("已下载", "Downloaded"));
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("导出", "Export")}
      </h3>
      <div className="flex flex-wrap gap-1" role="radiogroup" aria-label={tr("导出格式", "Export format")}>
        {FORMATS.map((entry) => (
          <button
            key={entry.id}
            role="radio"
            aria-checked={format === entry.id}
            onClick={() => setFormat(entry.id)}
            className={`rounded-full px-2.5 py-0.5 text-xs transition-colors ${
              format === entry.id
                ? "bg-primary text-primary-foreground"
                : "bg-secondary text-muted-foreground hover:text-foreground"
            }`}
          >
            {tr(entry.labelZh, entry.labelEn)}
          </button>
        ))}
      </div>
      <div className="max-h-48 overflow-y-auto rounded-lg border border-border/40 divide-y divide-border/30">
        {ordered.map((chapter) => (
          <label
            key={chapter.number}
            className="flex items-center gap-2 px-2 py-1 text-xs hover:bg-secondary/40 cursor-pointer"
          >
            <input
              type="checkbox"
              checked={selected.has(chapter.number)}
              onChange={() => toggle(chapter.number)}
              data-testid={`export-chapter-${chapter.number}`}
            />
            <span className="font-mono text-muted-foreground">{String(chapter.number).padStart(2, "0")}</span>
            <span className="truncate">
              {chapter.title?.trim() ? chapter.title : tr(`第 ${chapter.number} 章`, `Chapter ${chapter.number}`)}
            </span>
          </label>
        ))}
      </div>
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-muted-foreground">
          {tr(`已选 ${selectedCount}/${ordered.length} 章`, `${selectedCount}/${ordered.length} selected`)}
        </span>
        <button
          onClick={() => void exportBook()}
          disabled={busy}
          className="rounded-lg bg-primary px-3 py-1.5 text-xs font-bold text-primary-foreground disabled:opacity-50"
          data-testid="export-run"
        >
          {busy ? tr("导出中…", "Exporting…") : tr("导出下载", "Export")}
        </button>
      </div>
      {notice ? <p className="text-xs text-muted-foreground">{notice}</p> : null}
    </div>
  );
}
