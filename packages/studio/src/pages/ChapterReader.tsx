import { useEffect, useRef, useState } from "react";
import { findQueryMatch, queryTokens } from "@actalk/inkos-core/retrieval/global-search";
import { takeChapterFocus } from "../lib/chapter-focus";
import { fetchJson, postApi, useApi } from "../hooks/use-api";
import type { Theme } from "../hooks/use-theme";
import type { TFunction } from "../hooks/use-i18n";
import { useColors } from "../hooks/use-colors";
import { Skeleton } from "../components/ui/skeleton";
import { EntityHighlightOverlay, EDITOR_TYPOGRAPHY_CLASS } from "../components/EntityHighlightOverlay";
import { SkeletonParagraphs } from "../components/skeletons";
import { usePreferencesStore } from "../store/preferences";
import { usePanelWidth } from "../hooks/use-panel-width";
import { ChapterWorkspacePanel } from "../components/ChapterWorkspacePanel";
import {
  ChevronLeft,
  ChevronRight,
  Check,
  X,
  Columns2,
  List,
  RotateCcw,
  BookOpen,
  Hash,
  Type,
  Clock,
  Pencil,
  Save,
  Eye,
} from "lucide-react";

interface ChapterData {
  readonly chapterNumber: number;
  readonly filename: string;
  readonly content: string;
}

interface BookData {
  readonly book: {
    readonly title: string;
  };
}

interface Nav {
  toBook: (id: string) => void;
  toDashboard: () => void;
}

export function ChapterReader({ bookId, chapterNumber, nav, theme, t }: {
  bookId: string;
  chapterNumber: number;
  nav: Nav;
  theme: Theme;
  t: TFunction;
}) {
  const c = useColors(theme);
  // P4-1 打字机模式：专注开启时正文段落降透明，点击段落聚焦
  const focusMode = usePreferencesStore((state) => state.focusMode);
  const [activeParagraph, setActiveParagraph] = useState<number | null>(null);
  // 读写对照分屏（UI 优化方案目标布局「可分屏 ≤2 组」）：右侧只读对照章，
  // 宽度拖拽钳制 260~720 + 记忆（P3-2 use-panel-width 参数化复用，贴右缘）。
  const [splitOpen, setSplitOpen] = useState(false);
  const [splitChapter, setSplitChapter] = useState<number | null>(
    chapterNumber > 1 ? chapterNumber - 1 : null,
  );
  const split = usePanelWidth({
    min: 260,
    max: 720,
    defaultWidth: 420,
    storageKey: "inkos:studio:reader-split-width",
    side: "right",
  });
  const { data, loading, error, refetch } = useApi<ChapterData>(
    `/books/${bookId}/chapters/${chapterNumber}`,
  );
  // 198 号：面包屑显示书名（此前暴露 bookId，与顶栏不一致）；加载前回退 id。
  const { data: bookData } = useApi<BookData>(`/books/${bookId}`);
  const [editing, setEditing] = useState(false);
  // R52/1656 号：实体高亮 overlay——Codex 卡拉取（编辑态一次）+ 全局开关
  // （localStorage 默认开；书级覆盖挂观察）。
  const [entityCards, setEntityCards] = useState<ReadonlyArray<{
    name: string; aliases?: ReadonlyArray<string>; kind?: string; summary?: string; facts?: ReadonlyArray<string>;
  }> | null>(null);
  const [entityHighlightOn] = useState(
    () => (globalThis.localStorage?.getItem("inkos.entityHighlight") ?? "on") === "on",
  );
  const [editContent, setEditContent] = useState("");
  const [saving, setSaving] = useState(false);
  const [splitting, setSplitting] = useState(false);
  // 1651 号：壳内 alert 静默——拆分/保存提示改页内可见文本。
  const [readerNotice, setReaderNotice] = useState("");
  const [workspaceRevision, setWorkspaceRevision] = useState(0);
  // R48b/1643 号：编辑态光标拆分（R48a split 事务消费面）——光标 UTF-16
  // 偏移 = textarea.selectionStart（splitUtf16 契约同语义）。
  const editAreaRef = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => {
    if (!editing || entityCards !== null || !bookId) return;
    void fetchJson<{ cards: ReadonlyArray<{ name: string; aliases?: string[]; kind?: string; summary?: string; facts?: string[] }> }>(
      `/books/${encodeURIComponent(bookId)}/codex`,
    )
      .then((payload) => setEntityCards(payload.cards ?? []))
      .catch(() => setEntityCards([]));
  }, [editing, entityCards, bookId]);

  const handleSplitAtCursor = async () => {
    const offset = editAreaRef.current?.selectionStart ?? -1;
    if (offset <= 0 || offset >= editContent.length) {
      // 1651 号：壳内 alert 静默——边界提示改页内文本。
      setReaderNotice(t("reader.splitAtCursorBoundary"));
      return;
    }
    setSplitting(true);
    try {
      // 1646 号审查修复：拆分事务读盘上版本——编辑未保存即拆=拆旧稿+编辑
      // 丢失（所见非所得）。先落盘当前编辑内容再拆，保证拆分对象=所见。
      await fetchJson(`/books/${bookId}/chapters/${chapterNumber}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ content: editContent }),
      });
      await postApi(`/books/${bookId}/chapters/${chapterNumber}/split`, { offset });
      setEditing(false);
      nav.toBook(bookId);
    } catch (e) {
      setReaderNotice(e instanceof Error ? e.message : "Split failed");
    } finally {
      setSplitting(false);
    }
  };

  // R47/1640 号：⌘P 正文命中一次性焦点——数据到位后定位首个命中段落，
  // scrollIntoView 居中 + 闪烁高亮（takeChapterFocus 取走即清，防重放）。
  const [focusQuery, setFocusQuery] = useState<string | null>(null);
  const [focusParagraph, setFocusParagraph] = useState<number | null>(null);
  const focusTimersRef = useRef<number[]>([]);
  useEffect(() => {
    const pending = takeChapterFocus(bookId, chapterNumber);
    if (pending) setFocusQuery(pending);
  }, [bookId, chapterNumber]);
  useEffect(() => {
    if (!data || !focusQuery || editing) return;
    const lines = data.content.split("\n");
    const titleLine = lines.find((l) => l.startsWith("# "));
    const body = lines.filter((l) => l !== titleLine).join("\n").trim();
    const paragraphsForFocus = body.split(/\n\n+/).filter(Boolean);
    const tokens = queryTokens(focusQuery);
    const hitIndex = paragraphsForFocus.findIndex(
      (para) => findQueryMatch(para, tokens) !== null,
    );
    if (hitIndex < 0) return;
    setFocusParagraph(hitIndex);
    const frame = window.requestAnimationFrame(() => {
      document
        .querySelector('[data-search-hit="true"]')
        ?.scrollIntoView({ behavior: "smooth", block: "center" });
    });
    const clearTimer = window.setTimeout(() => setFocusParagraph(null), 2600);
    focusTimersRef.current.push(frame, clearTimer);
    return () => {
      for (const timer of focusTimersRef.current) {
        window.cancelAnimationFrame(timer);
        window.clearTimeout(timer);
      }
      focusTimersRef.current = [];
    };
  }, [data, focusQuery, editing]);

  const handleStartEdit = () => {
    if (!data) return;
    setEditContent(data.content);
    setEditing(true);
  };

  const handleCancelEdit = () => {
    setEditing(false);
    setEditContent("");
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await fetchJson(`/books/${bookId}/chapters/${chapterNumber}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ content: editContent }),
      });
      setEditing(false);
      refetch();
      setWorkspaceRevision((revision) => revision + 1);
    } catch (e) {
      setReaderNotice(e instanceof Error ? e.message : "Save failed");
    } finally {
      setSaving(false);
    }
  };

  if (loading && !data) return (
    <div data-loading="skeleton" className="space-y-10 py-4">
      <div className="space-y-3">
        <Skeleton className="h-9 w-2/5" />
        <Skeleton className="h-4 w-24" />
      </div>
      <SkeletonParagraphs count={4} />
    </div>
  );

  if (error) return <div className="text-destructive p-8 bg-destructive/5 rounded-xl border border-destructive/20">Error: {error}</div>;
  if (!data) return null;

  // Split markdown content into title and body
  const lines = data.content.split("\n");
  const titleLine = lines.find((l) => l.startsWith("# "));
  const title = titleLine?.replace(/^#\s*/, "") ?? `Chapter ${chapterNumber}`;
  const body = lines
    .filter((l) => l !== titleLine)
    .join("\n")
    .trim();

  const paragraphs = body.split(/\n\n+/).filter(Boolean);

  return (
    <div className="w-full space-y-10 fade-in">
      {/* Navigation & Actions */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-6">
        <nav className="flex items-center gap-2 text-[13px] font-medium text-muted-foreground">
          <button
            onClick={nav.toDashboard}
            className="hover:text-primary transition-colors flex items-center gap-1"
          >
            {t("bread.books")}
          </button>
          <span className="text-border">/</span>
          <button
            onClick={() => nav.toBook(bookId)}
            className="hover:text-primary transition-colors truncate max-w-[120px]"
          >
            {bookData?.book.title ?? bookId}
          </button>
          <span className="text-border">/</span>
          <span className="text-foreground flex items-center gap-1">
            <Hash size={12} />
            {chapterNumber}
          </span>
        </nav>

        <div className="flex gap-2">
          <button
            onClick={() => nav.toBook(bookId)}
            className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary text-muted-foreground rounded-xl hover:text-foreground hover:bg-secondary/80 transition-all border border-border/50"
          >
            <List size={14} />
            {t("reader.backToList")}
          </button>

          {/* 1651 号：页内提示条（壳内 alert 静默）。 */}
          {readerNotice ? (
            <span role="status" data-testid="reader-notice" className="text-xs text-destructive self-center">{readerNotice}</span>
          ) : null}

          {/* Edit / Preview toggle */}
          {editing ? (
            <>
              <button
                onClick={() => void handleSplitAtCursor()}
                disabled={splitting}
                className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary text-muted-foreground rounded-xl hover:text-foreground transition-all border border-border/50 disabled:opacity-50"
                title={t("reader.splitAtCursor")}
                data-testid="reader-split-at-cursor"
              >
                {t("reader.splitAtCursor")}
              </button>
              <button
                onClick={handleSave}
                disabled={saving}
                className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-primary text-primary-foreground rounded-xl hover:scale-105 active:scale-95 transition-all shadow-sm disabled:opacity-50"
              >
                {saving ? <div className="w-3.5 h-3.5 border-2 border-primary-foreground/20 border-t-primary-foreground rounded-full animate-spin" /> : <Save size={14} />}
                {saving ? t("book.saving") : t("book.save")}
              </button>
              <button
                onClick={handleCancelEdit}
                className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary text-muted-foreground rounded-xl hover:text-foreground transition-all border border-border/50"
              >
                <Eye size={14} />
                {t("reader.preview")}
              </button>
            </>
          ) : (
            <button
              onClick={handleStartEdit}
              className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary text-muted-foreground rounded-xl hover:text-primary hover:bg-primary/10 transition-all border border-border/50"
            >
              <Pencil size={14} />
              {t("reader.edit")}
            </button>
          )}

          {/* 读写对照分屏开关（UI 优化方案目标布局：可分屏 ≤2 组） */}
          <button
            onClick={() => setSplitOpen((v) => !v)}
            data-testid="reader-split-toggle"
            className={`flex items-center gap-2 px-4 py-2 text-xs font-bold rounded-xl transition-all border border-border/50 ${
              splitOpen
                ? "bg-primary/10 text-primary"
                : "bg-secondary text-muted-foreground hover:text-primary hover:bg-primary/10"
            }`}
          >
            <Columns2 size={14} />
            {splitOpen ? t("reader.splitClose") : t("reader.split")}
          </button>
        </div>
      </div>

      <ChapterWorkspacePanel
        key={`${chapterNumber}-${workspaceRevision}`}
        bookId={bookId}
        chapterNumber={chapterNumber}
        t={t}
        onChapterChanged={refetch}
        onChapterDeleted={() => nav.toBook(bookId)}
      />

      {/* Manuscript Sheet */}
      {/* 分屏组：左=主稿（读/写），右=只读对照章（拖宽/换章/关闭） */}
      <div className={splitOpen ? "flex items-stretch gap-0" : undefined}>
        <div className={splitOpen ? "flex-1 min-w-0" : undefined}>
      <div className="paper-sheet rounded-2xl p-8 md:p-16 lg:p-24 shadow-2xl shadow-primary/5 min-h-[80vh] relative overflow-hidden">
        {/* Physical Paper Details */}
        <div className="absolute top-0 left-8 w-px h-full bg-primary/5 hidden md:block" />
        <div className="absolute top-0 right-8 w-px h-full bg-primary/5 hidden md:block" />

        <header className="mb-16 text-center">
          <div className="flex items-center justify-center gap-2 text-muted-foreground/30 mb-8 select-none">
            <div className="h-px w-12 bg-border/40" />
            <BookOpen size={20} />
            <div className="h-px w-12 bg-border/40" />
          </div>
          <h1 className="text-4xl md:text-5xl font-serif font-medium italic text-foreground tracking-tight leading-tight">
            {title}
          </h1>
          <div className="mt-8 flex items-center justify-center gap-4 text-[10px] font-bold uppercase tracking-[0.2em] text-muted-foreground/60">
            <span>{t("reader.manuscriptPage")}</span>
            <span className="text-border">·</span>
            <span>{chapterNumber.toString().padStart(2, '0')}</span>
          </div>
        </header>

        {editing ? (
          <div className="relative" data-editor-overlay-host="true">
            {entityHighlightOn && entityCards !== null ? (
              <EntityHighlightOverlay text={editContent} cards={entityCards} textareaRef={editAreaRef} />
            ) : null}
            <textarea
              ref={editAreaRef}
              value={editContent}
              onChange={(e) => setEditContent(e.target.value)}
              className={`relative z-10 w-full min-h-[60vh] bg-transparent ${EDITOR_TYPOGRAPHY_CLASS} text-foreground/90 focus:outline-none resize-none border border-border/30 rounded-lg focus:border-primary/40 focus:ring-2 focus:ring-primary/10 transition-all`}
              autoFocus
            />
          </div>
        ) : (
          <article className="prose prose-zinc dark:prose-invert max-w-none">
            {paragraphs.map((para, i) => (
              <p
                key={i}
                onClick={focusMode ? () => setActiveParagraph(i) : undefined}
                data-search-hit={i === focusParagraph ? "true" : undefined}
                className={`font-serif text-lg md:text-xl leading-[1.8] text-foreground/90 mb-8 first-letter:text-2xl first-letter:font-bold first-letter:text-primary/40 ${focusMode && activeParagraph !== null && i !== activeParagraph ? "typewriter-dim" : ""} ${i === focusParagraph ? "bg-primary/10 ring-1 ring-primary/30 rounded-lg px-4 py-3 transition-colors" : ""}`}
              >
                {para}
              </p>
            ))}
          </article>
        )}

        <footer className="mt-24 pt-12 border-t border-border/20 flex flex-col items-center gap-6 text-center">
          <div className="flex items-center gap-4 text-xs font-medium text-muted-foreground">
             <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-secondary/50">
               <Type size={14} className="text-primary/60" />
               <span>{body.length.toLocaleString()} {t("reader.characters")}</span>
             </div>
             <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-secondary/50">
               <Clock size={14} className="text-primary/60" />
               <span>{Math.ceil(body.length / 500)} {t("reader.minRead")}</span>
             </div>
          </div>
          <p className="text-[10px] uppercase tracking-widest text-muted-foreground/40 font-bold">{t("reader.endOfChapter")}</p>
        </footer>
      </div>
        </div>

        {splitOpen ? (
          <>
            {/* 分隔条：拖拽调宽（260~720 钳制 + 记忆），双击复位 420 */}
            <div
              role="separator"
              aria-orientation="vertical"
              data-testid="reader-split-divider"
              onPointerDown={split.beginResize}
              onPointerMove={split.handleResizeMove}
              onPointerUp={split.endResize}
              onDoubleClick={split.resetWidth}
              className="w-1 shrink-0 cursor-col-resize bg-transparent hover:bg-primary/30 active:bg-primary/50 transition-colors"
            />
            <SplitChapterPane
              bookId={bookId}
              chapter={splitChapter}
              onChapterChange={setSplitChapter}
              onClose={() => setSplitOpen(false)}
              width={split.width}
              t={t}
            />
          </>
        ) : null}
      </div>

      {/* Footer Navigation */}
      <div className="flex justify-between items-center py-8">
        {chapterNumber > 1 ? (
          <button
            onClick={() => nav.toBook(bookId)}
            className="flex items-center gap-2 text-sm font-bold text-muted-foreground hover:text-primary transition-all group"
          >
            <RotateCcw size={16} className="group-hover:-rotate-45 transition-transform" />
            {t("reader.chapterList")}
          </button>
        ) : (
          <div />
        )}
      </div>
    </div>
  );
}

/**
 * 只读对照栏（读写对照分屏的右半）：独立拉取对照章，标题行含
 * 上一章/下一章/直接跳章与关闭；正文只读渲染（对照用途，无编辑面）。
 */
function SplitChapterPane({
  bookId,
  chapter,
  onChapterChange,
  onClose,
  width,
  t,
}: {
  bookId: string;
  chapter: number | null;
  onChapterChange: (next: number | null) => void;
  onClose: () => void;
  width: number;
  t: TFunction;
}) {
  const [jump, setJump] = useState("");
  // 空串 = 未选对照章：buildApiUrl 返回 null → hook 安静空态（不请求）。
  const { data, loading, error } = useApi<ChapterData>(
    chapter === null ? "" : `/books/${bookId}/chapters/${chapter}`,
  );

  const commitJump = () => {
    const n = Number.parseInt(jump, 10);
    if (Number.isInteger(n) && n >= 1) onChapterChange(n);
    setJump("");
  };

  const lines = data?.content.split("\n") ?? [];
  const titleLine = lines.find((l) => l.startsWith("# "));
  const title = titleLine?.replace(/^#\s*/, "") ?? (chapter !== null ? `Chapter ${chapter}` : "");
  const body = lines.filter((l) => l !== titleLine).join("\n").trim();
  const paragraphs = body.split(/\n\n+/).filter(Boolean);

  return (
    <aside
      data-testid="reader-split-pane"
      style={{ width }}
      className="shrink-0 overflow-hidden rounded-2xl border border-border/40 bg-secondary/20 p-6 flex flex-col min-h-[80vh]"
    >
      <div className="flex items-center justify-between gap-2 pb-3 border-b border-border/30">
        <div className="flex items-center gap-1 min-w-0">
          <button
            onClick={() => chapter !== null && chapter > 1 && onChapterChange(chapter - 1)}
            disabled={chapter === null || chapter <= 1}
            className="p-1.5 rounded-lg hover:bg-secondary text-muted-foreground disabled:opacity-30 transition-colors"
            aria-label={t("reader.splitPrev")}
          >
            <ChevronLeft size={14} />
          </button>
          <span className="text-xs font-bold text-foreground truncate">
            {chapter === null ? t("reader.compareChapter") : `${t("reader.compareChapter")} · ${chapter}`}
          </span>
          <button
            onClick={() => chapter !== null && onChapterChange(chapter + 1)}
            disabled={chapter === null}
            className="p-1.5 rounded-lg hover:bg-secondary text-muted-foreground disabled:opacity-30 transition-colors"
            aria-label={t("reader.splitNext")}
          >
            <ChevronRight size={14} />
          </button>
        </div>
        <div className="flex items-center gap-1">
          <input
            value={jump}
            onChange={(e) => setJump(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && commitJump()}
            onBlur={commitJump}
            inputMode="numeric"
            placeholder="#"
            data-testid="reader-split-jump"
            className="w-12 px-2 py-1 text-xs text-center bg-background/60 border border-border/40 rounded-lg focus:border-primary/40 focus:outline-none"
          />
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg hover:bg-secondary text-muted-foreground transition-colors"
            aria-label={t("reader.splitClose")}
          >
            <X size={14} />
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto pt-4" data-testid="reader-split-body">
        {chapter === null ? (
          <p className="text-xs text-muted-foreground/70 leading-6 pt-8 text-center">
            {t("reader.splitPlaceholder")}
          </p>
        ) : loading && !data ? (
          <SkeletonParagraphs count={4} />
        ) : error ? (
          <p className="text-xs text-destructive pt-8 text-center">
            {t("reader.splitLoadFailed")}: {error}
          </p>
        ) : (
          <>
            <h2 className="font-serif text-base font-medium italic text-foreground/80 mb-6 text-center">
              {title}
            </h2>
            <article className="prose prose-zinc dark:prose-invert max-w-none">
              {paragraphs.map((para, i) => (
                <p key={i} className="font-serif text-sm leading-[1.9] text-foreground/75 mb-5">
                  {para}
                </p>
              ))}
            </article>
          </>
        )}
      </div>
    </aside>
  );
}
