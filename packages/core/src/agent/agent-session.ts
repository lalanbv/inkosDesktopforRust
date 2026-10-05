import { createHash, randomUUID } from "node:crypto";
import { Agent } from "@earendil-works/pi-agent-core";
import type { AgentEvent, AgentMessage } from "@earendil-works/pi-agent-core";
// R30b 正统化：getModel 走 providers/all 静态目录读、env key 走自持解析
// （替代 0.87 deprecated 的 ./compat 子路径）。
import { getBuiltinModel } from "@earendil-works/pi-ai/providers/all";
import { getEnvApiKey } from "../llm/pi-env-keys.js";
import { createAssistantMessageEventStream, isContextOverflow } from "@earendil-works/pi-ai";
import type {
  Model,
  Api,
  AssistantMessage,
  AssistantMessageEventStream,
  Context as PiContext,
  ImageContent,
  Message,
  SimpleStreamOptions,
  ToolResultMessage,
  UserMessage,
} from "@earendil-works/pi-ai";
import type { PipelineRunner } from "../pipeline/runner.js";
import { buildAgentSystemPrompt } from "./agent-system-prompt.js";
import { buildChatToolSet, isProductionMutationToolName } from "./chat-tool-set.js";
import { createBookContextTransform, createInteractiveFilmContextTransform } from "./context-transform.js";
import { TokenMeter, type TokenMeterSnapshot } from "../utils/token-meter.js";
import {
  appendTranscriptEvents,
  readTranscriptEvents,
} from "../interaction/session-transcript.js";
import {
  TOOL_RESULT_BRIDGE_TEXT,
  adaptRestoredAgentMessagesForModel,
  appendRestoredHistoryBoundary,
  restoreAgentMessagesFromTranscript,
} from "../interaction/session-transcript-restore.js";
import type { TranscriptEvent, TranscriptRole } from "../interaction/session-transcript-schema.js";
import type { PlayMode, SessionKind } from "../interaction/session.js";
import type { ActionPayload, ActionSource, RequestedIntent } from "../interaction/action-envelope.js";
import type { ContextCompressionCallback } from "../models/context-compression.js";
import {
  createSkillRegistry,
  loadAvailableAgentSkills,
  resolveProductionSkillActivations,
  type ProductionSkillCapability,
} from "../skills/index.js";
import { assertSafeBookId } from "../utils/book-id.js";
import { PlayStore } from "../play/play-store.js";
import {
  assistantInvokesSkill,
  createUseSkillTool,
  sanitizeSkillTurnMessage,
  type ActivatedSkillGuidance,
} from "./skill-tool.js";
import { opaqueConversationId, runWithAgentTrajectory } from "../llm/agent-trajectory.js";
import { guardedAgentStream, guardedCompleteStream } from "./pi-stream.js";
import { maybeCompactSession, shouldCompactSession, scanSessionForCompaction } from "./session-compaction.js";
import { ContextWindowExceededError } from "../llm/provider.js";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface AgentSessionConfig {
  /** Unique session identifier (typically the BookSession id). */
  sessionId: string;
  /** Book ID, or null if in "new book" mode. */
  bookId: string | null;
  /** Studio conversation surface. Used to narrow the visible tools. */
  sessionKind?: SessionKind;
  /** Play interaction mode chosen by the player at launch (guided = choice-only, open = free text). */
  playMode?: PlayMode;
  /** Where this turn came from. Button/slash turns can execute confirmed production actions. */
  actionSource?: ActionSource;
  /** Explicit user-confirmed action requested by the UI/command surface. */
  requestedIntent?: RequestedIntent;
  /** Structured execution arguments confirmed by the UI/command surface. */
  actionPayload?: ActionPayload;
  /** User/UI-forced Agent Skills for this turn, e.g. @open-world-play. */
  requestedSkills?: ReadonlyArray<string>;
  /** Agent Skills explicitly disabled for this turn. */
  disabledSkills?: ReadonlyArray<string>;
  /** Language for the system prompt. */
  language: string;
  /** PipelineRunner for sub-agent tool delegation. */
  pipeline: PipelineRunner;
  /** Project root directory (books/ lives under this). */
  projectRoot: string;
  /** pi-ai Model to use, or provider+modelId to resolve via getModel. */
  model: Model<Api> | { provider: string; modelId: string };
  // 607 号：服务「流式响应」开关关闭（stream:false，108 号）时聊天走非流式完成——
  // 此前聊天路径恒流式忽略该偏好（层 3 stream 双端分歧，606 号备案）。
  // undefined / true = 默认流式。
  streamPreference?: boolean;
    /** Optional API key. When omitted, falls back to env-based key lookup. */
  apiKey?: string;
  /** Allow the read tool to read absolute paths outside projectRoot/books. Defaults to false; set INKOS_AGENT_ALLOW_SYSTEM_READ=1 to enable. */
  allowSystemFileRead?: boolean;
  /** Optional listener for streaming events (for SSE forwarding). */
  onEvent?: (event: AgentEvent) => void;
  /** Optional listener for context compression lifecycle events. */
  onContextCompression?: ContextCompressionCallback;
  /** Attachments uploaded with this user turn. Text is injected as protected user context; images use pi-ai ImageContent. */
  attachments?: ReadonlyArray<AgentSessionAttachment>;
  /**
   * Status block for a production task running in the background of this session
   * (e.g. a confirmed short-fiction run). Appended to the system prompt so the
   * agent can answer progress questions instead of claiming nothing is running.
   * Changing this value evicts the cached Agent so the prompt stays current.
   */
  backgroundTaskContext?: string;
  /**
   * Remove book/artifact-mutating production tools from this turn's tool table
   * (a confirmed production task is already running in this session, so a
   * parallel chat turn must not mutate the same book concurrently). Read-style
   * tools, research/material tools, and propose_action stay available —
   * confirmed actions started via propose_action are gated host-side anyway.
   * Changing this value evicts the cached Agent so the tool table stays current.
   */
  suppressProductionTools?: boolean;
}

export interface AgentSessionResult {
  /** Extracted text from the final assistant message. */
  responseText: string;
  /** Full raw Agent conversation history. */
  messages: AgentMessage[];
  /** Upstream model error surfaced by pi-agent-core, if the final assistant turn failed. */
  errorMessage?: string;
  /** G8a/333 号 AI 实况：最终助手消息的 token 用量（pi usage 权威值）。 */
  usage?: { input: number; output: number; totalTokens: number };
  /** G8a/333 号 AI 实况：本轮思考流聚合文本（thinking 块拼接，可为空串）。 */
  thinking?: string;
  /** G8a/333 号 AI 实况：首包/总耗时（毫秒；首包=起点到首个模型输出事件）。 */
  timings: { firstTokenMs: number; totalMs: number };
  /**
   * R41 计量面（564 号）：本轮末请求面 token 计量快照（表语义 = 全量
   * state.messages 回放 = 下一轮请求面；usage 锚点校准）。
   */
  contextMeter?: TokenMeterSnapshot;
}

export interface AgentSessionAttachment {
  readonly id: string;
  readonly filename: string;
  readonly mimeType: string;
  readonly size: number;
  readonly storedPath?: string;
  readonly text?: string;
  readonly image?: {
    readonly data: string;
    readonly mimeType: string;
  };
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

interface CachedAgent {
  agent: Agent;
  sessionId: string;
  projectRoot: string;
  bookId: string | null;
  sessionKind: SessionKind;
  actionSource: NonNullable<AgentSessionConfig["actionSource"]>;
  requestedIntent: AgentSessionConfig["requestedIntent"];
  actionPayloadKey: string;
  skillResolutionKey: string;
  turnSkills: Map<string, ActivatedSkillGuidance>;
  playWorldExists: boolean;
  language: string;
  modelIdentity: string;
  apiKey: string | undefined;
  allowSystemFileRead: boolean;
  backgroundTaskContext: string | undefined;
  suppressProductionTools: boolean;
  streamPreference: boolean | undefined;
  currentAttachmentPaths: string[];
  lastCommittedSeq: number;
  lastActive: number;
}

const agentCache = new Map<string, CachedAgent>();
const agentSessionQueues = new Map<string, Promise<void>>();

// 640 号 pending-abort（排队态取消）：abort 时对「已入队未受理」的聊天轮置
// 带序数的标记，受理点按入队序裁决——序 < 标记序的排队轮受理即中止（与在飞
// 中止契约同形：transcript started+user+failed{"aborted"} + 500 aborted 响应，
// 客户端 624 契约零改动）。用全局单调序数而非毫秒时间戳：同一毫秒内「先入队
// 后置标记」与「先置标记后入队」（换向流）无法靠时钟区分，序数给出严格全序。
// 消费语义：受理点取标记即清除（FIFO 下第一个受理者是唯一可能早于标记的轮；
// 未命中说明标记已无目标，同样清除自愈）。
const sessionQueueSeqCounter = { seq: 0 };
const pendingSessionAborts = new Map<string, number>();

function nextSessionQueueSeq(): number {
  sessionQueueSeqCounter.seq += 1;
  return sessionQueueSeqCounter.seq;
}

/** abort 路由在会话忙（队列在飞或排队）时置位；受理点按入队序裁决。 */
export function markPendingSessionAbort(projectRoot: string, sessionId: string): void {
  pendingSessionAborts.set(sessionQueueKey(projectRoot, sessionId), nextSessionQueueSeq());
}

/** 受理点裁决：取走标记；入队序早于标记序返回 true（本轮受理即中止）。 */
export function takePendingSessionAbort(projectRoot: string, sessionId: string, enqueuedSeq: number): boolean {
  const key = sessionQueueKey(projectRoot, sessionId);
  const markedSeq = pendingSessionAborts.get(key);
  if (markedSeq === undefined) return false;
  pendingSessionAborts.delete(key);
  return enqueuedSeq < markedSeq;
}

/** TTL for cached agents: 5 minutes. */
const CACHE_TTL_MS = 5 * 60 * 1000;

const EMPTY_USAGE = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
};

/** Cleanup interval handle (lazy-started). */
let cleanupTimer: ReturnType<typeof setInterval> | null = null;

function ensureCleanupTimer(): void {
  if (cleanupTimer) return;
  cleanupTimer = setInterval(() => {
    const now = Date.now();
    for (const [id, entry] of agentCache) {
      if (now - entry.lastActive > CACHE_TTL_MS) {
        agentCache.delete(id);
      }
    }
    // Stop the timer when nothing left to watch.
    if (agentCache.size === 0 && cleanupTimer) {
      clearInterval(cleanupTimer);
      cleanupTimer = null;
    }
  }, 60_000); // run every 60 s
  // Allow the process to exit even if this timer is alive.
  if (cleanupTimer && typeof cleanupTimer === "object" && "unref" in cleanupTimer) {
    cleanupTimer.unref();
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function resolveModel(spec: AgentSessionConfig["model"]): Model<Api> {
  if (!spec) {
    throw new Error("Model is required but was undefined. Check LLM configuration.");
  }
  if (typeof spec === "object" && "id" in spec && "api" in spec) {
    // Already a Model object.
    return spec as Model<Api>;
  }
  const { provider, modelId } = spec as { provider: string; modelId: string };
  if (!provider || !modelId) {
    throw new Error(`Invalid model spec: provider=${provider}, modelId=${modelId}`);
  }
  return getBuiltinModel(provider as any, modelId as any);
}

function envFlagEnabled(value: string | undefined, defaultValue: boolean): boolean {
  if (value === undefined) return defaultValue;
  if (value === "1" || value.toLowerCase() === "true") return true;
  if (value === "0" || value.toLowerCase() === "false") return false;
  return defaultValue;
}

function agentModelIdentity(model: Model<Api>): string {
  return [
    model.api,
    model.provider,
    model.baseUrl ?? "",
    model.id,
  ].join("::");
}

function actionPayloadCacheKey(payload: ActionPayload | undefined): string {
  return payload ? JSON.stringify(payload) : "";
}

function skillResolutionCacheKey(value: {
  readonly usedSkills: ReadonlyArray<{
    readonly id: string;
    readonly source?: string;
    readonly body?: string;
  }>;
  readonly forcedSkillIds: ReadonlyArray<string>;
  readonly missingSkillIds: ReadonlyArray<string>;
  readonly disabledSkillIds: ReadonlyArray<string>;
  readonly availableSkills: ReadonlyArray<{
    readonly id: string;
    readonly name: string;
    readonly description: string;
    readonly body?: string;
    readonly baseDir?: string;
  }>;
}): string {
  return createHash("sha256").update(JSON.stringify({
    used: value.usedSkills.map((skill) => ({
      id: skill.id,
      source: skill.source,
      body: skill.body ?? "",
    })),
    forced: value.forcedSkillIds,
    missing: value.missingSkillIds,
    disabled: value.disabledSkillIds,
    available: value.availableSkills.map((skill) => ({
      id: skill.id,
      name: skill.name,
      description: skill.description,
      body: skill.body ?? "",
      baseDir: skill.baseDir ?? "",
    })),
  })).digest("hex");
}

function sessionQueueKey(projectRoot: string, sessionId: string): string {
  return `${projectRoot}\0${sessionId}`;
}

function agentCacheKey(projectRoot: string, sessionId: string): string {
  return sessionQueueKey(projectRoot, sessionId);
}

function buildAttachmentUserBlock(attachments: ReadonlyArray<AgentSessionAttachment> | undefined, language: string): string {
  if (!attachments?.length) return "";
  const isEn = language === "en";
  const lines = [
    isEn
      ? "\n\n## Uploaded Files (host-provided, user-authorized)"
      : "\n\n## 用户上传文件（宿主已接收，用户授权本轮使用）",
  ];
  for (const attachment of attachments) {
    lines.push(`\n### ${attachment.filename}`);
    lines.push(`- id: ${attachment.id}`);
    lines.push(`- mime: ${attachment.mimeType || "application/octet-stream"}`);
    lines.push(`- size: ${attachment.size}`);
    if (attachment.storedPath) lines.push(`- stored_path: ${attachment.storedPath}`);
    if (attachment.text) {
      lines.push(isEn ? "\nContent:" : "\n内容：");
      lines.push("```");
      lines.push(attachment.text);
      lines.push("```");
    } else if (attachment.image) {
      lines.push(isEn ? "- image: attached as multimodal input" : "- 图片：已作为多模态输入附加");
    } else {
      lines.push(isEn
        ? "- content: stored only; no extractor is available for this MIME type yet"
        : "- 内容：已保存；当前 MIME 类型暂未配置文本抽取器");
    }
  }
  return lines.join("\n");
}

function attachmentImages(attachments: ReadonlyArray<AgentSessionAttachment> | undefined): ImageContent[] {
  return (attachments ?? [])
    .filter((attachment) => attachment.image)
    .map((attachment) => ({
      type: "image",
      data: attachment.image!.data,
      mimeType: attachment.image!.mimeType,
    }));
}

function localAssistantStopStream(model: Model<Api>): AssistantMessageEventStream {
  const stream = createAssistantMessageEventStream();
  const message: AssistantMessage = {
    role: "assistant",
    content: [],
    api: model.api,
    provider: model.provider,
    model: model.id,
    usage: EMPTY_USAGE,
    stopReason: "stop",
    timestamp: Date.now(),
  };
  queueMicrotask(() => {
    stream.push({ type: "done", reason: "stop", message });
    stream.end(message);
  });
  return stream;
}

export function isTerminalProductionToolName(toolName: unknown): boolean {
  return toolName === "propose_action"
    || toolName === "sub_agent"
    || toolName === "resync_chapter_state"
    || toolName === "short_fiction_run"
    || toolName === "script_create"
    || toolName === "storyboard_create"
    || toolName === "interactive_film_create"
    || toolName === "translation_create"
    || toolName === "fanfic_create"
    || toolName === "continuation_import"
    || toolName === "spinoff_create"
    || toolName === "imitation_create"
    || toolName === "generate_cover"
    || toolName === "play_start"
    || toolName === "play_edit"
    || toolName === "play_revise"
    || toolName === "play_step"
    || toolName === "create_narrative_forecast"
    || toolName === "get_narrative_forecast"
    || toolName === "select_narrative_branch";
}

function hasUnansweredTerminalToolResult(messages: AgentMessage[]): boolean {
  let assistantTextAfterTool = false;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (!message || typeof message !== "object" || !("role" in message)) continue;
    const role = (message as { role?: unknown }).role;
    if (role === "user") return false;
    if (role === "assistant") {
      const text = extractTextFromAssistant(message as AssistantMessage).trim();
      if (text) assistantTextAfterTool = true;
      continue;
    }
    if (role !== "toolResult") continue;
    const toolName = (message as { toolName?: unknown }).toolName;
    if (isTerminalProductionToolName(toolName)) {
      return !assistantTextAfterTool;
    }
  }
  return false;
}

async function runInAgentSessionQueue<T>(
  projectRoot: string,
  sessionId: string,
  task: () => Promise<T>,
): Promise<T> {
  const key = sessionQueueKey(projectRoot, sessionId);
  const previous = agentSessionQueues.get(key) ?? Promise.resolve();
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const queued = previous.catch(() => undefined).then(() => gate);
  agentSessionQueues.set(key, queued);

  await previous.catch(() => undefined);
  try {
    return await task();
  } finally {
    release();
    if (agentSessionQueues.get(key) === queued) {
      agentSessionQueues.delete(key);
    }
  }
}

async function latestCommittedSeq(projectRoot: string, sessionId: string): Promise<number> {
  const events = await readTranscriptEvents(projectRoot, sessionId);
  return events
    .filter((event) => event.type === "request_committed")
    .reduce((max, event) => Math.max(max, event.seq), 0);
}

function transcriptRoleForMessage(message: AgentMessage): TranscriptRole | null {
  if (!message || typeof message !== "object" || !("role" in message)) return null;
  const role = (message as { role?: unknown }).role;
  return role === "user" || role === "assistant" || role === "toolResult" || role === "system"
    ? role
    : null;
}

function firstToolCallId(message: AgentMessage): string | undefined {
  if (!message || typeof message !== "object" || !("content" in message)) return undefined;
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

function toolCallIdForMessage(message: AgentMessage): string | undefined {
  if (!message || typeof message !== "object") return undefined;
  if ((message as { role?: unknown }).role === "toolResult") {
    const toolCallId = (message as { toolCallId?: unknown }).toolCallId;
    return typeof toolCallId === "string" && toolCallId.length > 0 ? toolCallId : undefined;
  }
  return firstToolCallId(message);
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

async function ensureSessionCreatedEvent(
  projectRoot: string,
  sessionId: string,
  bookId: string | null,
  sessionKind?: SessionKind,
): Promise<void> {
  await appendTranscriptEvents(projectRoot, sessionId, ({ events, nextSeq }) => {
    if (events.some((event) => event.type === "session_created")) return [];

    const now = Date.now();
    return [{
      type: "session_created",
      version: 1,
      sessionId,
      seq: nextSeq,
      timestamp: now,
      bookId,
      ...(sessionKind ? { sessionKind } : {}),
      title: null,
      createdAt: now,
      updatedAt: now,
    }];
  });
}

async function appendAgentTranscriptEvent(
  projectRoot: string,
  sessionId: string,
  buildEvent: (seq: number) => TranscriptEvent,
): Promise<TranscriptEvent> {
  const events = await appendTranscriptEvents(projectRoot, sessionId, ({ nextSeq }) => [
    buildEvent(nextSeq),
  ]);
  const event = events[0];
  if (!event) throw new Error(`Failed to append transcript event for session "${sessionId}"`);
  return event;
}

/**
 * Extract readable text from an AssistantMessage's content array.
 * Filters out tool-call blocks; concatenates text blocks.
 */
function extractTextFromAssistant(msg: AssistantMessage): string {
  return msg.content
    .filter((c): c is { type: "text"; text: string } => c.type === "text")
    .map((c) => c.text)
    .join("");
}

/**
 * R41 计量投影（564 号）：各 AgentMessage 形态的文本面（user 字符串/内容
 * 分组、assistant 文本块；toolResult 等其余形态尽力取 text 分组）——缺省
 * 空串不抛（计量尽力而为，不阻断主链）。
 */
function extractMessageMeterText(message: AgentMessage): string {
  const candidate = message as { role?: string; content?: unknown };
  if (candidate.role === "assistant") {
    return extractTextFromAssistant(message as AssistantMessage);
  }
  const content = candidate.content;
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part) => {
        if (typeof part === "string") return part;
        const text = (part as { text?: unknown })?.text;
        return typeof text === "string" ? text : "";
      })
      .join("");
  }
  return "";
}

function lastAssistantMessage(messages: AgentMessage[]): AssistantMessage | undefined {
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i];
    if (msg && typeof msg === "object" && "role" in msg && (msg as { role?: unknown }).role === "assistant") {
      return msg as AssistantMessage;
    }
  }
  return undefined;
}

function assistantErrorMessage(message: AssistantMessage | undefined): string | undefined {
  return message &&
    (message.stopReason === "error" || message.stopReason === "aborted") &&
    message.errorMessage
      ? message.errorMessage
      : undefined;
}

function convertAgentMessagesForModel(messages: AgentMessage[], model: Model<Api>): Message[] {
  const llmMessages = messages.flatMap((message): Message[] => {
    if (!message || typeof message !== "object" || !("role" in message)) return [];
    const raw = message as { role?: unknown; content?: unknown };
    if (raw.role === "user" || raw.role === "assistant" || raw.role === "toolResult") {
      return [message as Message];
    }
    // 0.87 起系统提示词由 transcript 内 SystemMessage 携带（leading 或中途注入的
    // 状态块/边界）——原样保留 role，由 pi 的 provider 适配层处理各上游对中途
    // system 的兼容性（不支持的会重放合成 leading system）。0.73 时代此处把
    // system 降级为 user，是因为旧依赖的 transcript 无 system 语义。
    if (raw.role === "system") {
      return [message as Message];
    }
    return [];
  });

  const candidate = model as { api?: unknown; baseUrl?: unknown };
  // InkOS's internal `toolResult` role is not part of the OpenAI Chat Completions spec.
  // Many openai-completions upstreams (Google, and kkaiapi/DeepSeek-Pro-style gateways) reject
  // it outright — which surfaces as an opaque "503 provider temporarily unavailable" — so fold
  // tool results into a plain user message for EVERY openai-completions endpoint, not just Google.
  // Anthropic-format endpoints (MiniMax / 百炼) handle tool results natively and are left untouched.
  const isOpenAICompletionsCompatible = candidate.api === "openai-completions";
  if (!isOpenAICompletionsCompatible) return llmMessages;

  const converted: Message[] = [];
  const pushToolResultsAsUser = (toolResults: ToolResultMessage[]) => {
    const lines = toolResults.flatMap((result) => {
      const content = result.content
        .map((block) => block.type === "text" ? block.text : "[image]")
        .filter(Boolean)
        .join("\n")
        .trim() || "(empty tool result)";
      return [`- ${result.toolName} (${result.toolCallId}):`, content];
    });
    converted.push({
      role: "user",
      content: [
        "[Tool results]",
        ...lines,
        "Use these tool results to answer the active user request. If a tool failed, explain the failure and choose the next useful action.",
      ].join("\n"),
      timestamp: toolResults.reduce(
        (max, result) => Math.max(max, messageTimestamp(result as AgentMessage)),
        0,
      ) || Date.now(),
    });
  };

  for (let i = 0; i < llmMessages.length; i++) {
    const message = llmMessages[i];

    if (message.role === "assistant") {
      const textContent = message.content.filter(
        (block): block is { type: "text"; text: string } =>
          block.type === "text" && typeof block.text === "string" && block.text.trim().length > 0,
      );
      if (
        textContent.length === 1 &&
        message.content.length === 1 &&
        textContent[0].text.trim() === TOOL_RESULT_BRIDGE_TEXT
      ) {
        continue;
      }

      const toolCallIds = new Set<string>();
      for (const block of message.content) {
        if (block.type === "toolCall" && typeof block.id === "string" && block.id.length > 0) {
          toolCallIds.add(block.id);
        }
      }
      if (toolCallIds.size === 0) {
        converted.push(message);
        continue;
      }

      if (textContent.length > 0) {
        converted.push({ ...message, content: textContent });
      }

      const toolResults: ToolResultMessage[] = [];
      let nextIndex = i + 1;
      while (nextIndex < llmMessages.length) {
        const next = llmMessages[nextIndex];
        if (next.role !== "toolResult" || !toolCallIds.has(next.toolCallId)) break;
        toolResults.push(next);
        nextIndex += 1;
      }

      if (toolResults.length > 0) {
        pushToolResultsAsUser(toolResults);
        i = nextIndex - 1;
      }
      continue;
    }

    if (message.role === "toolResult") {
      pushToolResultsAsUser([message]);
      continue;
    }

    converted.push(message);
  }

  return converted;
}

/**
 * Extract thinking/reasoning text from an AssistantMessage's content array.
 */
function extractThinkingFromAssistant(msg: AssistantMessage): string {
  return msg.content
    .filter((c: any) => c.type === "thinking")
    .map((c: any) => c.thinking ?? "")
    .join("");
}

/**
 * Convert plain `{ role, content }` messages (from BookSession disk storage)
 * back into pi-agent AgentMessage format so they can be loaded into an Agent.
 */
function plainToAgentMessages(
  plain: Array<{ role: string; content: string }>,
): AgentMessage[] {
  return plain.map((m) => {
    const ts = Date.now();
    if (m.role === "user") {
      return { role: "user", content: m.content, timestamp: ts } satisfies UserMessage;
    }
    // For stored assistant messages we only have the text.
    // Re-wrap as a minimal AssistantMessage with a single TextContent.
    return {
      role: "assistant",
      content: [{ type: "text", text: m.content }],
      api: "anthropic-messages",
      provider: "anthropic",
      model: "unknown",
      usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
      stopReason: "stop",
      timestamp: ts,
    } satisfies AssistantMessage;
  });
}

/**
 * Flatten the Agent's in-memory messages to plain `{ role, content }` pairs
 * suitable for BookSession persistence.
 */
function agentMessagesToPlain(
  messages: AgentMessage[],
): Array<{ role: string; content: string; thinking?: string }> {
  const out: Array<{ role: string; content: string; thinking?: string }> = [];
  for (const msg of messages) {
    if (!msg || typeof msg !== "object" || !("role" in msg)) continue;

    const m = msg as { role: string; [k: string]: any };

    if (m.role === "user") {
      const content = typeof m.content === "string"
        ? m.content
        : Array.isArray(m.content)
          ? m.content
              .filter((c: any) => c.type === "text")
              .map((c: any) => c.text)
              .join("")
          : "";
      if (content) out.push({ role: "user", content });
    } else if (m.role === "assistant") {
      const text = extractTextFromAssistant(m as AssistantMessage);
      const thinking = extractThinkingFromAssistant(m as AssistantMessage);
      if (text || thinking) {
        const entry: { role: string; content: string; thinking?: string } = { role: "assistant", content: text };
        if (thinking) entry.thinking = thinking;
        out.push(entry);
      }
    }
    // ToolResult messages are internal; skip them for persistence.
  }
  return out;
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/**
 * Run a single conversation turn within a cached Agent session.
 *
 * If the session already exists in the cache, reuses the Agent (with its full
 * in-memory message history including tool calls). Otherwise creates a new
 * Agent, optionally restoring messages from `initialMessages`.
 */
export async function runAgentSession(
  config: AgentSessionConfig,
  userMessage: string,
  initialMessages?: Array<{ role: string; content: string }>,
): Promise<AgentSessionResult> {
  // 640 号：入队序在进队列前捕获（调用链同步段，无并发插队窗口）——受理点
  // 据此裁决 pending-abort（早于标记序的排队轮受理即中止）。
  const enqueuedSeq = nextSessionQueueSeq();
  return runInAgentSessionQueue(config.projectRoot, config.sessionId, () =>
    runAgentSessionUnlocked(config, userMessage, initialMessages, undefined, enqueuedSeq)
  );
}

/**
 * 640 号受理即中止：排队轮在等待期间被 abort（入队序早于标记序），受理时
 * 不装配 Agent、不调用模型——transcript 落 started + user + failed{"aborted"}
 * 三事件（与在飞中止同形：在飞轮的 user 事件也已随事件流落盘），返回与在飞
 * 中止同形状的 errorMessage 结果（server 侧 formatAgentFailure → 500 aborted，
 * 客户端 624 chatAbortedAt 契约零改动识别为中止）。
 */
async function acceptAbortedChatTurn(
  projectRoot: string,
  sessionId: string,
  bookId: string | null,
  sessionKind: SessionKind,
  input: string,
): Promise<AgentSessionResult> {
  const requestId = randomUUID();
  await ensureSessionCreatedEvent(projectRoot, sessionId, bookId, sessionKind);
  await appendAgentTranscriptEvent(projectRoot, sessionId, (seq) => ({
    type: "request_started",
    version: 1,
    sessionId,
    requestId,
    seq,
    timestamp: Date.now(),
    sessionKind,
    input,
  }));
  const userUuid = randomUUID();
  await appendAgentTranscriptEvent(projectRoot, sessionId, (seq) => ({
    type: "message",
    version: 1,
    sessionId,
    requestId,
    uuid: userUuid,
    parentUuid: null,
    seq,
    role: "user",
    timestamp: Date.now(),
    message: { role: "user", content: input, timestamp: Date.now() },
  }));
  await appendAgentTranscriptEvent(projectRoot, sessionId, (seq) => ({
    type: "request_failed",
    version: 1,
    sessionId,
    requestId,
    seq,
    timestamp: Date.now(),
    error: "aborted",
  }));
  return {
    responseText: "",
    messages: [],
    timings: { firstTokenMs: 0, totalMs: 0 },
    errorMessage: "aborted",
  };
}

async function runAgentSessionUnlocked(
  config: AgentSessionConfig,
  userMessage: string,
  initialMessages?: Array<{ role: string; content: string }>,
  overflowRetry?: { done: boolean },
  enqueuedSeq?: number,
): Promise<AgentSessionResult> {
  const { sessionId, language, pipeline, projectRoot, onEvent, onContextCompression } = config;
  // Normalize at the entry point so downstream comparisons, closures, and
  // fs paths never see `undefined`. The type is already `string | null`, but
  // some callers may bypass the type system (e.g. `activeBookId ?? null` gets
  // skipped) and we don't want that to (a) throw in path.join or (b) trigger
  // a spurious cache eviction because `null !== undefined`.
  const bookId: string | null = config.bookId ? assertSafeBookId(config.bookId) : null;
  const sessionKind: SessionKind = config.sessionKind ?? (bookId ? "book" : "chat");
  // 640 号受理边界：排队等待期间被 abort 的轮（入队序早于标记序）在此裁决——
  // 任何重装配（技能/模型/Agent 缓存）之前短路，受理即中止零模型调用。
  // overflowRetry 递归不传 enqueuedSeq（undefined）：压缩重试是新受理，不裁。
  if (enqueuedSeq !== undefined && takePendingSessionAbort(projectRoot, sessionId, enqueuedSeq)) {
    return acceptAbortedChatTurn(projectRoot, sessionId, bookId, sessionKind, userMessage);
  }
  const playMode = config.playMode;
  const actionSource = config.actionSource ?? "free-text";
  const requestedIntent = config.requestedIntent;
  const actionPayload = config.actionPayload;
  const actionPayloadKey = actionPayloadCacheKey(actionPayload);
  const configuredSkills = await loadAvailableAgentSkills({ projectRoot });
  const skillRegistry = createSkillRegistry({ skills: configuredSkills.skills });
  const skillResolution = skillRegistry.resolveSkills({
    requestedSkills: config.requestedSkills,
    disabledSkills: config.disabledSkills,
  });
  const skillResolutionKey = skillResolutionCacheKey(skillResolution);
  const model = resolveModel(config.model);
  const requestedModelIdentity = agentModelIdentity(model);
  const allowSystemFileRead = config.allowSystemFileRead ?? envFlagEnabled(process.env.INKOS_AGENT_ALLOW_SYSTEM_READ, false);
  const suppressProductionTools = config.suppressProductionTools ?? false;
  const playWorldExists = sessionKind === "play"
    ? Boolean(await new PlayStore(projectRoot).loadWorld(sessionId))
    : false;
  const cacheKey = agentCacheKey(projectRoot, sessionId);

  // ----- Resolve or create Agent -----
  let cached = agentCache.get(cacheKey);
  let currentCommittedSeq: number | undefined;

  if (cached) {
    currentCommittedSeq = await latestCommittedSeq(projectRoot, sessionId);
    // Evict and rebuild if model protocol identity OR bookId changed. Both are
    // captured into the Agent at construction time (model via initialState,
    // bookId via closures in systemPrompt / tools / transformContext), so a
    // mismatch means the cached Agent would keep using stale context.
    const modelChanged = cached.modelIdentity !== requestedModelIdentity;
    const projectRootChanged = cached.projectRoot !== projectRoot;
    const bookChanged = cached.bookId !== bookId;
    const sessionKindChanged = cached.sessionKind !== sessionKind;
    const actionSourceChanged = cached.actionSource !== actionSource;
    const requestedIntentChanged = cached.requestedIntent !== requestedIntent;
    const actionPayloadChanged = cached.actionPayloadKey !== actionPayloadKey;
    const skillResolutionChanged = cached.skillResolutionKey !== skillResolutionKey;
    const languageChanged = cached.language !== language;
    const apiKeyChanged = cached.apiKey !== config.apiKey;
    const readPermissionChanged = cached.allowSystemFileRead !== allowSystemFileRead;
    const playWorldChanged = cached.playWorldExists !== playWorldExists;
    const backgroundTaskContextChanged = cached.backgroundTaskContext !== config.backgroundTaskContext;
    const suppressProductionToolsChanged = cached.suppressProductionTools !== suppressProductionTools;
    // streamFn 闭包在 Agent 构造时捕获 config——服务「流式响应」开关切换后
    // 必须逐出重建（transcriptChanged 恒真的常态外，失败轮未 commit 时此处
    // 是偏好生效的唯一通路；621 号）。
    const streamPreferenceChanged = cached.streamPreference !== config.streamPreference;
    const transcriptChanged = cached.lastCommittedSeq !== currentCommittedSeq;

    if (
      modelChanged ||
      projectRootChanged ||
      bookChanged ||
      sessionKindChanged ||
      actionSourceChanged ||
      requestedIntentChanged ||
      actionPayloadChanged ||
      skillResolutionChanged ||
      languageChanged ||
      apiKeyChanged ||
      readPermissionChanged ||
      playWorldChanged ||
      backgroundTaskContextChanged ||
      suppressProductionToolsChanged ||
      streamPreferenceChanged ||
      transcriptChanged
    ) {
      agentCache.delete(cacheKey);
      cached = undefined;
    }
  }

  if (!cached) {
    let restoredHistory = await restoreAgentMessagesFromTranscript(projectRoot, sessionId, sessionKind);
    // R32a 轮间压缩（553 号）：缓存每轮必然逐出重建（transcriptChanged 恒真
    // ——上一轮 commit 已推进 seq），恢复产物即本轮上下文；usage 口径超
    // `window − reserveTokens` 时先落 compaction 条目再重恢复。失败放行
    // （压缩是维护性优化，绝不阻断会话），下一轮再试。
    if (restoredHistory.length > 0) {
      await maybeCompactSession({
        projectRoot,
        sessionId,
        sessionKind,
        requestId: `compaction-${randomUUID()}`,
        model,
        apiKey: config.apiKey,
        trigger: "threshold",
        onContextCompression,
      });
      const compacted = await restoreAgentMessagesFromTranscript(projectRoot, sessionId, sessionKind);
      if (compacted.length > 0) restoredHistory = compacted;
    }
    if (restoredHistory.length > 0) {
      onContextCompression?.({
        category: "session_context",
        phase: "start",
        sources: ["session transcript"],
      });
      onContextCompression?.({
        category: "session_context",
        phase: "end",
        sources: ["session transcript"],
      });
    }
    const restoredMessages = appendRestoredHistoryBoundary(
      adaptRestoredAgentMessagesForModel(
        restoredHistory,
        model,
      ),
      language,
    );
    const initialAgentMessages = restoredMessages.length > 0
      ? restoredMessages
      : initialMessages && initialMessages.length > 0
        ? plainToAgentMessages(initialMessages)
        : [];
    let terminalToolResultTail = false;
    const turnSkills = new Map<string, ActivatedSkillGuidance>(
      skillResolution.usedSkills.map((skill) => [skill.id, { skill, resources: [] }]),
    );
    const productionSkills = (capability: ProductionSkillCapability) => (
      resolveProductionSkillActivations(skillResolution.availableSkills, capability)
    );
    const allowIntentSkillSelection = actionSource === "free-text"
      && skillResolution.forcedSkillIds.length === 0;
    const baseSystemPrompt = buildAgentSystemPrompt(bookId, language, sessionKind, {
      actionSource,
      requestedIntent,
      playWorldExists,
      skills: skillResolution,
      allowIntentSkillSelection,
    });
    const intentSkillTool = allowIntentSkillSelection
      ? createUseSkillTool({
          registry: skillRegistry,
          disabledSkillIds: skillResolution.disabledSkillIds,
          onActivate: (activation) => turnSkills.set(activation.skill.id, activation),
        })
      : undefined;
    const agentTools = buildChatToolSet({
      pipeline,
      bookId,
      sessionId,
      sessionKind,
      actionSource,
      requestedIntent,
      actionPayload,
      projectRoot,
      allowSystemFileRead,
      language,
      playMode,
      playWorldExists,
      intentSkillTool,
      requestedSkillIds: () => [...turnSkills.keys()],
      attachmentPaths: () => cached?.currentAttachmentPaths ?? [],
      activeSkills: () => [...turnSkills.values()],
      workerSkills: (agent) => {
        if (agent === "architect" || agent === "writer") return productionSkills("longWriting");
        if (agent === "auditor" || agent === "reviser") return productionSkills("longReview");
        return [];
      },
      productionSkills,
    });
    const agent = new Agent({
      initialState: {
        model,
        systemPrompt: config.backgroundTaskContext
          ? `${baseSystemPrompt}\n\n${config.backgroundTaskContext}`
          : baseSystemPrompt,
        tools: suppressProductionTools
          ? agentTools.filter((tool) => !isProductionMutationToolName(tool.name))
          : agentTools,
        messages: initialAgentMessages,
      },
      transformContext: sessionKind === "interactive-film-authoring" && bookId
        ? createInteractiveFilmContextTransform(bookId, projectRoot)
        : createBookContextTransform(bookId, projectRoot, { onContextCompression }),
      convertToLlm: (messages) => {
        terminalToolResultTail = hasUnansweredTerminalToolResult(messages);
        return convertAgentMessagesForModel(messages, model);
      },
      streamFn: (streamModel, context, options) => {
        if (terminalToolResultTail) {
          terminalToolResultTail = false;
          return localAssistantStopStream(streamModel);
        }
        // 613 号：渲染链适配落地（guardedCompleteStream 补 text_delta 事件），
        // 608 回滚备案解除——stream:false 分支恢复接线。
        if (config.streamPreference === false) {
          return guardedCompleteStream(streamModel, context, options);
        }
        // R45/567 号：Provider 选择收拢进缝——Consumer 不分支于提供方身份。
        return guardedAgentStream(streamModel, context, options);
      },
      getApiKey: (provider: string) => {
        if (config.apiKey) return config.apiKey;
        return getEnvApiKey(provider);
      },
    });

    cached = {
      agent,
      sessionId,
      projectRoot,
      bookId,
      sessionKind,
      actionSource,
      requestedIntent,
      actionPayloadKey,
      skillResolutionKey,
      turnSkills,
      playWorldExists,
      language,
      modelIdentity: requestedModelIdentity,
      apiKey: config.apiKey,
      allowSystemFileRead,
      backgroundTaskContext: config.backgroundTaskContext,
      suppressProductionTools,
      streamPreference: config.streamPreference,
      currentAttachmentPaths: (config.attachments ?? [])
        .map((attachment) => attachment.storedPath?.trim())
        .filter((path): path is string => Boolean(path)),
      lastCommittedSeq: currentCommittedSeq ?? await latestCommittedSeq(projectRoot, sessionId),
      lastActive: Date.now(),
    };
    agentCache.set(cacheKey, cached);
    ensureCleanupTimer();
  }

  cached.lastActive = Date.now();
  cached.currentAttachmentPaths = (config.attachments ?? [])
    .map((attachment) => attachment.storedPath?.trim())
    .filter((path): path is string => Boolean(path));
  cached.turnSkills.clear();
  for (const skill of skillResolution.usedSkills) {
    cached.turnSkills.set(skill.id, { skill, resources: [] });
  }
  const { agent } = cached;
  const attachmentBlock = buildAttachmentUserBlock(config.attachments, language);
  const promptMessage = attachmentBlock ? `${userMessage}${attachmentBlock}` : userMessage;
  const promptImages = attachmentImages(config.attachments);

  // ----- Prepare transcript persistence -----
  const requestId = randomUUID();
  await ensureSessionCreatedEvent(projectRoot, sessionId, bookId, sessionKind);
  await appendAgentTranscriptEvent(projectRoot, sessionId, (seq) => ({
    type: "request_started",
    version: 1,
    sessionId,
    requestId,
    seq,
    timestamp: Date.now(),
    sessionKind,
    input: promptMessage,
  }));

  let parentUuid: string | null = null;
  let piTurnIndex = 0;
  let lastAssistantUuid: string | null = null;
  let skillTurnActive = cached.turnSkills.size > 0;

  const persistAgentEvent = async (event: AgentEvent): Promise<void> => {
    if (event.type === "turn_start") {
      piTurnIndex += 1;
      return;
    }
    if (event.type !== "message_end") return;

    const role = transcriptRoleForMessage(event.message);
    if (!role) return;

    if (assistantInvokesSkill(event.message)) skillTurnActive = true;
    const persistedMessage = sanitizeSkillTurnMessage(event.message, skillTurnActive);
    const uuid = randomUUID();
    const isToolResult = role === "toolResult";
    const toolCallId = toolCallIdForMessage(event.message);
    await appendAgentTranscriptEvent(projectRoot, sessionId, (seq) => ({
      type: "message",
      version: 1,
      sessionId,
      requestId,
      uuid,
      parentUuid: isToolResult && lastAssistantUuid ? lastAssistantUuid : parentUuid,
      seq,
      role,
      timestamp: messageTimestamp(event.message),
      piTurnIndex,
      ...(toolCallId ? { toolCallId } : {}),
      ...(isToolResult && lastAssistantUuid
        ? { sourceToolAssistantUuid: lastAssistantUuid }
        : {}),
      message: persistedMessage,
    }));

    if (role === "assistant") lastAssistantUuid = uuid;
    parentUuid = uuid;
  };

  // ----- Subscribe to events (transcript persistence + SSE forwarding) -----
  // G8a/333 号：AI 实况计时——turnStartedAt 起点到首个模型输出事件为"首包"。
  let turnStartedAt = 0;
  let firstEventAt = 0;
  const unsubscribe = agent.subscribe(async (event: AgentEvent) => {
    if (firstEventAt === 0 && event.type === "message_update") {
      firstEventAt = Date.now();
    }
    await persistAgentEvent(event);
    onEvent?.(event);
  });

  // ----- Execute the turn -----
  turnStartedAt = Date.now();
  let finalAssistant: AssistantMessage | undefined;
  let errorMessage: string | undefined;
  const turnMessageStartIndex = agent.state.messages.length;
  // R32b 溢出 compact-and-retry（553 号）：Pi Agent.prompt 把 streamFn 抛错转成
  // stopReason:"error" 的 failure message（handleRunFailure），所以溢出信号走
  // errorMessage 路径判定——上游 pi-ai isContextOverflow（provider 文案模式/
  // 静默溢出/length+零输出三 case）+ 本仓守卫文案（不在上游 pattern 表内）。
  // 命中→一次压缩+递归重建（缓存逐出→恢复已应用新 compaction 窗口）；失败保
  // 原错误——上游 overflowRecoveryUsed 一次守卫同构。
  let overflowSignal = false;

  try {
    await runWithAgentTrajectory({
      conversationId: opaqueConversationId(sessionId),
      runId: requestId,
      agentRole: "main",
    }, async () => {
      if (promptImages.length > 0) {
        await agent.prompt(promptMessage, promptImages);
      } else {
        await agent.prompt(promptMessage);
      }
    });

    finalAssistant = lastAssistantMessage(agent.state.messages);
    agent.state.messages = agent.state.messages.map((message, index) => (
      sanitizeSkillTurnMessage(
        message,
        skillTurnActive && index >= turnMessageStartIndex,
      )
    ));
    errorMessage = assistantErrorMessage(finalAssistant);
    if (errorMessage) {
      const overflowRecoveryAvailable = !overflowRetry?.done && (
        isContextOverflow(finalAssistant as AssistantMessage, model.contextWindow)
        || errorMessage.includes("InkOS context window guard")
      );
      if (overflowRecoveryAvailable) {
        overflowSignal = true;
      } else {
        await appendAgentTranscriptEvent(projectRoot, sessionId, (seq) => ({
          type: "request_failed",
          version: 1,
          sessionId,
          requestId,
          seq,
          timestamp: Date.now(),
          error: errorMessage as string,
        }));
        agentCache.delete(cacheKey);
      }
    } else {
      const committed = await appendAgentTranscriptEvent(projectRoot, sessionId, (seq) => ({
        type: "request_committed",
        version: 1,
        sessionId,
        requestId,
        seq,
        timestamp: Date.now(),
      }));
      cached.lastCommittedSeq = committed.seq;
    }
  } catch (error) {
    if (!overflowRetry?.done && error instanceof ContextWindowExceededError) {
      // 防御路径：prompt 阶段冒出的同步抛错（0.87 handleRunFailure 正常已转
      // failure message，此处兜 streamFn 之外的分析面）。
      overflowSignal = true;
      errorMessage = error.message;
    } else {
      await appendAgentTranscriptEvent(projectRoot, sessionId, (seq) => ({
        type: "request_failed",
        version: 1,
        sessionId,
        requestId,
        seq,
        timestamp: Date.now(),
        error: error instanceof Error ? error.message : String(error),
      }));
      agentCache.delete(cacheKey);
      throw error;
    }
  } finally {
    unsubscribe();
  }

  if (overflowSignal) {
    const compacted = await maybeCompactSession({
      projectRoot,
      sessionId,
      sessionKind,
      requestId,
      model,
      apiKey: config.apiKey,
      trigger: "overflow",
      onContextCompression,
    });
    await appendAgentTranscriptEvent(projectRoot, sessionId, (seq) => ({
      type: "request_failed",
      version: 1,
      sessionId,
      requestId,
      seq,
      timestamp: Date.now(),
      error: compacted
        ? `context window overflow; compacted and retried as a new request (${errorMessage})`
        : `context window overflow; compaction unavailable (${errorMessage})`,
    }));
    agentCache.delete(cacheKey);
    if (!compacted) {
      const allMessages = agent.state.messages;
      const retryFinal = finalAssistant ?? lastAssistantMessage(allMessages);
      const retryUsage = retryFinal?.usage
        ? {
            input: retryFinal.usage.input ?? 0,
            output: retryFinal.usage.output ?? 0,
            totalTokens: retryFinal.usage.totalTokens ?? 0,
          }
        : undefined;
      return {
        responseText: retryFinal ? extractTextFromAssistant(retryFinal) : "",
        messages: allMessages.slice(),
        ...(retryUsage ? { usage: retryUsage } : {}),
        ...(retryFinal ? { thinking: extractThinkingFromAssistant(retryFinal) } : {}),
        timings: {
          firstTokenMs: firstEventAt > 0 && turnStartedAt > 0 ? firstEventAt - turnStartedAt : 0,
          totalMs: turnStartedAt > 0 ? Date.now() - turnStartedAt : 0,
        },
        ...(errorMessage ? { errorMessage } : {}),
      };
    }
    return runAgentSessionUnlocked(config, userMessage, initialMessages, { done: true });
  }

  // ----- Extract result -----
  const allMessages = agent.state.messages;
  finalAssistant ??= lastAssistantMessage(allMessages);
  const responseText = finalAssistant ? extractTextFromAssistant(finalAssistant) : "";
  errorMessage ??= assistantErrorMessage(finalAssistant);

  // G8a/333 号：AI 实况——消息级 token 与首包/总耗时（pi usage 为权威值）。
  const usage = finalAssistant?.usage
    ? {
        input: finalAssistant.usage.input ?? 0,
        output: finalAssistant.usage.output ?? 0,
        totalTokens: finalAssistant.usage.totalTokens ?? 0,
      }
    : undefined;
  const thinking = finalAssistant ? extractThinkingFromAssistant(finalAssistant) : "";
  const totalMs = turnStartedAt > 0 ? Date.now() - turnStartedAt : 0;
  const firstTokenMs = firstEventAt > 0 && turnStartedAt > 0 ? firstEventAt - turnStartedAt : 0;

  // R41 计量面（564 号，与 Rust run_agent_loop 挂线同构）：全量消息回放入表
  // （state.messages 已含本轮 assistant 回复 = 下一轮请求面），usage 入锚。
  // 尽力而为：usage 缺失时纯启发式（锚点缺席），观测不阻断。
  let contextMeter: TokenMeterSnapshot | undefined;
  try {
    // 581 号：窗口兜底 128k（与 provider.ts 模型卡 miss 兜底同源）——无模型卡
    // 模型的 contextWindow 经服务端模型列表组装为 0（>0 才带字段），此前直传
    // → 计量窗口 0 → 超窗告警面永不触发（差分器快照面对照 node=0 vs
    // rust=128000 当场抓获）。非正数一律按缺省窗口计。
    const meter = new TokenMeter(model.contextWindow > 0 ? model.contextWindow : 128_000);
    for (const message of allMessages) {
      meter.append(extractMessageMeterText(message));
    }
    // 锚点模型身份 = 本轮请求侧 model.id（usage 归属请求模型）。
    meter.noteUsage(
      model.id ?? null,
      usage?.input ?? 0,
      usage?.output ?? 0,
      usage?.totalTokens ?? 0,
    );
    contextMeter = meter.measure();
  } catch {
    contextMeter = undefined;
  }

  return {
    responseText,
    messages: allMessages.slice(),
    ...(usage ? { usage } : {}),
    ...(thinking ? { thinking } : {}),
    timings: { firstTokenMs, totalMs },
    ...(contextMeter ? { contextMeter } : {}),
    ...(errorMessage ? { errorMessage } : {}),
  };
}

// ---------------------------------------------------------------------------
// Cache management
// ---------------------------------------------------------------------------

/** Manually evict a cached Agent session. */
export function evictAgentCache(sessionId: string): boolean {
  let deleted = agentCache.delete(sessionId);
  for (const [key, entry] of agentCache) {
    if (entry.sessionId !== sessionId) continue;
    agentCache.delete(key);
    deleted = true;
  }
  return deleted;
}

/** Abort an active cached pi-agent session and evict it from cache. */
export function abortAgentSession(projectRoot: string, sessionId: string): boolean {
  let aborted = false;
  for (const [key, entry] of agentCache) {
    if (entry.projectRoot !== projectRoot || entry.sessionId !== sessionId) continue;
    entry.agent.abort();
    entry.agent.clearAllQueues?.();
    agentCache.delete(key);
    aborted = true;
  }
  return aborted;
}

/**
 * R36 分支忙判定（636 号）：该会话是否有排队的 agent 任务（聊天轮/生产任务
 * 走同一互斥队列）。branch 要求「单请求事件整体在同一链上」——轮进行中拒绝
 * 分支（服务端映射 409 SESSION_BUSY）。无副作用，对照 abortAgentSession。
 */
export function isAgentSessionBusy(projectRoot: string, sessionId: string): boolean {
  return agentSessionQueues.has(sessionQueueKey(projectRoot, sessionId));
}
