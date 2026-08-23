import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowDownToLine, Maximize2, Minimize2, X } from "lucide-react";
import { tr } from "@/lib/app-language";
import { useApi } from "@/hooks/use-api";
import { chatSelectors, useChatStore } from "@/store/chat";
import type { ToolExecution } from "@/store/chat/types";
import { ToolExecutionSteps } from "@/components/chat/ToolExecutionSteps";

/**
 * 底部面板（P3-4）：Cmd+J 开合（App 分发 + 按页记忆）。
 * 两 tab：生成任务流（复用 ToolExecutionSteps 与 chat store 同一数据源）、
 * 日志（/logs 轻量内嵌，同 LogViewer 数据面）。可最大化高度；滚动跟随
 * 默认开启，用户上滚即暂停（按钮可恢复）。
 */

const NORMAL_HEIGHT = 240;

interface LogEntry {
  readonly level?: string;
  readonly tag?: string;
  readonly message: string;
  readonly timestamp?: string;
}

const LEVEL_COLORS: Record<string, string> = {
  error: "text-destructive",
  warn: "text-amber-500",
  info: "text-primary/70",
  debug: "text-muted-foreground/50",
};

/** 从激活会话消息里收集工具执行记录（与聊天区同源，倒序取最近 40 条）。 */
export function collectRecentExecutions(
  messages: ReadonlyArray<{ toolExecutions?: ReadonlyArray<ToolExecution> }>,
  limit = 40,
): ToolExecution[] {
  const all: ToolExecution[] = [];
  for (const message of messages) {
    if (message.toolExecutions) all.push(...message.toolExecutions);
  }
  return all.slice(-limit);
}

export function BottomPanel({ visible, onClose }: {
  visible: boolean;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<"tasks" | "logs">("tasks");
  const [maximized, setMaximized] = useState(false);
  const [follow, setFollow] = useState(true);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const messages = useChatStore(chatSelectors.activeMessages);
  const executions = useMemo(() => collectRecentExecutions(messages), [messages]);
  const { data: logsData, refetch: refetchLogs } = useApi<{ entries: ReadonlyArray<LogEntry> }>("/logs");
  const logEntries = useMemo(() => (logsData?.entries ?? []).slice(-200), [logsData]);

  // 滚动跟随：内容变化且未暂停时贴底
  useEffect(() => {
    if (!follow || !visible) return;
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [executions.length, logEntries.length, tab, follow, visible]);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    // 用户上滚离开底部 → 暂停跟随；滚回底部 → 自动恢复
    setFollow(distanceFromBottom < 40);
  };

  if (!visible) return null;

  return (
    <section
      data-slot="bottom-panel"
      data-testid="bottom-panel"
      className="shrink-0 flex flex-col border-t border-border/40 bg-background/70 backdrop-blur-sm"
      style={{ height: maximized ? "70vh" : NORMAL_HEIGHT }}
    >
      {/* 工具栏：tab 切换 + 跟随/最大化/关闭 */}
      <div className="flex items-center gap-1 px-2 h-9 shrink-0 border-b border-border/30">
        <button
          type="button"
          data-testid="bottom-tab-tasks"
          onClick={() => setTab("tasks")}
          className={`px-3 py-1 rounded-md text-[13px] transition-colors ${
            tab === "tasks" ? "bg-secondary text-foreground font-medium" : "text-muted-foreground hover:text-foreground"
          }`}
        >
          {tr("生成任务流", "Task Stream")}
        </button>
        <button
          type="button"
          data-testid="bottom-tab-logs"
          onClick={() => setTab("logs")}
          className={`px-3 py-1 rounded-md text-[13px] transition-colors ${
            tab === "logs" ? "bg-secondary text-foreground font-medium" : "text-muted-foreground hover:text-foreground"
          }`}
        >
          {tr("日志", "Logs")}
        </button>
        <div className="flex-1" />
        <button
          type="button"
          data-testid="bottom-follow"
          aria-pressed={follow}
          onClick={() => setFollow((value) => !value)}
          title={tr(follow ? "暂停滚动跟随" : "恢复滚动跟随", follow ? "Pause auto-scroll" : "Resume auto-scroll")}
          className={`w-7 h-7 rounded-md flex items-center justify-center transition-colors ${
            follow ? "text-primary hover:bg-primary/10" : "text-muted-foreground hover:text-foreground hover:bg-secondary/50"
          }`}
        >
          <ArrowDownToLine size={14} />
        </button>
        <button
          type="button"
          data-testid="bottom-maximize"
          onClick={() => setMaximized((value) => !value)}
          title={tr(maximized ? "还原高度" : "最大化", maximized ? "Restore height" : "Maximize")}
          className="w-7 h-7 rounded-md flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-secondary/50 transition-colors"
        >
          {maximized ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
        </button>
        <button
          type="button"
          aria-label={tr("关闭底部面板", "Close bottom panel")}
          onClick={onClose}
          className="w-7 h-7 rounded-md flex items-center justify-center text-muted-foreground hover:text-foreground hover:bg-secondary/50 transition-colors"
        >
          <X size={14} />
        </button>
      </div>

      {/* 内容区：任务流 / 日志 */}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto px-3 py-2"
        data-testid={`bottom-content-${tab}`}
      >
        {tab === "tasks" ? (
          executions.length === 0 ? (
            <p className="text-[13px] text-muted-foreground/60 italic py-4 text-center">
              {tr("暂无工具执行记录——开始一轮创作后，这里按时间顺序展示任务流。", "No tool executions yet — start a writing turn and the task stream will appear here.")}
            </p>
          ) : (
            <ToolExecutionSteps executions={executions} />
          )
        ) : (
          <div className="space-y-1">
            <div className="flex justify-end">
              <button
                type="button"
                onClick={() => void refetchLogs()}
                className="text-xs text-muted-foreground hover:text-foreground transition-colors"
              >
                {tr("刷新", "Refresh")}
              </button>
            </div>
            {logEntries.length === 0 ? (
              <p className="text-[13px] text-muted-foreground/60 italic py-4 text-center">
                {tr("暂无日志", "No log entries")}
              </p>
            ) : (
              logEntries.map((entry, index) => (
                <div key={index} className="flex gap-2 text-[12px] leading-5 font-mono">
                  {entry.timestamp && <span className="text-muted-foreground/50 shrink-0">{entry.timestamp}</span>}
                  {entry.level && (
                    <span className={`shrink-0 font-bold ${LEVEL_COLORS[entry.level] ?? "text-muted-foreground"}`}>
                      [{entry.level}]
                    </span>
                  )}
                  {entry.tag && <span className="text-muted-foreground/70 shrink-0">({entry.tag})</span>}
                  <span className="text-foreground/80 break-all">{entry.message}</span>
                </div>
              ))
            )}
          </div>
        )}
      </div>
    </section>
  );
}
