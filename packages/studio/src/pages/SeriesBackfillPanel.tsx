import { useEffect, useMemo, useState } from "react";
import { fetchJson, postApi } from "../hooks/use-api";
import type { TFunction } from "../hooks/use-i18n";
import { buildBackfillDiff, type BackfillItem } from "./series-backfill-diff";

interface BookOption {
  readonly id: string;
  readonly title: string;
}

interface Draft {
  readonly bookId: string;
  readonly sourceBookId: string;
  readonly updatedAt: string;
  readonly items: ReadonlyArray<BackfillItem>;
}

/**
 * 系列书回填向导（184 号 C3-b/c，对标 Sudowrite Story Bible）。
 * 三步：选源/目标书 → 抽取（LLM 摘要为结构化设定集）→ 预览勾选后写入
 * `story/series_backfill.md`（独立文件，不确认则零写入）。
 */
export function SeriesBackfillPanel({ t }: { t: TFunction }) {
  const { data } = useBookOptions();
  const [source, setSource] = useState("");
  const [target, setTarget] = useState("");
  const [extracting, setExtracting] = useState(false);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [applying, setApplying] = useState(false);
  const [status, setStatus] = useState<{ kind: "ok" | "error"; text: string } | null>(null);
  const [existing, setExisting] = useState<string | null>(null);
  const [draftMeta, setDraftMeta] = useState<{ sourceBookId: string; updatedAt: string } | null>(null);

  // 目标书变化 → 拉现有 series_backfill.md（186 号 diff 预览数据源）。
  useEffect(() => {
    if (!target) {
      setExisting(null);
      return;
    }
    void fetchJson<{ content: string | null }>(`/books/${target}/series-backfill/existing`)
      .then((res) => setExisting(res.content))
      .catch(() => setExisting(null));
  }, [target]);

  const categoryGrouped = useMemo(() => {
    if (!draft) return [];
    const groups = new Map<string, BackfillItem[]>();
    for (const item of draft.items) {
      const key = ["worldview", "character", "plot", "style"].includes(item.category) ? item.category : "other";
      const bucket = groups.get(key) ?? [];
      bucket.push(item);
      groups.set(key, bucket);
    }
    return [...groups.entries()];
  }, [draft]);

  const handleExtract = async (): Promise<void> => {
    setExtracting(true);
    setStatus(null);
    setDraft(null);
    setSelected(new Set());
    try {
      const res = await postApi<{ draft: Draft }>(`/books/${target}/series-backfill/extract`, {
        sourceBookId: source,
      });
      setDraft(res.draft);
      setDraftMeta({ sourceBookId: res.draft.sourceBookId, updatedAt: res.draft.updatedAt });
      setSelected(new Set(res.draft.items.map((item) => item.id)));
    } catch (e) {
      setStatus({ kind: "error", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setExtracting(false);
    }
  };

  const handleApply = async (): Promise<void> => {
    setApplying(true);
    setStatus(null);
    try {
      const res = await postApi<{ applied: number }>(`/books/${target}/series-backfill/apply`, {
        itemIds: [...selected],
      });
      setStatus({ kind: "ok", text: t("backfill.applied").replace("{n}", String(res.applied)) });
      // apply 为整体覆盖——成功后立即重拉服务端文件作为 diff 新基线
      // （否则 existing 停留在写入前，diff 会把覆盖语义显示错）。
      try {
        const ex = await fetchJson<{ content: string | null }>(`/books/${target}/series-backfill/existing`);
        setExisting(ex.content);
      } catch { /* 保留旧基线 */ }
    } catch (e) {
      setStatus({ kind: "error", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setApplying(false);
    }
  };

  // diff 预览（186 号）：现有内容 × 勾选后的将写入内容（apply 为整体覆盖，
  // 未勾选条目的移除在此显性化）。
  const diffLines = useMemo(() => {
    if (!draft || !draftMeta) return [];
    const selectedItems = draft.items.filter((item) => selected.has(item.id));
    return buildBackfillDiff(existing, draftMeta.sourceBookId, draftMeta.updatedAt, selectedItems);
  }, [draft, draftMeta, selected, existing]);
  const removedCount = diffLines.filter((line) => line.kind === "removed").length;
  const addedCount = diffLines.filter((line) => line.kind === "added").length;

  const toggle = (id: string): void => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <div className="space-y-4" data-page="series-backfill">
      <p className="text-sm text-muted-foreground">{t("backfill.hint")}</p>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        <select
          value={source}
          onChange={(e) => setSource(e.target.value)}
          className="px-3 py-2 rounded-lg bg-secondary/30 border border-border text-sm"
          data-slot="backfill-source"
        >
          <option value="">{t("backfill.selectSource")}</option>
          {data?.books.map((b) => <option key={b.id} value={b.id}>{b.title}</option>)}
        </select>
        <select
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          className="px-3 py-2 rounded-lg bg-secondary/30 border border-border text-sm"
          data-slot="backfill-target"
        >
          <option value="">{t("backfill.selectTarget")}</option>
          {data?.books.map((b) => <option key={b.id} value={b.id}>{b.title}</option>)}
        </select>
      </div>

      <button
        onClick={() => void handleExtract()}
        disabled={extracting || !source || !target || source === target}
        className="px-4 py-2 text-sm rounded-lg bg-primary text-primary-foreground font-bold disabled:opacity-30"
        data-slot="backfill-extract"
      >
        {extracting ? t("backfill.extracting") : t("backfill.extract")}
      </button>

      {status && (
        <div className={`text-sm ${status.kind === "ok" ? "text-emerald-600 dark:text-emerald-400" : "text-red-500"}`} data-slot="backfill-status">
          {status.text}
        </div>
      )}

      {draft && draft.items.length > 0 && (
        <div className="space-y-3" data-slot="backfill-preview">
          <div className="flex items-center justify-between">
            <span className="text-sm font-bold">{t("backfill.preview").replace("{n}", String(draft.items.length))}</span>
            <button
              onClick={() => setSelected((prev) => (prev.size === draft.items.length ? new Set() : new Set(draft.items.map((item) => item.id))))}
              className="text-xs text-muted-foreground hover:text-foreground underline underline-offset-2"
            >
              {selected.size === draft.items.length ? t("backfill.deselectAll") : t("backfill.selectAll")}
            </button>
          </div>
          {categoryGrouped.map(([category, items]) => (
            <div key={category} className="space-y-2">
              <div className="text-xs font-bold uppercase tracking-wider text-muted-foreground">{category}</div>
              {items.map((item) => (
                <label
                  key={item.id}
                  className="flex gap-3 items-start p-3 rounded-lg border border-border/50 hover:border-primary/30 transition-colors cursor-pointer"
                  data-slot="backfill-item"
                >
                  <input
                    type="checkbox"
                    checked={selected.has(item.id)}
                    onChange={() => toggle(item.id)}
                    className="mt-0.5"
                  />
                  <span>
                    <span className="text-sm font-semibold">{item.title}</span>
                    <span className="block text-xs text-muted-foreground mt-0.5">{item.content}</span>
                  </span>
                </label>
              ))}
            </div>
          ))}
          {diffLines.length > 0 && (
            <div className="rounded-lg border border-border/50 overflow-hidden" data-slot="backfill-diff">
              <div className="px-3 py-2 text-xs font-bold text-muted-foreground bg-secondary/30 flex items-center gap-3">
                <span>{t("backfill.diffTitle")}</span>
                <span className="text-emerald-600 dark:text-emerald-400">+{addedCount}</span>
                <span className="text-red-500">-{removedCount}</span>
              </div>
              <div className="max-h-64 overflow-y-auto font-mono text-[11px] leading-5">
                {diffLines.map((line: { kind: string; text: string }, idx: number) => (
                  <div
                    key={idx}
                    className={`px-3 whitespace-pre-wrap break-all ${
                      line.kind === "added" ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                      : line.kind === "removed" ? "bg-red-500/10 text-red-500 line-through decoration-red-400/60"
                      : "text-muted-foreground"
                    }`}
                    data-diff-kind={line.kind}
                  >
                    {line.kind === "added" ? `+ ${line.text}` : line.kind === "removed" ? `- ${line.text}` : `  ${line.text}`}
                  </div>
                ))}
              </div>
            </div>
          )}
          <button
            onClick={() => void handleApply()}
            disabled={applying || selected.size === 0}
            className="px-4 py-2 text-sm rounded-lg bg-primary text-primary-foreground font-bold disabled:opacity-30"
            data-slot="backfill-apply"
          >
            {applying ? t("backfill.applying") : t("backfill.apply").replace("{n}", String(selected.size))}
          </button>
          <p className="text-xs text-muted-foreground">{t("backfill.applyHint")}</p>
          <p className="text-xs text-muted-foreground">{t("backfill.diffHint")}</p>
        </div>
      )}

    </div>
  );
}

/** 书列表单独获取（避免与宿主页耦合）。 */
function useBookOptions(): { data: { books: ReadonlyArray<BookOption> } | null } {
  const [data, setData] = useState<{ books: ReadonlyArray<BookOption> } | null>(null);
  useEffect(() => {
    void fetchJson<{ books: ReadonlyArray<BookOption> }>("/books")
      .then(setData)
      .catch(() => undefined);
  }, []);
  return { data };
}
