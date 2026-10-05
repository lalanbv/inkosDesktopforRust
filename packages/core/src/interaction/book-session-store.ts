import { readdir, unlink } from "node:fs/promises";
import { createBookSession } from "./session.js";
import type { BookSession, PlayMode, SessionKind } from "./session.js";
import {
  activeChainEvents,
  appendTranscriptEvents,
  legacyBookSessionPath,
  readTranscriptEvents,
  sessionsDir,
  transcriptHead,
  transcriptPath,
} from "./session-transcript.js";
import {
  migrateLegacyBookSessionToTranscript,
  readLegacyBookSession,
} from "./session-transcript-legacy.js";
import { deriveBookSessionFromTranscript } from "./session-transcript-restore.js";

/**
 * 从 messages 数组里取第一条 user 消息，裁剪成 ≤20 字的单行字符串。
 * 用于把用户首条提问作为会话标题。
 */
export function extractFirstUserMessageTitle(messages: unknown): string | null {
  if (!Array.isArray(messages)) return null;
  for (const message of messages) {
    if (!message || typeof message !== "object") continue;
    if ((message as { role?: unknown }).role !== "user") continue;
    const content = (message as { content?: unknown }).content;
    if (typeof content !== "string") return null;
    const oneLine = content.trim().replace(/\s+/g, " ");
    if (oneLine.length === 0) return null;
    return oneLine.length > 20 ? `${oneLine.slice(0, 20)}…` : oneLine;
  }
  return null;
}

export class SessionAlreadyMigratedError extends Error {
  constructor(sessionId: string, currentBookId: string) {
    super(`Session "${sessionId}" is already bound to book "${currentBookId}"`);
    this.name = "SessionAlreadyMigratedError";
  }
}

export async function loadBookSession(
  projectRoot: string,
  sessionId: string,
): Promise<BookSession | null> {
  const transcriptSession = await deriveBookSessionFromTranscript(projectRoot, sessionId);
  if (transcriptSession) return transcriptSession;

  const legacySession = await readLegacyBookSession(projectRoot, sessionId);
  if (!legacySession) return null;

  await migrateLegacyBookSessionToTranscript(projectRoot, legacySession);
  return await deriveBookSessionFromTranscript(projectRoot, sessionId) ?? legacySession;
}

async function appendSessionCreatedEvent(
  projectRoot: string,
  session: BookSession,
): Promise<void> {
  await appendTranscriptEvents(projectRoot, session.sessionId, ({ events, nextSeq }) => {
    if (events.some((event) => event.type === "session_created")) return [];
    return [{
      type: "session_created",
      version: 1,
      sessionId: session.sessionId,
      seq: nextSeq,
      timestamp: session.createdAt,
      bookId: session.bookId,
      ...(session.sessionKind ? { sessionKind: session.sessionKind } : {}),
      ...(session.playMode ? { playMode: session.playMode } : {}),
      title: session.title,
      createdAt: session.createdAt,
      updatedAt: session.updatedAt,
    }];
  });
}

async function appendSessionMetadataUpdatedEvent(
  projectRoot: string,
  sessionId: string,
  metadata: {
    readonly bookId?: string | null;
    readonly sessionKind?: SessionKind;
    readonly playMode?: PlayMode;
    readonly title?: string | null;
    readonly updatedAt: number;
  },
): Promise<void> {
  await appendTranscriptEvents(projectRoot, sessionId, ({ nextSeq }) => [{
    type: "session_metadata_updated",
    version: 1,
    sessionId,
    seq: nextSeq,
    timestamp: metadata.updatedAt,
    updatedAt: metadata.updatedAt,
    ...("bookId" in metadata ? { bookId: metadata.bookId } : {}),
    ...(metadata.sessionKind ? { sessionKind: metadata.sessionKind } : {}),
    ...(metadata.playMode ? { playMode: metadata.playMode } : {}),
    ...("title" in metadata ? { title: metadata.title } : {}),
  }]);
}

export async function persistBookSession(
  projectRoot: string,
  session: BookSession,
): Promise<void> {
  const events = await readTranscriptEvents(projectRoot, session.sessionId);
  if (events.length === 0) {
    if (session.messages.length === 0) {
      await appendSessionCreatedEvent(projectRoot, session);
      return;
    }
    await migrateLegacyBookSessionToTranscript(projectRoot, session);
    return;
  }

  await appendSessionMetadataUpdatedEvent(projectRoot, session.sessionId, {
    bookId: session.bookId,
    ...(session.sessionKind ? { sessionKind: session.sessionKind } : {}),
    ...(session.playMode ? { playMode: session.playMode } : {}),
    title: session.title,
    updatedAt: session.updatedAt,
  });
}

export interface BookSessionSummary {
  readonly sessionId: string;
  readonly bookId: string | null;
  readonly sessionKind?: SessionKind;
  readonly playMode?: PlayMode;
  readonly title: string | null;
  readonly messageCount: number;
  readonly head: number | null;
  readonly branchCount: number;
  readonly createdAt: number;
  readonly updatedAt: number;
}

export async function listBookSessions(
  projectRoot: string,
  bookId: string | null,
): Promise<ReadonlyArray<BookSessionSummary>> {
  const dir = sessionsDir(projectRoot);
  let files: string[];
  try {
    files = await readdir(dir);
  } catch {
    return [];
  }

  const sessionIds = new Set<string>();
  for (const file of files) {
    if (file.endsWith(".jsonl")) {
      sessionIds.add(file.slice(0, -".jsonl".length));
    } else if (file.endsWith(".json")) {
      sessionIds.add(file.slice(0, -".json".length));
    }
  }

  const summaries = await Promise.all(
    [...sessionIds].map(async (sessionId): Promise<BookSessionSummary | null> => {
      try {
        const session = await loadBookSession(projectRoot, sessionId);
        if (!session || session.bookId !== bookId) return null;

        return {
          sessionId: session.sessionId,
          bookId: session.bookId,
          sessionKind: session.sessionKind,
          playMode: session.playMode,
          title: session.title,
          messageCount: session.messages.length,
          head: session.head ?? null,
          branchCount: session.branchCount ?? 0,
          createdAt: session.createdAt,
          updatedAt: session.updatedAt,
        };
      } catch {
        return null;
      }
    }),
  );

  return summaries
    .filter((summary): summary is BookSessionSummary => summary !== null)
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

export async function renameBookSession(
  projectRoot: string,
  sessionId: string,
  title: string,
): Promise<BookSession | null> {
  const session = await loadBookSession(projectRoot, sessionId);
  if (!session) return null;
  const updatedAt = Date.now();
  await appendSessionMetadataUpdatedEvent(projectRoot, sessionId, { title, updatedAt });
  return loadBookSession(projectRoot, sessionId);
}

export async function deleteBookSession(
  projectRoot: string,
  sessionId: string,
): Promise<void> {
  await Promise.all([
    unlink(transcriptPath(projectRoot, sessionId)).catch(() => undefined),
    unlink(legacyBookSessionPath(projectRoot, sessionId)).catch(() => undefined),
  ]);
}

/** R36 branch 结果（636 号）：移动后的 head 与累计分支次数。 */
export interface BranchBookSessionResult {
  readonly head: number | null;
  readonly branchCount: number;
}

/**
 * R36 会话树化（636 号）：分支 = 追加一条 branch_moved 事件（append-only 不
 * 破坏）+ head 指针移动（replay 重建）。`toSeq` = 新 head 的事件 seq；null =
 * resetLeaf（head 置空，后续写入开新链根）。弃用路径零删改、可再 branch 回
 * 去。调用方负责忙判定（轮进行中不 branch，保证单请求事件整体在同一链上）。
 * 会话不存在（无 transcript）返回 null。
 */
export async function branchBookSession(
  projectRoot: string,
  sessionId: string,
  toSeq: number | null,
): Promise<BranchBookSessionResult | null> {
  let appended = 0;
  await appendTranscriptEvents(projectRoot, sessionId, ({ events, nextSeq }) => {
    if (events.length === 0) return [];
    const branchTargets = new Set(events.map((event) => event.seq));
    if (toSeq !== null && !branchTargets.has(toSeq)) {
      throw new BranchTargetNotFoundError(toSeq);
    }
    appended = 1;
    const fromSeq = transcriptHead(events);
    return [{
      type: "branch_moved",
      version: 1,
      sessionId,
      seq: nextSeq,
      timestamp: Date.now(),
      fromSeq,
      toSeq,
    }];
  });
  if (!appended) return null;

  const events = await readTranscriptEvents(projectRoot, sessionId);
  return {
    head: transcriptHead(events),
    branchCount: events.filter((event) => event.type === "branch_moved").length,
  };
}

/** branch 目标 seq 不在事件流中（服务端映射 400 INVALID_BRANCH_TARGET）。 */
export class BranchTargetNotFoundError extends Error {
  constructor(readonly toSeq: number) {
    super(`Branch target seq ${toSeq} not found in transcript`);
    this.name = "BranchTargetNotFoundError";
  }
}

/**
 * R36 分支点读面（638 号）：一个可回退分支点 = 一条已提交请求的
 * request_committed 事件（该轮事件 seq 最大的收尾点——消息先于 commit 落盘，
 * branch 到它 = 「保留到该轮结束」）。preview 取同 requestId 的
 * request_started.input（每轮必有、纯文本，免解析 message 载荷形状）；
 * onActiveChain 标记该点是否在当前 head 链上（false = 已弃用分支，可切回）。
 * 失败轮（request_failed 无 committed）不是分支点。会话不存在返回 null。
 */
export interface SessionBranchPoint {
  readonly seq: number;
  readonly requestId: string;
  readonly timestamp: number;
  readonly preview: string;
  readonly onActiveChain: boolean;
}

export interface SessionBranchPointsResult {
  readonly sessionId: string;
  readonly head: number | null;
  readonly branchCount: number;
  readonly points: ReadonlyArray<SessionBranchPoint>;
}

export async function deriveSessionBranchPoints(
  projectRoot: string,
  sessionId: string,
): Promise<SessionBranchPointsResult | null> {
  const events = await readTranscriptEvents(projectRoot, sessionId);
  if (events.length === 0) return null;
  const chainSeqs = new Set(activeChainEvents(events).map((event) => event.seq));
  const startedInputs = new Map<string, string>();
  for (const event of events) {
    if (event.type === "request_started") startedInputs.set(event.requestId, event.input);
  }
  const points: SessionBranchPoint[] = [];
  for (const event of events) {
    if (event.type !== "request_committed") continue;
    points.push({
      seq: event.seq,
      requestId: event.requestId,
      timestamp: event.timestamp,
      preview: startedInputs.get(event.requestId) ?? "",
      onActiveChain: chainSeqs.has(event.seq),
    });
  }
  return {
    sessionId,
    head: transcriptHead(events),
    branchCount: events.filter((event) => event.type === "branch_moved").length,
    points,
  };
}

export async function migrateBookSession(
  projectRoot: string,
  sessionId: string,
  newBookId: string,
): Promise<BookSession | null> {
  const session = await loadBookSession(projectRoot, sessionId);
  if (!session) return null;
  if (session.bookId !== null) {
    throw new SessionAlreadyMigratedError(sessionId, session.bookId);
  }

  await appendSessionMetadataUpdatedEvent(projectRoot, sessionId, {
    bookId: newBookId,
    sessionKind: "book",
    updatedAt: Date.now(),
  });
  return loadBookSession(projectRoot, sessionId);
}

export async function createAndPersistBookSession(
  projectRoot: string,
  bookId: string | null,
  sessionId?: string,
  sessionKind?: SessionKind,
  options?: { readonly playMode?: PlayMode },
): Promise<BookSession> {
  // 如果指定了 sessionId 且对应文件已存在，视为幂等操作直接返回（支持"用户发消息时才持久化 draft"流程）
  if (sessionId) {
    const existing = await loadBookSession(projectRoot, sessionId);
    if (existing) {
      if ((sessionKind && existing.sessionKind !== sessionKind) || (options?.playMode && existing.playMode !== options.playMode)) {
        await appendSessionMetadataUpdatedEvent(projectRoot, sessionId, {
          ...(sessionKind ? { sessionKind } : {}),
          ...(options?.playMode ? { playMode: options.playMode } : {}),
          updatedAt: Date.now(),
        });
        return await loadBookSession(projectRoot, sessionId) ?? existing;
      }
      return existing;
    }
  }
  const session = createBookSession(bookId, sessionId, sessionKind, options);
  await appendSessionCreatedEvent(projectRoot, session);
  return session;
}
