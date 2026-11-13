import { useState, useEffect, useCallback, useRef } from "react";
import type { Theme } from "../../hooks/use-theme";
import type { TFunction } from "../../hooks/use-i18n";
import type { SSEMessage } from "../../hooks/use-sse";
import { useChatStore } from "../../store/chat";
import { fetchJson } from "../../hooks/use-api";
import { useI18n } from "../../hooks/use-i18n";
import { tr } from "../../lib/app-language";
import { PanelRightClose, PanelRightOpen, ArrowLeft, Loader2, Pencil, Save, X } from "lucide-react";
import { Streamdown } from "streamdown";
import { cjk } from "@streamdown/cjk";
import { ProgressSection } from "../sidebar/ProgressSection";
import { FoundationSection } from "../sidebar/FoundationSection";
import { SummarySection } from "../sidebar/SummarySection";
import { ChaptersSection } from "../sidebar/ChaptersSection";
import { CharacterSection } from "../sidebar/CharacterSection";
import { PendingHooksView } from "../sidebar/PendingHooksView";
import {
  FOUNDATION_FILE_LABELS,
  presentCurrentState,
  roleFromPath,
} from "../../lib/truth-display";
import {
  artifactEditDraftKey,
  clearArtifactEditDraft,
  loadArtifactEditDraft,
  saveArtifactEditDraft,
} from "../../lib/artifact-edit-draft";

export interface BookSidebarProps {
  readonly bookId: string;
  readonly theme: Theme;
  readonly t: TFunction;
  readonly sse: { messages: ReadonlyArray<SSEMessage>; connected: boolean };
}

const streamdownPlugins = { cjk };

// Friendly header label for an opened truth file: character files show the
// character's name, foundation files their friendly label, everything else its
// path as a last resort.
function artifactLabel(file: string): string {
  return roleFromPath(file)?.name ?? FOUNDATION_FILE_LABELS[file] ?? file;
}

// Read-mode body for an opened file. A few files need reader-friendly handling
// instead of raw markdown: pending_hooks.md (a wide tracking table) renders as
// cards and current_state.md gets a reader-facing empty state.
function renderTruthBody(
  file: string | null,
  content: string,
) {
  if (file === "pending_hooks.md") {
    return <PendingHooksView content={content} />;
  }
  if (file === "current_state.md") {
    const { isEmpty, body: stateBody } = presentCurrentState(content);
    return isEmpty ? (
      <p className="text-[14px] leading-6 text-muted-foreground/60 italic">
        {tr(
          "还没有运行状态。开始写作后，每写完一章这里会自动记录最新的故事进展。",
          "No run status yet. Once writing starts, each finished chapter is logged here automatically.",
        )}
      </p>
    ) : (
      <Streamdown plugins={streamdownPlugins} mode="static">{stateBody}</Streamdown>
    );
  }
  return <Streamdown plugins={streamdownPlugins} mode="static">{content}</Streamdown>;
}

export function ArtifactView({ bookId }: { readonly bookId: string }) {
  const { t } = useI18n();
  const artifactFile = useChatStore((s) => s.artifactFile);
  const artifactChapter = useChatStore((s) => s.artifactChapter);
  const closeArtifact = useChatStore((s) => s.closeArtifact);
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState(false);
  const [editContent, setEditContent] = useState("");
  const [saving, setSaving] = useState(false);

  const isChapter = artifactChapter !== null;
  const label = isChapter
    ? t("artifact.chapterLabel").replace("{n}", String(artifactChapter))
    : artifactFile ? artifactLabel(artifactFile) : "";

  useEffect(() => {
    // 1008 号：响应竞态守卫——目标（artifactFile/章节号）在 dock 挂载中
    // in-place 切换，旧目标响应晚到不得覆盖新目标内容；旧目标失败晚到
    // 不得把新目标正文清成空态。
    let cancelled = false;
    setLoading(true);
    // 998 号：目标切换时恢复该目标的编辑草稿（dock 开合/面板切换卸载重建后
    // 编辑中状态与未保存内容不丢——996 同族第二面）。无草稿 → 非编辑态
    // （与原 setEditing(false) 切换语义一致）。
    const draftKey = artifactEditDraftKey(bookId, isChapter, artifactChapter, artifactFile);
    const draft = loadArtifactEditDraft(draftKey);
    setEditing(draft?.editing ?? false);
    setEditContent(draft?.content ?? "");
    if (isChapter) {
      fetchJson<{ content: string }>(`/books/${bookId}/chapters/${artifactChapter}`)
        .then((data) => {
          if (!cancelled) setContent(data.content ?? "");
        })
        .catch(() => {
          if (!cancelled) setContent(null);
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    } else if (artifactFile) {
      fetchJson<{ content: string | null }>(
        `/books/${bookId}/truth/${artifactFile}`,
      )
        .then((data) => {
          if (!cancelled) setContent(data.content ?? "");
        })
        .catch(() => {
          if (!cancelled) setContent(null);
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    }
    return () => {
      cancelled = true;
    };
  }, [bookId, artifactFile, artifactChapter, isChapter]);

  const handleEdit = useCallback(() => {
    setEditContent(content ?? "");
    setEditing(true);
    // 998 号：进入编辑即建草稿（此后每次输入实时更新——卸载不丢）
    saveArtifactEditDraft(
      artifactEditDraftKey(bookId, isChapter, artifactChapter, artifactFile),
      { editing: true, content: content ?? "" },
    );
  }, [content, bookId, artifactFile, artifactChapter, isChapter]);

  const handleDraftChange = useCallback(
    (value: string) => {
      setEditContent(value);
      saveArtifactEditDraft(
        artifactEditDraftKey(bookId, isChapter, artifactChapter, artifactFile),
        { editing: true, content: value },
      );
    },
    [bookId, artifactFile, artifactChapter, isChapter],
  );

  const handleCancelEdit = useCallback(() => {
    setEditing(false);
    clearArtifactEditDraft(
      artifactEditDraftKey(bookId, isChapter, artifactChapter, artifactFile),
    );
  }, [bookId, artifactFile, artifactChapter, isChapter]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    try {
      if (isChapter) {
        await fetchJson(`/books/${bookId}/chapters/${artifactChapter}`, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ content: editContent }),
        });
      } else if (artifactFile) {
        await fetchJson(`/books/${bookId}/truth/${artifactFile}`, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ content: editContent }),
        });
      }
      setContent(editContent);
      setEditing(false);
      // 998 号：保存成功清草稿（会话内该目标回到非编辑态）
      clearArtifactEditDraft(
        artifactEditDraftKey(bookId, isChapter, artifactChapter, artifactFile),
      );
    } catch {
      // keep editing state on error（草稿保留——失败不丢未保存内容）
    } finally {
      setSaving(false);
    }
  }, [bookId, artifactFile, artifactChapter, isChapter, editContent]);

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 px-3 py-2.5 border-b border-border/20 shrink-0">
        <button
          onClick={closeArtifact}
          aria-label={t("book.backToFileList")}
          className="w-6 h-6 rounded-md flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-secondary/50 transition-colors"
        >
          <ArrowLeft size={14} />
        </button>
        <span className="text-[15px] leading-6 font-medium truncate flex-1">{label}</span>
        {!loading && content !== null && !editing && (
          <button
            onClick={handleEdit}
            aria-label={t("book.editFileContent")}
            className="w-6 h-6 rounded-md flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-secondary/50 transition-colors"
          >
            <Pencil size={12} />
          </button>
        )}
        {editing && (
          <div className="flex items-center gap-1">
            <button
              onClick={handleSave}
              disabled={saving}
              aria-label={t("book.saveChanges")}
              className="w-6 h-6 rounded-md flex items-center justify-center text-emerald-500 hover:bg-emerald-500/10 transition-colors"
            >
              {saving ? <Loader2 size={12} className="animate-spin" /> : <Save size={12} />}
            </button>
            <button
              onClick={handleCancelEdit}
              aria-label={t("book.cancelEditing")}
              className="w-6 h-6 rounded-md flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-secondary/50 transition-colors"
            >
              <X size={12} />
            </button>
          </div>
        )}
      </div>
      <div className="flex-1 overflow-y-auto">
        {loading ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 size={16} className="text-muted-foreground animate-spin" />
          </div>
        ) : content === null ? (
          <p className="text-[14px] leading-6 text-foreground/70 italic px-4 py-3">{t("artifact.fileMissing")}</p>
        ) : editing ? (
          <textarea
            value={editContent}
            onChange={(e) => handleDraftChange(e.target.value)}
            aria-label={t("book.editFileContent")}
            className="w-full h-full min-h-[300px] bg-transparent text-[15px] leading-7 px-4 py-3 resize-none outline-none border-0 font-mono"
          />
        ) : (
          <div className="px-4 py-3 text-[15px] leading-7">
            {renderTruthBody(isChapter ? null : artifactFile, content)}
          </div>
        )}
      </div>
    </div>
  );
}

// 1003 号：写作活动指示全量重放（999 同族补位）——PanelView 挂在移动端抽屉
// BookSidebarToggle 的 `{open && ...}` 条件挂载下，旧「只看最后一条」增量形
// effect 在抽屉开合卸载期间 store 累积的中间事件被跳过：write:start 之后落到
// log 等未映射类型时重挂载 activeOp 保持 null，「正在写作中」指示丢失。指示
// 本就是消息流的纯派生——全量重放幂等且卸载期事件不丢（与 999
// deriveProgressState 同形；挂载中逐事件语义等价，未映射事件不动累加器）。
export type ActiveOp = "write" | "audit" | "revise";

export function deriveActiveOp(messages: ReadonlyArray<SSEMessage>): ActiveOp | null {
  let activeOp: ActiveOp | null = null;
  for (const message of messages) {
    if (message.event === "write:start") {
      activeOp = "write";
    } else if (message.event === "tool:start") {
      const data = message.data as { tool?: string } | null;
      const action = data?.tool?.split("__").at(-1);
      if (action === "write_chapters") activeOp = "write";
      else if (action === "review_chapter") activeOp = "audit";
      else if (action === "revise_chapter") activeOp = "revise";
    } else if (message.event === "write:complete" || message.event === "tool:end") {
      activeOp = null;
    }
    // 其余事件（log/…）：不改变活动操作——与旧增量形逐事件语义一致
  }
  return activeOp;
}

// P3-4：内部视图导出给 ContextDock（面板注册表复用）；BookSidebar 本体保留
// 为旧轨（P4 走查后随双轨清理一并删除）。
export function PanelView({ bookId, theme: _theme, t, sse }: BookSidebarProps) {
  const isZh = t("nav.connected") === "\u5DF2\u8FDE\u63A5";

  // Show writing indicator only during pipeline operations (write/audit/revise)
  // 1003 号：增量 useState+useEffect → 消息流全量重放派生（卸载期事件不丢）
  const activeOp = deriveActiveOp(sse.messages);

  const OP_LABELS: Record<string, string> = {
    write: isZh ? "正在写作中..." : "Writing...",
    audit: isZh ? "正在审计中..." : "Auditing...",
    revise: isZh ? "正在修订中..." : "Revising...",
  };

  return (
    <div className="flex flex-col gap-2 p-3">
      {activeOp && (
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-primary/5 border border-primary/10">
          <Loader2 size={12} className="text-primary animate-spin shrink-0" />
          <span className="text-[14px] leading-5 text-primary font-medium">
            {OP_LABELS[activeOp] ?? activeOp}
          </span>
        </div>
      )}
      <ProgressSection sse={sse} />
      <ChaptersSection bookId={bookId} isZh={isZh} />
      <CharacterSection bookId={bookId} />
      <FoundationSection bookId={bookId} />
      <SummarySection bookId={bookId} />
    </div>
  );
}

const SIDEBAR_RATIO = 0.4;
const SIDEBAR_MIN = 280;
const SIDEBAR_MAX = 700;

function defaultSidebarWidth(): number {
  return Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Math.round(window.innerWidth * SIDEBAR_RATIO)));
}

export function BookSidebar({ bookId, theme, t, sse }: BookSidebarProps) {
  const sidebarView = useChatStore((s) => s.sidebarView);
  const [width, setWidth] = useState(defaultSidebarWidth);
  const dragging = useRef(false);

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    const startX = e.clientX;
    const startW = width;
    const onMove = (ev: MouseEvent) => {
      if (!dragging.current) return;
      const delta = startX - ev.clientX;
      setWidth(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startW + delta)));
    };
    const onUp = () => {
      dragging.current = false;
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }, [width]);

  return (
    <aside
      aria-label={t("nav.openContextPanel")}
      className="hidden lg:flex shrink-0 flex-col bg-background/30 backdrop-blur-sm overflow-y-auto relative"
      style={{ width }}
    >
      {/* Resize handle */}
      <div
        onMouseDown={onMouseDown}
        className="absolute left-0 top-0 h-full w-1 cursor-col-resize hover:bg-primary/20 active:bg-primary/30 transition-colors z-10"
      />
      {sidebarView === "artifact" ? (
        <ArtifactView bookId={bookId} />
      ) : (
        <PanelView bookId={bookId} theme={theme} t={t} sse={sse} />
      )}
    </aside>
  );
}

export function BookSidebarToggle({ bookId, theme, t, sse }: BookSidebarProps) {
  const [open, setOpen] = useState(false);
  const sidebarView = useChatStore((s) => s.sidebarView);

  return (
    <>
      <button
        onClick={() => setOpen(true)}
        aria-label={t("nav.openContextPanel")}
        className="fixed right-3 top-[72px] z-20 lg:hidden w-8 h-8 rounded-lg bg-card border border-border/40 flex items-center justify-center text-muted-foreground hover:text-foreground transition-colors"
      >
        <PanelRightOpen size={14} />
      </button>

      {open && (
        <div className="fixed inset-0 z-40 lg:hidden" onClick={() => setOpen(false)}>
          <div className="absolute inset-0 bg-black/20 backdrop-blur-sm" />
          <aside
            aria-label={t("nav.openContextPanel")}
            className="absolute right-0 top-0 h-full w-[420px] max-w-[85vw] bg-background border-l border-border/20 overflow-y-auto"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between px-3 py-2 border-b border-border/20">
              <span className="text-[15px] leading-6 font-medium text-muted-foreground">
                {t("nav.mobilePanelTitle")}
              </span>
              {/* 1003 号：873/1002 同族补位——纯图标关闭钮补可访问名称。1002 备案
                  「SR 用户走关闭钮路径」的前提此前并不成立（该钮自身无名）；遮罩
                  div 仍维持备案不设名。 */}
              <button
                onClick={() => setOpen(false)}
                aria-label={t("nav.closeContextPanel")}
                className="w-6 h-6 rounded-md flex items-center justify-center text-muted-foreground hover:text-foreground"
              >
                <PanelRightClose size={14} />
              </button>
            </div>
            {sidebarView === "artifact" ? (
              <ArtifactView bookId={bookId} />
            ) : (
              <PanelView bookId={bookId} theme={theme} t={t} sse={sse} />
            )}
          </aside>
        </div>
      )}
    </>
  );
}
