import { randomUUID } from "node:crypto";
import { appendFile, mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import type { AgentMessage } from "@earendil-works/pi-agent-core";
import {
  TranscriptEventSchema,
  type BranchMovedEvent,
  type TranscriptEvent,
} from "./session-transcript-schema.js";
import type { SessionKind, TranscriptRole } from "./session-transcript-schema.js";

const SESSIONS_DIR = ".inkos/sessions";
const appendQueues = new Map<string, Promise<void>>();

export function sessionsDir(projectRoot: string): string {
  return join(projectRoot, SESSIONS_DIR);
}

export function transcriptPath(projectRoot: string, sessionId: string): string {
  return join(sessionsDir(projectRoot), `${sessionId}.jsonl`);
}

export function legacyBookSessionPath(projectRoot: string, sessionId: string): string {
  return join(sessionsDir(projectRoot), `${sessionId}.json`);
}

export async function readTranscriptEvents(
  projectRoot: string,
  sessionId: string,
): Promise<TranscriptEvent[]> {
  let raw: string;
  try {
    raw = await readFile(transcriptPath(projectRoot, sessionId), "utf-8");
  } catch {
    return [];
  }

  const events: TranscriptEvent[] = [];
  for (const line of raw.split(/\r?\n/)) {
    if (!line.trim()) continue;
    try {
      const parsed = TranscriptEventSchema.safeParse(JSON.parse(line));
      if (parsed.success) events.push(parsed.data);
    } catch {
      continue;
    }
  }

  return events.sort((a, b) => a.seq - b.seq);
}

export async function nextTranscriptSeq(projectRoot: string, sessionId: string): Promise<number> {
  const events = await readTranscriptEvents(projectRoot, sessionId);
  return events.reduce((max, event) => Math.max(max, event.seq), 0) + 1;
}

function isBranchMoved(event: TranscriptEvent): event is BranchMovedEvent {
  return event.type === "branch_moved";
}

function eventParentSeq(event: TranscriptEvent): number | null | undefined {
  return (event as { parentSeq?: number | null }).parentSeq;
}

/**
 * R36 head replay（636 号）：按 seq 序重放 branch_moved 重建当前 head——
 * `toSeq` 即新 head（null = resetLeaf 置空），其余事件延伸链（head = seq）。
 * 与 Pi「leaf 不落盘」不同：head 从事件流可重建（O(n) 一次，装载路径已有
 * 全量读取），服务端跨进程重启/双引擎切换后分支状态不丢。
 */
export function transcriptHead(events: ReadonlyArray<TranscriptEvent>): number | null {
  let head: number | null = null;
  for (const event of events) {
    head = isBranchMoved(event) ? event.toSeq : event.seq;
  }
  return head;
}

/**
 * R36 active 链过滤（636 号）：从 head 沿 parentSeq 反向回溯出 root→head
 * 路径（升序返回，等于 seq 序的路径子集——父 seq 恒小于子 seq，append-only
 * 不变量）。弃用分支上的事件被剪除；未落 branch_moved 的纯 legacy 文件回溯
 * 退化为全量（缺省语义 = 线性链，父即 seq 序前一事件）——旧行为零改写。
 * 链断（parentSeq 指向不存在事件）时保留已收集后缀，恢复安全网同 553 号
 * compaction 容错先例。branch_moved 自身不入链（元事件，replay 时改写 head
 * 而非延伸链）。
 */
export function activeChainEvents(events: ReadonlyArray<TranscriptEvent>): TranscriptEvent[] {
  if (events.length === 0) return [];
  const sorted = [...events].sort((a, b) => a.seq - b.seq);
  const head = transcriptHead(sorted);
  if (head === null) return [];

  const bySeq = new Map<number, { event: TranscriptEvent; index: number }>();
  sorted.forEach((event, index) => bySeq.set(event.seq, { event, index }));

  const chain: TranscriptEvent[] = [];
  let cursor = bySeq.get(head);
  while (cursor && chain.length <= sorted.length) {
    const { event, index } = cursor;
    if (isBranchMoved(event)) break;
    chain.push(event);
    const parent = eventParentSeq(event);
    if (parent === undefined) {
      // legacy 线性语义：父 = seq 序前一事件
      cursor = index > 0 ? bySeq.get(sorted[index - 1].seq) : undefined;
    } else {
      cursor = parent === null ? undefined : bySeq.get(parent);
    }
  }
  return chain.reverse();
}

export async function appendTranscriptEvent(
  projectRoot: string,
  event: TranscriptEvent,
): Promise<void> {
  await appendTranscriptEvents(projectRoot, event.sessionId, () => [event]);
}

export async function appendTranscriptEvents(
  projectRoot: string,
  sessionId: string,
  buildEvents: (context: {
    readonly events: ReadonlyArray<TranscriptEvent>;
    readonly nextSeq: number;
  }) => ReadonlyArray<TranscriptEvent> | Promise<ReadonlyArray<TranscriptEvent>>,
): Promise<TranscriptEvent[]> {
  const key = `${projectRoot}:${sessionId}`;
  const previous = appendQueues.get(key) ?? Promise.resolve();
  let result: TranscriptEvent[] = [];

  const next = previous.then(async () => {
    const events = await readTranscriptEvents(projectRoot, sessionId);
    const nextSeq = events.reduce((max, event) => Math.max(max, event.seq), 0) + 1;
    const built = await buildEvents({ events, nextSeq });
    // R36（636 号）：parentSeq 由 append 助手统一戳记——链语义单一事实源，
    // 写入方零感知。branch_moved 改写 head 不延伸链；其余事件以戳记时 head
    // 为父并推进 head。per-session 串行队列保证戳记时 head 即追加序前驱。
    let head = transcriptHead(events);
    for (const event of built) {
      (event as { parentSeq?: number | null }).parentSeq = head;
      head = isBranchMoved(event) ? event.toSeq : event.seq;
    }
    result = built.map((event) => TranscriptEventSchema.parse(event));
    if (result.length === 0) return;

    await mkdir(sessionsDir(projectRoot), { recursive: true });
    await appendFile(
      transcriptPath(projectRoot, sessionId),
      `${result.map((event) => JSON.stringify(event)).join("\n")}\n`,
      "utf-8",
    );
  });

  appendQueues.set(key, next.catch(() => undefined));
  await next;
  return result;
}

function transcriptRoleForMessage(message: AgentMessage): TranscriptRole | null {
  if (!message || typeof message !== "object" || !("role" in message)) return null;
  const role = (message as { role?: unknown }).role;
  return role === "user" || role === "assistant" || role === "toolResult" || role === "system"
    ? role
    : null;
}

function messageTimestamp(message: AgentMessage): number {
  if (message && typeof message === "object") {
    const timestamp = (message as { timestamp?: unknown }).timestamp;
    if (typeof timestamp === "number" && Number.isFinite(timestamp) && timestamp >= 0) {
      return Math.floor(timestamp);
    }
  }
  return Date.now();
}

function toolCallIdForMessage(message: AgentMessage): string | undefined {
  if (!message || typeof message !== "object") return undefined;
  if ((message as { role?: unknown }).role === "toolResult") {
    const toolCallId = (message as { toolCallId?: unknown }).toolCallId;
    return typeof toolCallId === "string" && toolCallId.length > 0 ? toolCallId : undefined;
  }

  const content = (message as { content?: unknown }).content;
  if (!Array.isArray(content)) return undefined;
  const block = content.find(
    (item): item is { type: "toolCall"; id: string } =>
      !!item &&
      typeof item === "object" &&
      (item as { type?: unknown }).type === "toolCall" &&
      typeof (item as { id?: unknown }).id === "string",
  );
  return block?.id;
}

export async function appendManualSessionMessages(
  projectRoot: string,
  sessionId: string,
  messages: ReadonlyArray<AgentMessage>,
  input = "",
  options: {
    readonly sessionKind?: SessionKind;
    readonly legacyDisplay?: {
      readonly thinking?: string;
      readonly toolExecutions?: readonly unknown[];
    };
  } = {},
): Promise<void> {
  const persistedMessages = messages
    .map((message) => ({ message, role: transcriptRoleForMessage(message) }))
    .filter((entry): entry is { message: AgentMessage; role: TranscriptRole } => entry.role !== null);
  if (persistedMessages.length === 0) return;

  const requestId = randomUUID();
  await appendTranscriptEvents(projectRoot, sessionId, ({ nextSeq }) => {
    let seq = nextSeq;
    const events: TranscriptEvent[] = [{
      type: "request_started",
      version: 1,
      sessionId,
      requestId,
      seq: seq++,
      timestamp: Date.now(),
      ...(options.sessionKind ? { sessionKind: options.sessionKind } : {}),
      input,
    }];

    let parentUuid: string | null = null;
    let lastAssistantUuid: string | null = null;
    for (const { message, role } of persistedMessages) {
      const uuid = randomUUID();
      const isToolResult = role === "toolResult";
      const toolCallId = toolCallIdForMessage(message);
      const legacyDisplay = role === "assistant" && options.legacyDisplay
        ? {
            ...(options.legacyDisplay.thinking ? { thinking: options.legacyDisplay.thinking } : {}),
            ...(options.legacyDisplay.toolExecutions?.length
              ? { toolExecutions: [...options.legacyDisplay.toolExecutions] }
              : {}),
          }
        : undefined;
      events.push({
        type: "message",
        version: 1,
        sessionId,
        requestId,
        uuid,
        parentUuid: isToolResult && lastAssistantUuid ? lastAssistantUuid : parentUuid,
        seq: seq++,
        role,
        timestamp: messageTimestamp(message),
        ...(toolCallId ? { toolCallId } : {}),
        ...(isToolResult && lastAssistantUuid
          ? { sourceToolAssistantUuid: lastAssistantUuid }
          : {}),
        ...(legacyDisplay && (legacyDisplay.thinking || legacyDisplay.toolExecutions?.length)
          ? { legacyDisplay }
          : {}),
        message,
      });
      if (role === "assistant") lastAssistantUuid = uuid;
      parentUuid = uuid;
    }

    events.push({
      type: "request_committed",
      version: 1,
      sessionId,
      requestId,
      seq,
      timestamp: Date.now(),
    });
    return events;
  });
}
