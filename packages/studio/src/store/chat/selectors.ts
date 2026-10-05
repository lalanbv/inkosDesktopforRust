import type { ChatState } from "./types";

const EMPTY_MESSAGES: readonly [] = [];

export const chatSelectors = {
  activeSession: (s: ChatState) => (s.activeSessionId ? s.sessions[s.activeSessionId] ?? null : null),
  activeMessages: (s: ChatState) =>
    (s.activeSessionId ? s.sessions[s.activeSessionId]?.messages : undefined) ?? EMPTY_MESSAGES,
  isActiveSessionStreaming: (s: ChatState) => Boolean(s.activeSessionId && s.sessions[s.activeSessionId]?.isStreaming),
  // 聊天轮本身是否在流式中；后台任务运行期间为 false（此时仍可继续发消息）。
  isActiveSessionChatStreaming: (s: ChatState) =>
    Boolean(s.activeSessionId && s.sessions[s.activeSessionId]?.isChatStreaming),
  // 上一条失败的聊天轮发送记录；存在且非聊天流式中时 UI 显示"重试"按钮。
  activeSessionLastFailedSend: (s: ChatState) =>
    (s.activeSessionId ? s.sessions[s.activeSessionId]?.lastFailedSend : undefined) ?? null,
  // 648 号：当前会话的 write_next 生产任务执行中（direct-write_next-* 工具卡
  // 处于 running/processing）——QuickActions 的「写下一章」chip 据此翻转为
  // 就地停止（停止走 abortSession → /sessions/:id/abort，后端经
  // reservedProductionSessions→activeConfirmedTasks 命中任务控制器）。
  activeSessionWriteNextRunning: (s: ChatState) => {
    const session = s.activeSessionId ? s.sessions[s.activeSessionId] : undefined;
    if (!session) return false;
    return session.messages.some((m) =>
      (m.parts ?? []).some((p) =>
        p.type === "tool"
        && p.execution.id.startsWith("direct-write_next-")
        && (p.execution.status === "running" || p.execution.status === "processing")));
  },
  isEmpty: (s: ChatState) =>
    ((s.activeSessionId ? s.sessions[s.activeSessionId]?.messages.length : 0) ?? 0) === 0
    && !Boolean(s.activeSessionId && s.sessions[s.activeSessionId]?.isStreaming),
};
