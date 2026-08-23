import { Loader2, Eye } from "lucide-react";
import { tr } from "@/lib/app-language";
import { useApi } from "@/hooks/use-api";
import { chatSelectors, useChatStore } from "@/store/chat";

/**
 * 状态栏（P3-5）：h-6 常驻底条，全部真实数据源。
 * 左=当前书·章节+字数（章节内容前端统计）；中=SSE 连接点（绿/黄，断连点击
 * 重连）+ daemon 运行点；右=生成中 spinner +「查看」直达底部面板任务流。
 */

function countWords(content: string): number {
  // 中文字数近似：去空白后的字符数（与项目字数口径一致的前端近似）
  return content.replace(/\s/g, "").length;
}

function StatusDot({ color, label }: { color: string; label: string }) {
  return <span className={`inline-block size-1.5 rounded-full ${color}`} aria-label={label} title={label} />;
}

export function StatusBar({ bookTitle, chapter, sseConnected, onReconnect, daemonRunning, onOpenBottomPanel }: {
  /** 当前书名（书/章等书籍系页面），无则显示项目级文案。 */
  bookTitle?: string;
  /** 章节页携带 {bookId, number}；组件自取章节内容统计字数。 */
  chapter?: { bookId: string; number: number };
  sseConnected: boolean;
  onReconnect: () => void;
  daemonRunning: boolean | undefined;
  onOpenBottomPanel: () => void;
}) {
  const generating = useChatStore(chatSelectors.isActiveSessionStreaming);
  const { data: chapterData } = useApi<{ content: string }>(
    chapter ? `/books/${chapter.bookId}/chapters/${chapter.number}` : "/project",
  );

  const leftLabel = chapter
    ? `${bookTitle ?? chapter.bookId} · ${tr(`第 ${chapter.number} 章`, `Chapter ${chapter.number}`)}`
    : bookTitle ?? tr("InkOS Studio", "InkOS Studio");

  return (
    <footer
      data-slot="status-bar"
      data-testid="status-bar"
      className="h-6 shrink-0 flex items-center gap-4 px-3 border-t border-border/40 bg-background/80 text-[11px] text-muted-foreground select-none"
    >
      {/* 左：当前书·章节 + 字数 */}
      <span className="truncate" data-testid="status-context">
        {leftLabel}
        {chapter && chapterData?.content ? (
          <span className="ml-2 text-muted-foreground/60">
            {tr(`约 ${countWords(chapterData.content)} 字`, `~${countWords(chapterData.content)} chars`)}
          </span>
        ) : null}
      </span>

      <div className="flex-1" />

      {/* 中：SSE 连接点 + daemon 运行点 */}
      <button
        type="button"
        data-testid="status-sse"
        onClick={onReconnect}
        title={sseConnected ? tr("实时连接正常", "Live connection OK") : tr("实时连接断开，点击重连", "Live connection lost — click to reconnect")}
        className="flex items-center gap-1.5 hover:text-foreground transition-colors"
      >
        <StatusDot
          color={sseConnected ? "bg-emerald-500" : "bg-amber-500 animate-pulse"}
          label={sseConnected ? "sse-ok" : "sse-lost"}
        />
        {sseConnected ? tr("实时", "Live") : tr("重连", "Reconnect")}
      </button>
      <span className="flex items-center gap-1.5" data-testid="status-daemon" title={tr("后台守护进程", "Background daemon")}>
        <StatusDot
          color={daemonRunning ? "bg-emerald-500" : "bg-muted-foreground/40"}
          label={daemonRunning ? "daemon-running" : "daemon-stopped"}
        />
        {tr("守护", "Daemon")}
      </span>

      {/* 右：生成中 + 查看任务流 */}
      {generating ? (
        <span className="flex items-center gap-1 text-primary" data-testid="status-generating">
          <Loader2 size={11} className="animate-spin" />
          {tr("生成中", "Generating")}
          <button
            type="button"
            onClick={onOpenBottomPanel}
            className="ml-1 inline-flex items-center gap-1 rounded px-1 hover:bg-secondary/60 transition-colors"
          >
            <Eye size={11} />
            {tr("查看", "View")}
          </button>
        </span>
      ) : null}
    </footer>
  );
}
