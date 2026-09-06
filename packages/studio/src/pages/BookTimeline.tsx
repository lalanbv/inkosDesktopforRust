import { useEffect, useMemo, useState } from "react";
import { useApi, putApi, postApi } from "../hooks/use-api";
import { ArrowLeft, Pencil } from "lucide-react";
import type { Theme } from "../hooks/use-theme";
import type { TFunction } from "../hooks/use-i18n";
import type { Nav } from "../lib/nav";
import { writeTaskSessionId } from "../hooks/use-book-activity";
import {
  buildTimelineAfterCellEdit,
  buildTimelineAfterAddPlotline,
  buildTimelineAfterRenamePlotline,
  buildTimelineAfterRemovePlotline,
  initializeTimelineFromChapters,
  type TimelineDoc,
} from "./timeline-edit";

interface ChapterMeta {
  readonly number: number;
  readonly title: string;
  readonly status: string;
  readonly wordCount: number;
}

interface BookData {
  readonly book: {
    readonly id: string;
    readonly title: string;
  };
  readonly chapters: ReadonlyArray<ChapterMeta>;
  readonly nextChapter: number;
}

/** story/timeline.json（181 号 C4-b）的 GET 响应载荷。 */
interface TimelinePayload {
  readonly timeline: {
    readonly version: 1;
    readonly bookId: string;
    readonly updatedAt: string;
    readonly plotlines: ReadonlyArray<{
      readonly id: string;
      readonly name: string;
      readonly cells: ReadonlyArray<{
        readonly chapter: number;
        readonly title?: string;
        readonly note?: string;
      }>;
    }>;
  } | null;
}

/** 网格单元格的统一展示形态（兜底章 / timeline 节拍 / 空占位共用）。 */
interface TimelineGridCell {
  readonly number: number;
  readonly title: string;
  readonly subtitle: string;
  readonly tone: Tone;
  /** timeline 线的可编辑格（兜底格点击仍是「去阅读」）。 */
  readonly editable: boolean;
  /** 空占位格：该线该章暂无节拍，点击 = 新建节拍。 */
  readonly placeholder: boolean;
}

type Tone = "done" | "review" | "failed" | "wip" | "imported" | "planned" | "empty";

function statusTone(status: string): Tone {
  if (status === "approved" || status === "published") return "done";
  if (status === "ready-for-review" || status === "audit-passed") return "review";
  if (status === "audit-failed" || status === "rejected" || status === "state-degraded") return "failed";
  if (status === "imported") return "imported";
  return "wip";
}

const TONE_CLASS: Record<Tone, string> = {
  done: "bg-emerald-500/10 border-emerald-500/40 text-emerald-600 dark:text-emerald-400",
  review: "bg-amber-500/10 border-amber-500/40 text-amber-600 dark:text-amber-400",
  failed: "bg-red-500/10 border-red-500/40 text-red-600 dark:text-red-400",
  wip: "bg-sky-500/10 border-sky-500/40 text-sky-600 dark:text-sky-400",
  imported: "bg-muted border-border text-muted-foreground",
  planned: "bg-violet-500/10 border-violet-500/40 text-violet-600 dark:text-violet-400",
  empty: "bg-transparent border-dashed border-border/60 text-muted-foreground/60",
};

interface EditTarget {
  readonly plotlineId: string;
  readonly plotlineName: string;
  readonly chapter: number;
  readonly title: string;
  readonly note: string;
}

/**
 * 编辑弹窗（C4-c）。受控组件：输入值由父级持有，提交回传 title/note。
 * 独立导出便于静态渲染测试。
 */
export function TimelineEditDialog({ bookId, target, saving, writing, onSubmit, onWriteFromBeat, onCancel, nav, t }: {
  bookId: string;
  target: EditTarget;
  saving: boolean;
  writing: boolean;
  onSubmit: (patch: { title: string; note: string }) => void;
  onWriteFromBeat: () => void;
  onCancel: () => void;
  nav: Nav;
  t: TFunction;
}) {
  const [title, setTitle] = useState(target.title);
  const [note, setNote] = useState(target.note);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" data-testid="timeline-edit-dialog">
      <div className="w-[380px] rounded-2xl bg-background border border-border shadow-2xl p-6">
        <div className="text-xs text-muted-foreground mb-1">
          {target.plotlineName} · {t("timeline.chapter").replace("{n}", String(target.chapter))}
        </div>
        <h2 className="text-lg font-bold mb-4">{t("timeline.editBeat")}</h2>
        <label className="block text-xs font-medium text-muted-foreground mb-1">{t("timeline.beatTitle")}</label>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          maxLength={200}
          className="w-full mb-3 px-3 py-2 text-sm rounded-lg border border-border bg-transparent focus:outline-none focus:ring-1 focus:ring-primary/40"
          data-slot="timeline-beat-title"
        />
        <label className="block text-xs font-medium text-muted-foreground mb-1">{t("timeline.beatNote")}</label>
        <textarea
          value={note}
          onChange={(e) => setNote(e.target.value)}
          maxLength={2000}
          rows={3}
          className="w-full mb-4 px-3 py-2 text-sm rounded-lg border border-border bg-transparent focus:outline-none focus:ring-1 focus:ring-primary/40 resize-none"
          data-slot="timeline-beat-note"
        />
        <div className="flex items-center justify-between gap-2">
          <span className="flex items-center gap-3">
            <button
              onClick={() => nav.toChapter(bookId, target.chapter)}
              className="text-xs text-muted-foreground hover:text-foreground underline underline-offset-2"
            >
              {t("timeline.openChapter")}
            </button>
            <button
              onClick={onWriteFromBeat}
              disabled={writing || saving}
              className="text-xs font-bold text-primary hover:underline underline-offset-2 disabled:opacity-50"
              data-slot="timeline-write-from-beat"
            >
              {writing ? t("dash.writing") : t("timeline.writeFromBeat")}
            </button>
          </span>
          <div className="flex gap-2">
            <button onClick={onCancel} className="px-4 py-2 text-sm rounded-lg border border-border hover:bg-secondary/60 transition-colors">
              {t("timeline.cancel")}
            </button>
            <button
              onClick={() => onSubmit({ title, note })}
              disabled={saving}
              className="px-4 py-2 text-sm font-bold rounded-lg bg-primary text-primary-foreground hover:scale-105 active:scale-95 transition-transform disabled:opacity-50"
              data-slot="timeline-save"
            >
              {saving ? t("timeline.saving") : t("timeline.save")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * 书籍时间线（180 号 W-C4-a、181 号 C4-b 多线、182 号 C4-c 编辑回写）。
 *
 * 数据源：story/timeline.json 多线优先；缺省回退单线只读兜底（点「编辑时间线」
 * 初始化后进入可编辑态）。保存走 PUT 端点 + 乐观更新，失败回滚并提示。
 */
export function BookTimeline({ bookId, nav, theme, t }: {
  bookId: string;
  nav: Nav;
  theme: Theme;
  t: TFunction;
}) {
  void theme; // 样式走语义色 token，无需调色板
  const { data, loading, error } = useApi<BookData>(`/books/${bookId}`);
  const { data: timelineRes, loading: timelineLoading, refetch: refetchTimeline } = useApi<TimelinePayload>(`/books/${bookId}/timeline`);

  // 乐观更新：保存期间用本地 doc 渲染，成功后 refetch 对齐服务端，失败回滚。
  const [optimistic, setOptimistic] = useState<TimelineDoc | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [editTarget, setEditTarget] = useState<EditTarget | null>(null);
  const [addingLine, setAddingLine] = useState(false);
  const [newLineName, setNewLineName] = useState("");
  const [renamingLineId, setRenamingLineId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [removingLineId, setRemovingLineId] = useState<string | null>(null);
  const [writeStarted, setWriteStarted] = useState(false);

  const chapters = useMemo(() => {
    const list = [...(data?.chapters ?? [])];
    list.sort((a, b) => a.number - b.number);
    return list;
  }, [data]);

  const doc: TimelineDoc | null = optimistic ?? timelineRes?.timeline ?? null;

  const saveDoc = async (next: TimelineDoc): Promise<void> => {
    setSaving(true);
    setSaveError(null);
    setOptimistic(next);
    try {
      await putApi(`/books/${bookId}/timeline`, next);
      setOptimistic(null);
      refetchTimeline();
    } catch (e) {
      setOptimistic(null); // 失败回滚：放弃本地 doc，回服务端真值
      setSaveError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const startEditableTimeline = async (): Promise<void> => {
    const next = initializeTimelineFromChapters(bookId, chapters, t("timeline.mainPlotline"));
    await saveDoc(next);
  };

  // 多线（timeline 存在且至少一条线有节拍）优先；否则单线只读兜底。
  const grid = useMemo(() => {
    const timelinePlotlines = doc?.plotlines ?? [];
    if (timelinePlotlines.length > 0) {
      // 列轴 = 各线 cells 章号并集；全空（新建线尚未填节拍）时回退章节表。
      const chapterNumbers = new Set<number>();
      for (const line of timelinePlotlines) {
        for (const cell of line.cells) chapterNumbers.add(cell.chapter);
      }
      if (chapterNumbers.size === 0) {
        for (const ch of chapters) chapterNumbers.add(ch.number);
      }
      const columns = [...chapterNumbers].sort((a, b) => a - b);
      const fromTimeline = timelinePlotlines;
      return {
        columns,
        lines: fromTimeline.map((line) => ({
          key: line.id,
          label: line.name,
          cells: columns.map<TimelineGridCell>((number) => {
            const cell = line.cells.find((c) => c.chapter === number);
            if (!cell) {
              return {
                number,
                title: t("timeline.chapter").replace("{n}", String(number)),
                subtitle: t("timeline.addBeat"),
                tone: "empty" as Tone,
                editable: true,
                placeholder: true,
              };
            }
            return {
              number,
              title: cell.title ?? t("timeline.chapter").replace("{n}", String(number)),
              subtitle: cell.note ?? "",
              tone: "planned" as Tone,
              editable: true,
              placeholder: false,
            };
          }),
        })),
      };
    }
    return {
      columns: chapters.map((ch) => ch.number),
      lines: [{
        key: "main",
        label: t("timeline.mainPlotline"),
        cells: chapters.map<TimelineGridCell>((ch) => ({
          number: ch.number,
          title: ch.title,
          subtitle: ch.wordCount.toLocaleString(),
          tone: statusTone(ch.status),
          editable: false,
          placeholder: false,
        })),
      }],
    };
  }, [doc, chapters, t]);

  const totalWords = chapters.reduce((sum, ch) => sum + (ch.wordCount || 0), 0);
  // 双源就绪才渲染网格：timeline 未到时先按兜底渲染会让快速点击误跳阅读器。
  const hasGrid = !loading && !timelineLoading && !error && (chapters.length > 0 || grid.columns.length > 0);
  const editableMode = grid.lines.some((line) => line.cells.some((cell) => cell.editable));

  return (
    <div className="max-w-6xl mx-auto px-6 py-10 md:px-12 fade-in" data-page="book-timeline">
      <div className="flex items-center justify-between gap-3 mb-6">
        <button
          onClick={() => nav.toBook(bookId)}
          className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
        >
          <ArrowLeft size={16} />
          <span>{data?.book.title ?? bookId}</span>
        </button>
        {!loading && !error && !editableMode && chapters.length > 0 && (
          <button
            onClick={() => void startEditableTimeline()}
            disabled={saving}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-bold rounded-lg border border-border hover:bg-secondary/60 transition-colors disabled:opacity-50"
            data-slot="timeline-init"
          >
            <Pencil size={12} />
            {t("timeline.editTimeline")}
          </button>
        )}
      </div>

      <h1 className="text-2xl font-bold mb-1">{t("timeline.title")}</h1>
      {!loading && !error && (
        <p className="text-sm text-muted-foreground mb-6">
          {t("timeline.stats").replace("{chapters}", String(chapters.length)).replace("{words}", totalWords.toLocaleString())}
        </p>
      )}

      {loading && <div className="text-sm text-muted-foreground" data-loading="skeleton">{t("common.loading")}</div>}
      {error && <div className="text-sm text-red-500">{String(error)}</div>}
      {saveError && (
        <div className="mb-3 text-sm text-red-500" data-slot="timeline-save-error">
          {t("timeline.saveFailed").replace("{message}", saveError)}
        </div>
      )}
      {writeStarted && (
        <div className="mb-3 text-sm text-emerald-600 dark:text-emerald-400" data-slot="timeline-write-started">
          {t("timeline.writeStarted")}
        </div>
      )}

      {!loading && !error && chapters.length === 0 && grid.columns.length === 0 && (
        <div className="text-sm text-muted-foreground border border-border/50 rounded-xl px-5 py-8 text-center">
          {t("timeline.empty")}
        </div>
      )}

      {hasGrid && (
        <div className="overflow-x-auto border border-border/50 rounded-xl">
          <div className="min-w-max">
            {/* 列头：章号 */}
            <div className="flex border-b border-border/40 bg-secondary/30 sticky top-0">
              <div className="w-36 shrink-0 px-4 py-2 text-xs font-semibold text-muted-foreground border-r border-border/40">
                {t("timeline.plotline")}
              </div>
              {grid.columns.map((number) => (
                <div key={number} className="w-36 shrink-0 px-3 py-2 text-xs font-semibold text-muted-foreground border-r border-border/30 last:border-r-0 text-center">
                  {t("timeline.chapter").replace("{n}", String(number))}
                </div>
              ))}
            </div>
            {/* 情节线行 */}
            {grid.lines.map((line) => (
              <div key={line.key} className="flex">
                <div className="w-36 shrink-0 px-4 py-3 text-sm font-medium border-r border-border/40 flex items-center justify-between gap-1">
                  {renamingLineId === line.key ? (
                    <input
                      value={renameValue}
                      onChange={(e) => setRenameValue(e.target.value)}
                      maxLength={120}
                      autoFocus
                      className="w-full px-1.5 py-1 text-xs rounded border border-border bg-transparent focus:outline-none focus:ring-1 focus:ring-primary/40"
                      data-slot="timeline-rename-input"
                    />
                  ) : (
                    <span className="truncate">{line.label}</span>
                  )}
                  {editableMode && renamingLineId === line.key ? (
                    <span className="flex flex-col gap-0.5 shrink-0">
                      <button
                        onClick={async () => {
                          const base = optimistic ?? doc;
                          if (!base || !renameValue.trim()) return;
                          await saveDoc(buildTimelineAfterRenamePlotline(base, line.key, renameValue));
                          setRenamingLineId(null);
                        }}
                        className="text-[10px] text-emerald-600 dark:text-emerald-400 hover:underline"
                        data-slot="timeline-rename-confirm"
                      >
                        {t("timeline.confirmAdd")}
                      </button>
                      <button onClick={() => setRenamingLineId(null)} className="text-[10px] text-muted-foreground hover:underline">
                        {t("timeline.cancel")}
                      </button>
                    </span>
                  ) : editableMode ? (
                    <span className="flex flex-col gap-0.5 shrink-0 opacity-40 hover:opacity-100 transition-opacity">
                      <button
                        onClick={() => { setRenamingLineId(line.key); setRenameValue(line.label); }}
                        title={t("timeline.renamePlotline")}
                        className="text-[10px] text-muted-foreground hover:text-primary"
                        data-slot="timeline-rename"
                      >
                        {t("timeline.rename")}
                      </button>
                      <button
                        onClick={async () => {
                          if (removingLineId !== line.key) {
                            setRemovingLineId(line.key);
                            return;
                          }
                          const base = optimistic ?? doc;
                          if (!base) return;
                          await saveDoc(buildTimelineAfterRemovePlotline(base, line.key));
                          setRemovingLineId(null);
                        }}
                        title={t("timeline.removePlotline")}
                        className={`text-[10px] hover:underline ${removingLineId === line.key ? "text-red-500 font-bold" : "text-muted-foreground hover:text-red-500"}`}
                        data-slot="timeline-remove"
                      >
                        {removingLineId === line.key ? t("timeline.removeConfirm") : t("timeline.remove")}
                      </button>
                    </span>
                  ) : null}
                </div>
                {line.cells.map((cell) => (
                  <button
                    key={cell.number}
                    onClick={() => {
                      if (!cell.editable) {
                        nav.toChapter(bookId, cell.number);
                        return;
                      }
                      setEditTarget({
                        plotlineId: line.key,
                        plotlineName: line.label,
                        chapter: cell.number,
                        title: cell.placeholder ? "" : cell.title,
                        note: cell.placeholder ? "" : cell.subtitle,
                      });
                    }}
                    title={`${cell.title} · ${cell.subtitle}`}
                    className={`w-36 shrink-0 px-3 py-3 border-r border-border/30 last:border-r-0 border-b-0 text-left transition-transform ${cell.editable ? "hover:scale-[1.03]" : ""} ${TONE_CLASS[cell.tone]}`}
                    data-timeline-cell={cell.number}
                    data-tone={cell.tone}
                    data-editable={cell.editable ? "true" : "false"}
                  >
                    <div className="text-xs font-semibold truncate">{cell.title}</div>
                    <div className="text-[11px] opacity-70 mt-0.5 truncate">{cell.subtitle}</div>
                  </button>
                ))}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 新增情节线（可编辑态） */}
      {editableMode && !addingLine && (
        <button
          onClick={() => setAddingLine(true)}
          className="mt-4 text-xs text-muted-foreground hover:text-primary underline underline-offset-2"
          data-slot="timeline-add-line"
        >
          {t("timeline.addPlotline")}
        </button>
      )}
      {editableMode && addingLine && (
        <div className="mt-4 flex items-center gap-2" data-slot="timeline-add-line-form">
          <input
            value={newLineName}
            onChange={(e) => setNewLineName(e.target.value)}
            maxLength={120}
            placeholder={t("timeline.plotlineName")}
            className="px-3 py-1.5 text-sm rounded-lg border border-border bg-transparent focus:outline-none focus:ring-1 focus:ring-primary/40"
            data-slot="timeline-add-line-name"
          />
          <button
            onClick={async () => {
              const base = optimistic ?? doc;
              if (!base || !newLineName.trim() || !base.plotlines) return;
              await saveDoc(buildTimelineAfterAddPlotline(base, newLineName));
              setAddingLine(false);
              setNewLineName("");
            }}
            disabled={saving}
            className="px-3 py-1.5 text-xs font-bold rounded-lg bg-primary text-primary-foreground disabled:opacity-50"
            data-slot="timeline-add-line-confirm"
          >
            {t("timeline.confirmAdd")}
          </button>
          <button
            onClick={() => { setAddingLine(false); setNewLineName(""); }}
            className="px-3 py-1.5 text-xs rounded-lg border border-border hover:bg-secondary/60"
          >
            {t("timeline.cancel")}
          </button>
        </div>
      )}

      {editTarget && (
        <TimelineEditDialog
          bookId={bookId}
          target={editTarget}
          saving={saving}
          writing={writeStarted}
          nav={nav}
          t={t}
          onCancel={() => { setEditTarget(null); setWriteStarted(false); }}
          onSubmit={async (patch) => {
            const base = optimistic ?? doc;
            if (!base) return;
            await saveDoc(buildTimelineAfterCellEdit(base, editTarget.plotlineId, editTarget.chapter, patch));
            setEditTarget(null);
          }}
          onWriteFromBeat={() => {
            // 按此节拍写下一章：beat 文本作为规划输入（context）注入 write-next，
            // 引擎侧非空 context 替换自动 plan；带伪会话 sessionId 激活检查点+可停止。
            const beatParts = [editTarget.title, editTarget.note].filter((part) => part.trim());
            const context = `时间线节拍【${editTarget.plotlineName} · 第${editTarget.chapter}章】${beatParts.join("：")}`;
            void postApi(`/books/${bookId}/write-next`, {
              context,
              sessionId: writeTaskSessionId(bookId),
            }).then(() => setWriteStarted(true)).catch(() => setWriteStarted(false));
          }}
        />
      )}
    </div>
  );
}
