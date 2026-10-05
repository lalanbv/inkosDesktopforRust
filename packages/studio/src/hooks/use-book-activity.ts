import type { SSEMessage } from "./use-sse";

const START_EVENTS = new Set(["write:start", "draft:start"]);
const TERMINAL_EVENTS = new Set(["write:complete", "write:error", "draft:complete", "draft:error"]);
const BOOK_REFRESH_EVENTS = new Set([
  "write:complete",
  "write:error",
  "draft:complete",
  "draft:error",
  "rewrite:complete",
  "rewrite:error",
  "revise:complete",
  "revise:error",
  "audit:complete",
  "audit:error",
]);

const BOOK_COLLECTION_REFRESH_EVENTS = new Set([
  "book:created",
  "book:deleted",
  "book:error",
  "write:complete",
  "write:error",
  "draft:complete",
  "draft:error",
  "rewrite:complete",
  "rewrite:error",
  "revise:complete",
  "revise:error",
  "audit:complete",
  "audit:error",
]);

const DAEMON_STATUS_REFRESH_EVENTS = new Set([
  "daemon:started",
  "daemon:stopped",
  "daemon:error",
]);

export interface BookActivity {
  readonly writing: boolean;
  readonly drafting: boolean;
  readonly lastError: string | null;
}

export interface SidebarBookSummary {
  readonly id: string;
  readonly title: string;
  readonly genre: string;
  readonly status: string;
  readonly chaptersWritten: number;
}

function getBookId(message: SSEMessage): string | null {
  const data = message.data as { bookId?: unknown } | null;
  return typeof data?.bookId === "string" ? data.bookId : null;
}

function getBookSummary(message: SSEMessage): SidebarBookSummary | null {
  const data = message.data as { book?: unknown } | null;
  const book = data?.book;
  if (!book || typeof book !== "object") return null;
  const candidate = book as Partial<SidebarBookSummary>;
  if (
    typeof candidate.id !== "string" ||
    typeof candidate.title !== "string" ||
    typeof candidate.genre !== "string" ||
    typeof candidate.status !== "string" ||
    typeof candidate.chaptersWritten !== "number"
  ) {
    return null;
  }
  return {
    id: candidate.id,
    title: candidate.title,
    genre: candidate.genre,
    status: candidate.status,
    chaptersWritten: candidate.chaptersWritten,
  };
}

export function deriveActiveBookIds(messages: ReadonlyArray<SSEMessage>): ReadonlySet<string> {
  const active = new Set<string>();

  for (const message of messages) {
    const bookId = getBookId(message);
    if (!bookId) continue;

    if (START_EVENTS.has(message.event)) {
      active.add(bookId);
      continue;
    }

    if (TERMINAL_EVENTS.has(message.event)) {
      active.delete(bookId);
    }
  }

  return active;
}

export function deriveBookActivity(messages: ReadonlyArray<SSEMessage>, bookId: string): BookActivity {
  let writing = false;
  let drafting = false;
  let lastError: string | null = null;

  for (const message of messages) {
    if (getBookId(message) !== bookId) continue;

    const data = message.data as { error?: unknown } | null;

    switch (message.event) {
      case "write:start":
        writing = true;
        lastError = null;
        break;
      case "write:complete":
        writing = false;
        lastError = null;
        break;
      case "write:error":
        writing = false;
        lastError = typeof data?.error === "string" ? data.error : "Unknown error";
        break;
      case "draft:start":
        drafting = true;
        lastError = null;
        break;
      case "draft:complete":
        drafting = false;
        lastError = null;
        break;
      case "draft:error":
        drafting = false;
        lastError = typeof data?.error === "string" ? data.error : "Unknown error";
        break;
      default:
        break;
    }
  }

  return { writing, drafting, lastError };
}

/**
 * 书籍页 write-next 的稳定伪会话 id（176 号）：write-next 请求体 sessionId 与
 * SSE 快照恢复/停止按钮共用同一锚点——引擎侧检查点（174 号）与真实可停止
 * （175 号）在此激活。形如 `book:{bookId}:write`，不对应聊天会话目录
 * （task-store 只按它做文件名键；abort 端点对无会话条目自然无害）。
 */
export function writeTaskSessionId(bookId: string): string {
  return `book:${bookId}:write`;
}

/**
 * 用户主动停止 write-next 的双端中性文案（642 号）：TS server.ts 直连端点与
 * Rust write_next_route 字面同形，停止以 write:error{error:本文案} 广播。
 */
export const WRITE_STOPPED_MESSAGE = "写作已按您的要求停止。";

/**
 * 用户主动停止属中性结果、不以失败呈现（188 号）。644 号走查实证：642 号把
 * 停止广播换成中性文案后，UI 侧遗留的 `includes("Operation aborted")` 判定
 * 不再命中——中性文案被红色失败分支包裹自相矛盾。判定同时认双端中性文案
 * 与遗留 abort 字面（兼容旧快照/旧广播形态）。
 */
export function isWriteStoppedMessage(error: string | null | undefined): boolean {
  if (!error) return false;
  return error.includes(WRITE_STOPPED_MESSAGE) || error.includes("Operation aborted");
}

export function shouldRefetchBookView(message: SSEMessage, bookId: string): boolean {
  return getBookId(message) === bookId && BOOK_REFRESH_EVENTS.has(message.event);
}

export function shouldRefetchBookCollections(message: SSEMessage | undefined): boolean {
  return Boolean(message && BOOK_COLLECTION_REFRESH_EVENTS.has(message.event));
}

export function shouldRefetchDaemonStatus(message: SSEMessage | undefined): boolean {
  return Boolean(message && DAEMON_STATUS_REFRESH_EVENTS.has(message.event));
}

export function applyBookCollectionEvent(
  books: ReadonlyArray<SidebarBookSummary>,
  message: SSEMessage | undefined,
): ReadonlyArray<SidebarBookSummary> | null {
  if (!message) return null;

  if (message.event === "book:created") {
    const book = getBookSummary(message);
    if (!book) return null;
    const existingIndex = books.findIndex((candidate) => candidate.id === book.id);
    if (existingIndex < 0) {
      return [...books, book];
    }
    return books.map((candidate, index) => index === existingIndex ? book : candidate);
  }

  if (message.event === "book:deleted") {
    const bookId = getBookId(message);
    if (!bookId) return null;
    return books.filter((book) => book.id !== bookId);
  }

  return null;
}
