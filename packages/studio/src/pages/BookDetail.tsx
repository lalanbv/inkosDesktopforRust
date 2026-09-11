import { fetchJson, useApi, postApi } from "../hooks/use-api";
import { useEffect, useMemo, useState } from "react";
import { QualityDebtsCard } from "../components/QualityDebtsCard";
import { PromiseTimelineCard } from "../components/PromiseTimelineCard";
import { StyleBindingCard } from "../components/StyleBindingCard";
import { RosterCandidatesCard } from "../components/RosterCandidatesCard";
import type { Theme } from "../hooks/use-theme";
import type { TFunction } from "../hooks/use-i18n";
import type { SSEMessage } from "../hooks/use-sse";
import { useColors } from "../hooks/use-colors";
import { deriveBookActivity, shouldRefetchBookView, writeTaskSessionId } from "../hooks/use-book-activity";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  ChevronLeft,
  Zap,
  FileText,
  CheckCheck,
  BarChart2,
  Download,
  Search,
  Wand2,
  Eye,
  Database,
  Check,
  X,
  ShieldCheck,
  RotateCcw,
  RefreshCw,
  Sparkles,
  Trash2,
  Save,
  Hand,
  Settings2,
  Waypoints,
  Square,
  AlertTriangle,
  BookOpen
} from "lucide-react";
import { genreLabel } from "../lib/genre-labels";

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
    readonly genre: string;
    readonly status: string;
    readonly chapterWordCount: number;
    readonly targetChapters?: number;
    readonly language?: string;
    readonly fanficMode?: string;
  };
  readonly chapters: ReadonlyArray<ChapterMeta>;
  readonly nextChapter: number;
}

type ReviseMode = "spot-fix" | "polish" | "rewrite" | "rework" | "anti-detect";
type ExportFormat = "txt" | "md" | "epub";
type BookStatus = "active" | "paused" | "outlining" | "completed" | "dropped";

interface Nav {
  toDashboard: () => void;
  toChapter: (bookId: string, num: number) => void;
  toAnalytics: (bookId: string) => void;
  toTruth: (bookId: string) => void;
  toBookTimeline: (bookId: string) => void;
}

function translateChapterStatus(status: string, t: TFunction): string {
  const map: Record<string, () => string> = {
    "ready-for-review": () => t("chapter.readyForReview"),
    "approved": () => t("chapter.approved"),
    "drafted": () => t("chapter.drafted"),
    "needs-revision": () => t("chapter.needsRevision"),
    "imported": () => t("chapter.imported"),
    "audit-failed": () => t("chapter.auditFailed"),
    // 207 号：后端 ChapterStatus 全集补齐——此前 fallthrough 显示原始枚举
    // （state-degraded 以「STATE-DEGRADED」大写暴露）。
    "state-degraded": () => t("chapter.stateDegraded"),
    "card-generated": () => t("chapter.cardGenerated"),
    "drafting": () => t("chapter.drafting"),
    "auditing": () => t("chapter.auditing"),
    "audit-passed": () => t("chapter.auditPassed"),
    "revising": () => t("chapter.revising"),
    "rejected": () => t("chapter.rejected"),
    "published": () => t("chapter.published"),
  };
  return map[status]?.() ?? status;
}

const STATUS_CONFIG: Record<string, { color: string; icon: React.ReactNode }> = {
  "ready-for-review": { color: "text-amber-500 bg-amber-500/10", icon: <Eye size={12} /> },
  approved: { color: "text-emerald-500 bg-emerald-500/10", icon: <Check size={12} /> },
  drafted: { color: "text-muted-foreground bg-muted/20", icon: <FileText size={12} /> },
  "needs-revision": { color: "text-destructive bg-destructive/10", icon: <RotateCcw size={12} /> },
  imported: { color: "text-blue-500 bg-blue-500/10", icon: <Download size={12} /> },
  "state-degraded": { color: "text-orange-500 bg-orange-500/10", icon: <AlertTriangle size={12} /> },
  "audit-passed": { color: "text-emerald-500 bg-emerald-500/10", icon: <ShieldCheck size={12} /> },
  rejected: { color: "text-destructive bg-destructive/10", icon: <X size={12} /> },
  published: { color: "text-violet-500 bg-violet-500/10", icon: <BookOpen size={12} /> },
};

export function BookDetail({
  bookId,
  nav,
  theme,
  t,
  sse,
}: {
  bookId: string;
  nav: Nav;
  theme: Theme;
  t: TFunction;
  sse: { messages: ReadonlyArray<SSEMessage> };
}) {
  const c = useColors(theme);
  const { data, loading, error, refetch } = useApi<BookData>(`/books/${bookId}`);
  const [writeRequestPending, setWriteRequestPending] = useState(false);
  const [draftRequestPending, setDraftRequestPending] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false);
  const [rewritingChapters, setRewritingChapters] = useState<ReadonlyArray<number>>([]);
  const [revisingChapters, setRevisingChapters] = useState<ReadonlyArray<number>>([]);
  const [syncingChapters, setSyncingChapters] = useState<ReadonlyArray<number>>([]);
  const [savingSettings, setSavingSettings] = useState(false);
  const [settingsWordCount, setSettingsWordCount] = useState<number | null>(null);
  const [settingsTargetChapters, setSettingsTargetChapters] = useState<number | null>(null);
  const [settingsStatus, setSettingsStatus] = useState<BookStatus | null>(null);
  const [exportFormat, setExportFormat] = useState<ExportFormat>("txt");
  const [exportApprovedOnly, setExportApprovedOnly] = useState(false);
  const [bookActionPending, setBookActionPending] = useState<string | null>(null);
  // Auto (pipeline self-reviews) vs manual (write the draft and stop; you
  // run audit / revise / approve as checkpoint actions). This is scoped to
  // the current book, with project-level mode as the inherited default.
  const [reviewMode, setReviewMode] = useState<"auto" | "manual">("auto");
  useEffect(() => {
    void fetchJson<{ mode?: string }>(`/books/${encodeURIComponent(bookId)}/chapter-review-mode`)
      .then((r) => setReviewMode(r.mode === "manual" ? "manual" : "auto"))
      .catch(() => undefined);
  }, [bookId]);
  // 189 号：时间线节拍自动沉淀（书籍级开关，默认关）。
  const [autoBeats, setAutoBeats] = useState(false);
  useEffect(() => {
    void fetchJson<{ enabled?: boolean }>(`/books/${encodeURIComponent(bookId)}/timeline-auto-beats`)
      .then((r) => setAutoBeats(r.enabled === true))
      .catch(() => undefined);
  }, [bookId]);
  // 176 号：重启恢复——挂载时按伪会话订阅一次快照（引擎/服务重启后
  // write:start/complete 事件已丢，只有检查点快照知道任务仍在跑）。收到
  // running 快照 → 恢复 writing 态；终态快照 → 静默刷新视图后关闭；无快照
  // （5s 内只有 ping）→ 超时关闭。
  useEffect(() => {
    const source = new EventSource(
      `/api/v1/events?sessionId=${encodeURIComponent(writeTaskSessionId(bookId))}`,
    );
    let settled = false;
    const close = () => {
      if (settled) return;
      settled = true;
      source.close();
    };
    source.addEventListener("task:snapshot", (event) => {
      try {
        const snapshot = JSON.parse((event as MessageEvent).data) as {
          execution?: { status?: string };
        };
        if (snapshot.execution?.status === "running" || snapshot.execution?.status === "processing") {
          setWriteRequestPending(true);
        } else {
          refetch();
        }
      } catch {
        // 坏快照忽略——维持默认态。
      }
      close();
    });
    const timer = window.setTimeout(close, 5000);
    return () => {
      window.clearTimeout(timer);
      close();
    };
    // refetch 是 useApi 的稳定回调；bookId 变化即重订。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bookId]);
  const activity = useMemo(() => deriveBookActivity(sse.messages, bookId), [bookId, sse.messages]);
  const writing = writeRequestPending || activity.writing;
  const drafting = draftRequestPending || activity.drafting;
  const latestPersistedChapter = data ? data.nextChapter - 1 : 0;

  useEffect(() => {
    const recent = sse.messages.at(-1);
    if (!recent) return;

    const data = recent.data as { bookId?: string } | null;
    if (data?.bookId !== bookId) return;

    if (recent.event === "write:start") {
      setWriteRequestPending(false);
      return;
    }

    if (recent.event === "draft:start") {
      setDraftRequestPending(false);
      return;
    }

    if (shouldRefetchBookView(recent, bookId)) {
      setWriteRequestPending(false);
      setDraftRequestPending(false);
      refetch();
    }
  }, [bookId, refetch, sse.messages]);

  const handleWriteNext = async () => {
    setWriteRequestPending(true);
    try {
      // 176 号：sessionId 锚定检查点 + 可停止（引擎 174/175 号能力在此激活）。
      await postApi(`/books/${bookId}/write-next`, {
        sessionId: writeTaskSessionId(bookId),
      });
    } catch (e) {
      setWriteRequestPending(false);
      alert(e instanceof Error ? e.message : "Failed");
    }
  };

  const handleStopWriteNext = async () => {
    try {
      // Rust 引擎实停（175 号）；Node 回退端诚实返回 aborted:false，任务
      // 继续跑完——两端都以 write:error / write:complete 收敛 UI 态。
      await postApi(`/sessions/${encodeURIComponent(writeTaskSessionId(bookId))}/abort`);
    } catch {
      // 停止失败不打断——SSE 终态事件仍会清 writing。
    }
  };

  const handleDraft = async () => {
    setDraftRequestPending(true);
    try {
      await postApi(`/books/${bookId}/draft`);
    } catch (e) {
      setDraftRequestPending(false);
      alert(e instanceof Error ? e.message : "Failed");
    }
  };

  const handleToggleReviewMode = async () => {
    const next = reviewMode === "manual" ? "auto" : "manual";
    setReviewMode(next);
    try {
      await fetchJson(`/books/${encodeURIComponent(bookId)}/chapter-review-mode`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ mode: next }),
      });
    } catch {
      setReviewMode(reviewMode); // revert on failure
    }
  };

  const handleToggleAutoBeats = async () => {
    const next = !autoBeats;
    setAutoBeats(next);
    try {
      await fetchJson(`/books/${encodeURIComponent(bookId)}/timeline-auto-beats`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ enabled: next }),
      });
    } catch {
      setAutoBeats(!next); // revert on failure
    }
  };

  const handleDeleteBook = async () => {
    setConfirmDeleteOpen(false);
    setDeleting(true);
    try {
      const res = await fetch(`/api/v1/books/${bookId}`, { method: "DELETE" });
      if (!res.ok) {
        const json = await res.json().catch(() => ({}));
        throw new Error((json as { error?: string }).error ?? `${res.status}`);
      }
      nav.toDashboard();
    } catch (e) {
      alert(e instanceof Error ? e.message : "Delete failed");
    } finally {
      setDeleting(false);
    }
  };

  const handleRewrite = async (chapterNum: number) => {
    const brief = window.prompt(
      data?.book.language === "en"
        ? "Optional rewrite brief for this run only. Leave blank to use existing focus."
        : "可选：输入这次重写要遵循的补充想法。留空则沿用现有 focus。",
      "",
    );
    if (brief === null) return;
    setRewritingChapters((prev) => [...prev, chapterNum]);
    try {
      await fetchJson(`/books/${bookId}/rewrite/${chapterNum}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ brief: brief.trim() || undefined }),
      });
      refetch();
    } catch (e) {
      alert(e instanceof Error ? e.message : "Rewrite failed");
    } finally {
      setRewritingChapters((prev) => prev.filter((n) => n !== chapterNum));
    }
  };

  const handleRevise = async (chapterNum: number, mode: ReviseMode) => {
    const brief = window.prompt(
      data?.book.language === "en"
        ? "Optional revise brief for this run only. Leave blank to use existing focus."
        : "可选：输入这次修订要遵循的补充想法。留空则沿用现有 focus。",
      "",
    );
    if (brief === null) return;
    setRevisingChapters((prev) => [...prev, chapterNum]);
    try {
      await fetchJson(`/books/${bookId}/revise/${chapterNum}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ mode, brief: brief.trim() || undefined }),
      });
      refetch();
    } catch (e) {
      alert(e instanceof Error ? e.message : "Revision failed");
    } finally {
      setRevisingChapters((prev) => prev.filter((n) => n !== chapterNum));
    }
  };

  const handleSync = async (chapterNum: number) => {
    const brief = window.prompt(
      data?.book.language === "en"
        ? "Optional sync brief for interpreting the edited chapter body. Leave blank to sync directly from the text."
        : "可选：输入这次同步时要遵循的补充说明。留空则直接按正文同步。",
      "",
    );
    if (brief === null) return;
    setSyncingChapters((prev) => [...prev, chapterNum]);
    try {
      await fetchJson(`/books/${bookId}/resync/${chapterNum}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ brief: brief.trim() || undefined }),
      });
      refetch();
    } catch (e) {
      alert(e instanceof Error ? e.message : "Sync failed");
    } finally {
      setSyncingChapters((prev) => prev.filter((n) => n !== chapterNum));
    }
  };

  const handleSaveSettings = async () => {
    if (!data) return;
    setSavingSettings(true);
    try {
      const body: Record<string, unknown> = {};
      if (settingsWordCount !== null) body.chapterWordCount = settingsWordCount;
      if (settingsTargetChapters !== null) body.targetChapters = settingsTargetChapters;
      if (settingsStatus !== null) body.status = settingsStatus;
      await fetchJson(`/books/${bookId}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      refetch();
    } catch (e) {
      alert(e instanceof Error ? e.message : "Save failed");
    } finally {
      setSavingSettings(false);
    }
  };

  const handleApproveAll = async () => {
    if (!data) return;
    const reviewable = data.chapters.filter((ch) => ch.status === "ready-for-review");
    let failed = 0;
    for (const chapter of reviewable) {
      try {
        await postApi(`/books/${bookId}/chapters/${chapter.number}/approve`);
      } catch {
        failed += 1;
      }
    }
    if (failed > 0) {
      alert(`${failed}/${reviewable.length} approve(s) failed`);
    }
    refetch();
  };

  const runBookAction = async (key: string, action: () => Promise<string>) => {
    setBookActionPending(key);
    try {
      alert(await action());
      refetch();
    } catch (e) {
      alert(e instanceof Error ? e.message : "Action failed");
    } finally {
      setBookActionPending(null);
    }
  };

  const handleEvaluate = async () => {
    await runBookAction("eval", async () => {
      const result = await fetchJson<{
        qualityScore: number;
        totalChapters: number;
        totalWords: number;
        auditPassRate: number;
        avgAiTellDensity: number;
        hookResolveRate: number;
      }>(`/books/${bookId}/eval`);
      return [
        `${t("book.evaluate")}: ${result.qualityScore}/100`,
        `${t("dash.chapters")}: ${result.totalChapters}`,
        `${t("book.words")}: ${result.totalWords.toLocaleString()}`,
        `Audit: ${result.auditPassRate}%`,
        `AI tells: ${result.avgAiTellDensity}/1k`,
        `Hooks: ${result.hookResolveRate}%`,
      ].join("\n");
    });
  };

  const handleConsolidate = async () => {
    await runBookAction("consolidate", async () => {
      const result = await fetchJson<{ archivedVolumes?: number; retainedChapters?: number }>(`/books/${bookId}/consolidate`, {
        method: "POST",
      });
      return data?.book.language === "en"
        ? `Consolidated ${result.archivedVolumes ?? 0} volume(s). Retained ${result.retainedChapters ?? 0} recent chapter summaries.`
        : `已归并 ${result.archivedVolumes ?? 0} 个卷摘要，保留最近 ${result.retainedChapters ?? 0} 条章节摘要。`;
    });
  };

  const handleReviseFoundation = async () => {
    const feedback = window.prompt(
      data?.book.language === "en"
        ? "Foundation revision feedback. This rewrites the book foundation, not chapter body."
        : "输入重修基础设定的反馈。此操作会重写基础设定，不直接改正文。",
      "",
    );
    if (!feedback?.trim()) return;
    await runBookAction("revise-foundation", async () => {
      await fetchJson(`/books/${bookId}/foundation/revise`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ feedback }),
      });
      return data?.book.language === "en" ? "Foundation revised." : "基础设定已重修。";
    });
  };

  const handlePlan = async () => {
    const context = window.prompt(
      data?.book.language === "en"
        ? "Optional planning context for the next chapter."
        : "可选：下一章规划补充说明。",
      "",
    );
    if (context === null) return;
    await runBookAction("plan", async () => {
      const result = await fetchJson<{ chapterNumber?: number; title?: string }>(`/books/${bookId}/plan`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ context: context.trim() || undefined }),
      });
      return data?.book.language === "en"
        ? `Planned chapter ${result.chapterNumber ?? "?"}: ${result.title ?? ""}`
        : `已计划第 ${result.chapterNumber ?? "?"} 章：${result.title ?? ""}`;
    });
  };

  const handleCompose = async () => {
    const context = window.prompt(
      data?.book.language === "en"
        ? "Optional compose context for the next chapter."
        : "可选：下一章组装补充说明。",
      "",
    );
    if (context === null) return;
    await runBookAction("compose", async () => {
      const result = await fetchJson<{ chapterNumber?: number; title?: string }>(`/books/${bookId}/compose`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ context: context.trim() || undefined }),
      });
      return data?.book.language === "en"
        ? `Composed chapter ${result.chapterNumber ?? "?"}: ${result.title ?? ""}`
        : `已组装第 ${result.chapterNumber ?? "?"} 章：${result.title ?? ""}`;
    });
  };

  const handleRepairState = async (chapterNum: number) => {
    await runBookAction(`repair-state-${chapterNum}`, async () => {
      await fetchJson(`/books/${bookId}/repair-state/${chapterNum}`, { method: "POST" });
      return data?.book.language === "en" ? `Chapter ${chapterNum} state repaired.` : `第 ${chapterNum} 章状态已修复。`;
    });
  };

  if (loading) return (
    <div className="flex flex-col items-center justify-center py-32 space-y-4">
      <div className="w-8 h-8 border-2 border-primary/20 border-t-primary rounded-full animate-spin" />
      <span className="text-sm text-muted-foreground">{t("common.loading")}</span>
    </div>
  );

  if (error) return <div className="text-destructive p-8 bg-destructive/5 rounded-xl border border-destructive/20">Error: {error}</div>;
  if (!data) return null;

  const { book, chapters } = data;
  const totalWords = chapters.reduce((sum, ch) => sum + (ch.wordCount ?? 0), 0);
  const reviewCount = chapters.filter((ch) => ch.status === "ready-for-review").length;

  const currentWordCount = settingsWordCount ?? book.chapterWordCount;
  const currentTargetChapters = settingsTargetChapters ?? book.targetChapters ?? 0;
  const currentStatus = settingsStatus ?? (book.status as BookStatus);

  return (
    <div className="space-y-8 fade-in">
      {/* Breadcrumbs */}
      <nav className="flex items-center gap-2 text-[13px] font-medium text-muted-foreground">
        <button
          onClick={nav.toDashboard}
          className="hover:text-primary transition-colors flex items-center gap-1"
        >
          <ChevronLeft size={14} />
          {t("bread.books")}
        </button>
        <span className="text-border">/</span>
        <span className="text-foreground">{book.title}</span>
      </nav>

      {/* G3/337 号：质量债务清单（无未决债务时自动隐藏）。 */}
      <QualityDebtsCard bookId={bookId} />

      {/* G10/338 号：承诺账本运营视图（无数据时自动隐藏）。 */}
      <PromiseTimelineCard bookId={bookId} />

      {/* G4/340 号：写法绑定管理（档案池选档 → 注入写作链）。 */}
      <StyleBindingCard bookId={bookId} />

      {/* G7b/343 号：新专名确认卡（三选确认写回名册，无候选时隐藏）。 */}
      <RosterCandidatesCard bookId={bookId} />

      {/* Header Section */}
      <div className="flex flex-col md:flex-row md:items-end justify-between gap-6 border-b border-border/40 pb-8">
        <div className="space-y-2">
          <div className="flex items-center gap-3">
            <h1 className="text-4xl font-serif font-medium">{book.title}</h1>
            {book.language === "en" && (
              <span className="px-1.5 py-0.5 rounded border border-primary/20 text-primary text-[10px] font-bold">EN</span>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-sm text-muted-foreground font-medium">
            <span className="px-2 py-0.5 rounded bg-secondary/50 text-foreground/70 tracking-wider text-xs">{genreLabel(book.genre, book.language === "en" ? "en" : "zh")}</span>
            <div className="flex items-center gap-1.5">
              <FileText size={14} />
              <span>{chapters.length} {t("dash.chapters")}</span>
            </div>
            <div className="flex items-center gap-1.5">
              <Zap size={14} />
              <span>{totalWords.toLocaleString()} {t("book.words")}</span>
            </div>
            {book.fanficMode && (
              <span className="flex items-center gap-1 text-purple-500">
                <Sparkles size={12} />
                <span className="italic">fanfic:{book.fanficMode}</span>
              </span>
            )}
          </div>
        </div>

        <div className="flex flex-wrap gap-2">
          {/* 176 号：writing 时按钮语义翻转为「停止」——Rust 引擎在阶段边界
              实停（175 号），Node 回退端诚实不可停、任务跑完自然收敛。 */}
          <button
            onClick={writing ? handleStopWriteNext : handleWriteNext}
            disabled={!writing && drafting}
            className={`flex items-center gap-2 px-5 py-2.5 text-sm font-bold rounded-xl transition-all shadow-lg disabled:opacity-50 ${
              writing
                ? "bg-destructive text-destructive-foreground hover:scale-105 active:scale-95 shadow-destructive/20"
                : "bg-primary text-primary-foreground hover:scale-105 active:scale-95 shadow-primary/20"
            }`}
          >
            {writing ? <Square size={14} /> : <Zap size={16} />}
            {writing ? t("book.stopWriting") : t("book.writeNext")}
          </button>
          <button
            onClick={handleDraft}
            disabled={writing || drafting}
            className="flex items-center gap-2 px-5 py-2.5 text-sm font-bold bg-secondary text-foreground rounded-xl hover:bg-secondary/80 transition-all border border-border/50 disabled:opacity-50"
          >
            {drafting ? <div className="w-4 h-4 border-2 border-muted-foreground/20 border-t-muted-foreground rounded-full animate-spin" /> : <Wand2 size={16} />}
            {drafting ? t("book.drafting") : t("book.draftOnly")}
          </button>
          <button
            onClick={() => nav.toBookTimeline(bookId)}
            className="flex items-center gap-2 px-5 py-2.5 text-sm font-bold bg-secondary text-foreground rounded-xl hover:bg-secondary/80 transition-all border border-border/50"
          >
            {t("timeline.title")}
          </button>
          <button
            onClick={handleToggleReviewMode}
            title={reviewMode === "manual"
              ? t("book.reviewModeManualHint")
              : t("book.reviewModeAutoHint")}
            className="flex items-center gap-2 px-4 py-2.5 text-sm font-medium bg-secondary/60 text-foreground rounded-xl border border-border/50 hover:bg-secondary transition-all"
          >
            {reviewMode === "manual" ? <Hand size={16} /> : <Settings2 size={16} />}
            {reviewMode === "manual" ? t("book.reviewModeManual") : t("book.reviewModeAuto")}
          </button>
          {/* 189 号：时间线节拍自动沉淀（书籍级开关，默认关）——开启后写完的
              章节会按既有情节线自动补节拍到时间线。 */}
          <button
            onClick={handleToggleAutoBeats}
            title={t("book.autoBeatsHint")}
            className={`flex items-center gap-2 px-4 py-2.5 text-sm font-medium rounded-xl border transition-all ${
              autoBeats
                ? "bg-primary/10 text-primary border-primary/30"
                : "bg-secondary/60 text-muted-foreground border-border/50 hover:bg-secondary"
            }`}
          >
            <Waypoints size={16} />
            {t("book.autoBeats")}
            <span
              className={`ml-0.5 inline-block h-2 w-2 rounded-full ${autoBeats ? "bg-primary" : "bg-muted-foreground/40"}`}
            />
          </button>
          <button
            onClick={() => setConfirmDeleteOpen(true)}
            disabled={deleting}
            className="flex items-center gap-2 px-5 py-2.5 text-sm font-bold bg-destructive/10 text-destructive rounded-xl hover:bg-destructive hover:text-white transition-all border border-destructive/20 disabled:opacity-50"
          >
            {deleting ? <div className="w-4 h-4 border-2 border-destructive/20 border-t-destructive rounded-full animate-spin" /> : <Trash2 size={16} />}
            {deleting ? t("common.loading") : t("book.deleteBook")}
          </button>
        </div>
      </div>

      {(writing || drafting || activity.lastError) && (
        <div
          className={`rounded-2xl border px-4 py-3 text-sm ${
            activity.lastError
              ? activity.lastError.includes("Operation aborted")
                ? // 188 号：用户主动停止属中性结果，不以红色失败呈现
                  "border-border/60 bg-secondary/30 text-muted-foreground"
                : "border-destructive/30 bg-destructive/5 text-destructive"
              : "border-primary/20 bg-primary/[0.04] text-foreground"
          }`}
        >
          {activity.lastError ? (
            <span>
              {activity.lastError.includes("Operation aborted")
                ? `${t("book.writeStopped")}`
                : `${t("book.pipelineFailed")}: ${activity.lastError}`}
            </span>
          ) : writing ? (
            <span>{t("book.pipelineWriting")}</span>
          ) : (
            <span>{t("book.pipelineDrafting")}</span>
          )}
        </div>
      )}

      {/* Tool Strip */}
      <div className="flex flex-wrap items-center gap-2 py-1">
          {reviewCount > 0 && (
            <button
              onClick={handleApproveAll}
              className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-emerald-500/10 text-emerald-600 rounded-lg hover:bg-emerald-500/20 transition-all border border-emerald-500/20"
            >
              <CheckCheck size={14} />
              {t("book.approveAll")} ({reviewCount})
            </button>
          )}
          <button
            onClick={() => nav.toTruth(bookId)}
            className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary/50 text-muted-foreground rounded-lg hover:text-foreground hover:bg-secondary transition-all border border-border/50"
          >
            <Database size={14} />
            {t("book.truthFiles")}
          </button>
          <button
            onClick={() => nav.toAnalytics(bookId)}
            className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary/50 text-muted-foreground rounded-lg hover:text-foreground hover:bg-secondary transition-all border border-border/50"
          >
            <BarChart2 size={14} />
            {t("book.analytics")}
          </button>
          <button
            onClick={handleEvaluate}
            disabled={bookActionPending === "eval"}
            className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary/50 text-muted-foreground rounded-lg hover:text-foreground hover:bg-secondary transition-all border border-border/50 disabled:opacity-50"
          >
            <Search size={14} />
            {bookActionPending === "eval" ? t("common.loading") : t("book.evaluate")}
          </button>
          <button
            onClick={handleConsolidate}
            disabled={bookActionPending === "consolidate"}
            className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary/50 text-muted-foreground rounded-lg hover:text-foreground hover:bg-secondary transition-all border border-border/50 disabled:opacity-50"
          >
            <Database size={14} />
            {bookActionPending === "consolidate" ? t("common.loading") : t("book.consolidate")}
          </button>
          <button
            onClick={handleReviseFoundation}
            disabled={bookActionPending === "revise-foundation"}
            className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary/50 text-muted-foreground rounded-lg hover:text-foreground hover:bg-secondary transition-all border border-border/50 disabled:opacity-50"
          >
            <Sparkles size={14} />
            {bookActionPending === "revise-foundation" ? t("common.loading") : t("book.reviseFoundation")}
          </button>
          <button
            onClick={handlePlan}
            disabled={bookActionPending === "plan"}
            className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary/50 text-muted-foreground rounded-lg hover:text-foreground hover:bg-secondary transition-all border border-border/50 disabled:opacity-50"
          >
            <FileText size={14} />
            {bookActionPending === "plan" ? t("common.loading") : t("book.planNext")}
          </button>
          <button
            onClick={handleCompose}
            disabled={bookActionPending === "compose"}
            className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary/50 text-muted-foreground rounded-lg hover:text-foreground hover:bg-secondary transition-all border border-border/50 disabled:opacity-50"
          >
            <Wand2 size={14} />
            {bookActionPending === "compose" ? t("common.loading") : t("book.composeNext")}
          </button>
          <div className="flex items-center gap-2">
            <select
              value={exportFormat}
              onChange={(e) => setExportFormat(e.target.value as ExportFormat)}
              className="px-2 py-2 text-xs font-bold bg-secondary/50 text-muted-foreground rounded-lg border border-border/50 outline-none"
            >
              <option value="txt">TXT</option>
              <option value="md">MD</option>
              <option value="epub">EPUB</option>
            </select>
            <label className="flex items-center gap-1.5 text-xs font-bold text-muted-foreground cursor-pointer select-none">
              <input
                type="checkbox"
                checked={exportApprovedOnly}
                onChange={(e) => setExportApprovedOnly(e.target.checked)}
                className="rounded border-border/50"
              />
              {t("book.approvedOnly")}
            </label>
            <button
              onClick={async () => {
                try {
                  const data = await fetchJson<{ path?: string; chapters?: number }>(`/books/${bookId}/export-save`, {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ format: exportFormat, approvedOnly: exportApprovedOnly }),
                  });
                  alert(`${t("common.exportSuccess")}\n${data.path}\n(${data.chapters} ${t("dash.chapters")})`);
                } catch (e) {
                  alert(e instanceof Error ? e.message : "Export failed");
                }
              }}
              className="flex items-center gap-2 px-4 py-2 text-xs font-bold bg-secondary/50 text-muted-foreground rounded-lg hover:text-foreground hover:bg-secondary transition-all border border-border/50"
            >
              <Download size={14} />
              {t("book.export")}
            </button>
          </div>
      </div>

      {/* Book Settings */}
      <div className="paper-sheet rounded-2xl border border-border/40 shadow-sm p-6">
        <h2 className="text-sm font-bold uppercase tracking-widest text-muted-foreground mb-4">{t("book.settings")}</h2>
        <div className="flex flex-wrap items-end gap-4">
          <div className="flex flex-col gap-1">
            <label className="text-[11px] font-bold uppercase tracking-widest text-muted-foreground">{t("create.wordsPerChapter")}</label>
            <input
              type="number"
              value={currentWordCount}
              onChange={(e) => setSettingsWordCount(Number(e.target.value))}
              className="px-3 py-2 text-sm rounded-lg border border-border/50 bg-secondary/30 outline-none focus:border-primary/50 w-32"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-[11px] font-bold uppercase tracking-widest text-muted-foreground">{t("create.targetChapters")}</label>
            <input
              type="number"
              value={currentTargetChapters}
              onChange={(e) => setSettingsTargetChapters(Number(e.target.value))}
              className="px-3 py-2 text-sm rounded-lg border border-border/50 bg-secondary/30 outline-none focus:border-primary/50 w-32"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-[11px] font-bold uppercase tracking-widest text-muted-foreground">{t("book.status")}</label>
            <select
              value={currentStatus}
              onChange={(e) => setSettingsStatus(e.target.value as BookStatus)}
              className="px-3 py-2 text-sm rounded-lg border border-border/50 bg-secondary/30 outline-none focus:border-primary/50"
            >
              <option value="active">{t("book.statusActive")}</option>
              <option value="paused">{t("book.statusPaused")}</option>
              <option value="outlining">{t("book.statusOutlining")}</option>
              <option value="completed">{t("book.statusCompleted")}</option>
              <option value="dropped">{t("book.statusDropped")}</option>
            </select>
          </div>
          <button
            onClick={handleSaveSettings}
            disabled={savingSettings}
            className="flex items-center gap-2 px-4 py-2 text-sm font-bold bg-primary text-primary-foreground rounded-lg hover:scale-105 active:scale-95 transition-all disabled:opacity-50"
          >
            {savingSettings ? <div className="w-4 h-4 border-2 border-primary-foreground/20 border-t-primary-foreground rounded-full animate-spin" /> : <Save size={14} />}
            {savingSettings ? t("book.saving") : t("book.save")}
          </button>
        </div>
      </div>

      {/* Chapters Table */}
      <div className="paper-sheet rounded-2xl overflow-hidden border border-border/40 shadow-xl shadow-primary/5">
        <div className="overflow-x-auto">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="bg-muted/30 border-b border-border/50">
                <th className="text-left px-6 py-4 font-bold text-[11px] uppercase tracking-widest text-muted-foreground w-16">#</th>
                <th className="text-left px-6 py-4 font-bold text-[11px] uppercase tracking-widest text-muted-foreground">{t("book.manuscriptTitle")}</th>
                <th className="text-left px-6 py-4 font-bold text-[11px] uppercase tracking-widest text-muted-foreground w-28">{t("book.words")}</th>
                <th className="text-left px-6 py-4 font-bold text-[11px] uppercase tracking-widest text-muted-foreground w-36">{t("book.status")}</th>
                <th className="text-right px-6 py-4 font-bold text-[11px] uppercase tracking-widest text-muted-foreground">{t("book.curate")}</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/30">
              {chapters.map((ch, index) => {
                const staggerClass = `stagger-${Math.min(index + 1, 5)}`;
                return (
                <tr key={ch.number} className={`group hover:bg-primary/[0.02] transition-colors fade-in ${staggerClass}`}>
                  <td className="px-6 py-4 text-muted-foreground/60 font-mono text-xs">{ch.number.toString().padStart(2, '0')}</td>
                  <td className="px-6 py-4">
                    <button
                      onClick={() => nav.toChapter(bookId, ch.number)}
                      className="font-serif text-lg font-medium hover:text-primary transition-colors text-left"
                    >
                      {ch.title || t("chapter.label").replace("{n}", String(ch.number))}
                    </button>
                  </td>
                  <td className="px-6 py-4 text-muted-foreground font-medium tabular-nums text-xs">{(ch.wordCount ?? 0).toLocaleString()}</td>
                  <td className="px-6 py-4">
                    <div className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[10px] font-bold uppercase tracking-tight ${STATUS_CONFIG[ch.status]?.color ?? "bg-muted text-muted-foreground"}`}>
                      {STATUS_CONFIG[ch.status]?.icon}
                      {translateChapterStatus(ch.status, t)}
                    </div>
                  </td>
                  <td className="px-6 py-4 text-right">
                    <div className="flex gap-1.5 justify-end opacity-0 group-hover:opacity-100 transition-opacity">
                      {ch.status === "ready-for-review" && (
                        <>
                          <button
                            onClick={async () => {
                              try { await postApi(`/books/${bookId}/chapters/${ch.number}/approve`); refetch(); }
                              catch (e) { alert(e instanceof Error ? e.message : "Approve failed"); }
                            }}
                            className="p-2 rounded-lg bg-emerald-500/10 text-emerald-600 hover:bg-emerald-500 hover:text-white transition-all shadow-sm"
                            title={t("book.approve")}
                          >
                            <Check size={14} />
                          </button>
                          <button
                            onClick={async () => {
                              try { await postApi(`/books/${bookId}/chapters/${ch.number}/reject`); refetch(); }
                              catch (e) { alert(e instanceof Error ? e.message : "Reject failed"); }
                            }}
                            className="p-2 rounded-lg bg-destructive/10 text-destructive hover:bg-destructive hover:text-white transition-all shadow-sm"
                            title={t("book.reject")}
                          >
                            <X size={14} />
                          </button>
                        </>
                      )}
                      <button
                        onClick={async () => {
                          try {
                            const auditResult = await fetchJson<{ passed?: boolean; issues?: unknown[] }>(`/books/${bookId}/audit/${ch.number}`, { method: "POST" });
                            alert(auditResult.passed ? "Audit passed" : `Audit failed: ${auditResult.issues?.length ?? 0} issues`);
                            refetch();
                          } catch (e) {
                            alert(e instanceof Error ? e.message : "Audit failed");
                          }
                        }}
                        className="p-2 rounded-lg bg-secondary text-muted-foreground hover:text-primary hover:bg-primary/10 transition-all shadow-sm"
                        title={t("book.audit")}
                      >
                        <ShieldCheck size={14} />
                      </button>
                      <button
                        onClick={() => handleRewrite(ch.number)}
                        disabled={rewritingChapters.includes(ch.number)}
                        className="p-2 rounded-lg bg-secondary text-muted-foreground hover:text-primary hover:bg-primary/10 transition-all shadow-sm disabled:opacity-50"
                        title={t("book.rewrite")}
                      >
                        {rewritingChapters.includes(ch.number)
                          ? <div className="w-3.5 h-3.5 border-2 border-muted-foreground/20 border-t-muted-foreground rounded-full animate-spin" />
                          : <RotateCcw size={14} />}
                      </button>
                      <button
                        onClick={() => handleSync(ch.number)}
                        disabled={syncingChapters.includes(ch.number) || ch.number !== latestPersistedChapter}
                        className="p-2 rounded-lg bg-secondary text-muted-foreground hover:text-primary hover:bg-primary/10 transition-all shadow-sm disabled:opacity-50"
                        title={data?.book.language === "en" ? "Sync truth/state from edited chapter" : "根据已编辑章节同步 truth/state"}
                      >
                        {syncingChapters.includes(ch.number)
                          ? <div className="w-3.5 h-3.5 border-2 border-muted-foreground/20 border-t-muted-foreground rounded-full animate-spin" />
                          : <RefreshCw size={14} />}
                      </button>
                      {ch.status === "state-degraded" && (
                        <button
                          onClick={() => handleRepairState(ch.number)}
                          disabled={bookActionPending === `repair-state-${ch.number}`}
                          className="p-2 rounded-lg bg-amber-500/10 text-amber-600 hover:bg-amber-500 hover:text-white transition-all shadow-sm disabled:opacity-50"
                          title={t("book.repairState")}
                        >
                          {bookActionPending === `repair-state-${ch.number}`
                            ? <div className="w-3.5 h-3.5 border-2 border-amber-600/20 border-t-amber-600 rounded-full animate-spin" />
                            : <Settings2 size={14} />}
                        </button>
                      )}
                      <select
                        disabled={revisingChapters.includes(ch.number)}
                        value=""
                        onChange={(e) => {
                          const mode = e.target.value as ReviseMode;
                          if (mode) handleRevise(ch.number, mode);
                        }}
                        className="px-2 py-1.5 text-[11px] font-bold rounded-lg bg-secondary text-muted-foreground border border-border/50 outline-none hover:text-primary hover:bg-primary/10 transition-all disabled:opacity-50 cursor-pointer"
                        title="Revise with AI"
                      >
                        <option value="" disabled>{revisingChapters.includes(ch.number) ? t("common.loading") : t("book.curate")}</option>
                        <option value="spot-fix">{t("book.spotFix")}</option>
                        <option value="polish">{t("book.polish")}</option>
                        <option value="rewrite">{t("book.rewrite")}</option>
                        <option value="rework">{t("book.rework")}</option>
                        <option value="anti-detect">{t("book.antiDetect")}</option>
                      </select>
                    </div>
                  </td>
                </tr>
                );
              })}
            </tbody>
          </table>
        </div>

        {chapters.length === 0 && (
          <div className="flex flex-col items-center justify-center py-20 text-center">
            <div className="w-12 h-12 rounded-full bg-muted/20 flex items-center justify-center mb-4">
               <FileText size={20} className="text-muted-foreground/40" />
            </div>
            <p className="text-sm italic font-serif text-muted-foreground">
              {t("book.noChapters")}
            </p>
          </div>
        )}
      </div>

      <ConfirmDialog
        open={confirmDeleteOpen}
        title={t("book.deleteBook")}
        message={t("book.confirmDelete")}
        confirmLabel={t("common.delete")}
        cancelLabel={t("common.cancel")}
        variant="danger"
        onConfirm={handleDeleteBook}
        onCancel={() => setConfirmDeleteOpen(false)}
      />
    </div>
  );
}
