import type { AgentMessage } from "@earendil-works/pi-agent-core";
import { readTranscriptEvents } from "./session-transcript.js";
import {
  BookSessionSchema,
  type BookSession,
  type InteractionMessage,
  type ToolExecution,
  type PlayMode,
} from "./session.js";
import type { MessageEvent, SessionKind, TranscriptEvent } from "./session-transcript-schema.js";

function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object";
}

/**
 * transcript 反序列化域专用宽取：pi 0.87 的 Message union 与 Record guard
 * narrow 不兼容（interface 成员因无隐式索引签名被剔除、条件类型成员却保留，
 * narrow 结果恒为 ToolResultMessage|never）。运行时形状检查统一走本助手取
 * Record 域、出口处断言回 AgentMessage，行为与 0.73 时代完全一致。
 */
function asRecord(value: unknown): Record<string, unknown> | undefined {
  return isObject(value) ? (value as Record<string, unknown>) : undefined;
}

function contentBlocks(message: Record<string, unknown>): unknown[] {
  return Array.isArray(message.content) ? message.content : [];
}

function hasTextContent(message: Record<string, unknown>): boolean {
  return contentBlocks(message).some(
    (block) =>
      isObject(block) &&
      block.type === "text" &&
      typeof block.text === "string" &&
      block.text.length > 0,
  );
}

function hasToolCallContent(message: Record<string, unknown>): boolean {
  return contentBlocks(message).some(
    (block) => isObject(block) && block.type === "toolCall" && typeof block.id === "string",
  );
}

function toolCallIds(message: Record<string, unknown>): string[] {
  return contentBlocks(message)
    .filter(
      (block): block is Record<string, unknown> =>
        isObject(block) && block.type === "toolCall" && typeof block.id === "string",
    )
    .map((block) => block.id as string);
}

function isThinkingBlock(block: unknown): boolean {
  return isObject(block) && (block.type === "thinking" || block.type === "redacted_thinking");
}

function removeTrailingThinking(message: AgentMessage): AgentMessage {
  const raw = asRecord(message);
  if (!raw || raw.role !== "assistant" || !Array.isArray(raw.content)) {
    return message;
  }

  const content = [...raw.content];
  while (content.length > 0 && isThinkingBlock(content[content.length - 1])) {
    content.pop();
  }

  if (content.length === raw.content.length) return message;
  return { ...raw, content } as AgentMessage;
}

const emptyUsage = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
};

export const TOOL_RESULT_BRIDGE_TEXT = "I have processed the tool results.";
const MAX_RESTORED_DIALOGUE_MESSAGES = 12;
const MAX_RESTORED_TOOL_SUMMARY_ITEMS = 8;
const EXPIRED_SKILL_RESULT_TEXT = "Skill instructions expired after their original turn.";
const RESTORED_HISTORY_BOUNDARY_ZH =
  "[已完成的历史上下文]\n" +
  "以上是已经完成并提交的历史上下文，只能用于回忆，不代表当前轮已经执行了动作。\n" +
  "当前轮必须优先遵循用户接下来输入的最新指令；如果最新输入是提问、讨论或确认，直接回答，不要因为历史工具结果跳过判断或继续执行旧动作。";
const RESTORED_HISTORY_BOUNDARY_EN =
  "[Completed history context]\n" +
  "The messages above are completed and committed history. Use them only as memory; they do not mean any action has already run in the current turn.\n" +
  "For the current turn, prioritize the next user instruction. If it is a question, discussion, or confirmation, answer directly instead of continuing an old tool action.";

function toolResultBridgeMessage(timestamp: number): AgentMessage {
  return {
    role: "assistant",
    content: [{ type: "text", text: TOOL_RESULT_BRIDGE_TEXT }],
    api: "openai-completions",
    provider: "inkos",
    model: "synthetic-tool-result-bridge",
    usage: emptyUsage,
    stopReason: "stop",
    timestamp,
  } as AgentMessage;
}

function addToolResultBridges(messages: AgentMessage[]): AgentMessage[] {
  const bridged: AgentMessage[] = [];

  for (let i = 0; i < messages.length; i++) {
    const message = messages[i];
    bridged.push(message);

    const raw = asRecord(message);
    if (!raw || raw.role !== "toolResult") continue;

    const next = messages[i + 1];
    const nextRaw = asRecord(next);
    if (nextRaw && (nextRaw.role === "toolResult" || nextRaw.role === "assistant")) continue;

    const timestamp = typeof raw.timestamp === "number" ? raw.timestamp + 1 : Date.now();
    bridged.push(toolResultBridgeMessage(timestamp));
  }

  return bridged;
}

function systemMessage(content: string, timestamp: number): AgentMessage {
  return {
    role: "system",
    content,
    timestamp,
  } as unknown as AgentMessage;
}

export function appendRestoredHistoryBoundary(
  messages: AgentMessage[],
  language: string,
): AgentMessage[] {
  if (messages.length === 0) return messages;
  const timestamp = messages.reduce((max, message) => {
    if (isObject(message) && typeof message.timestamp === "number") {
      return Math.max(max, message.timestamp);
    }
    return max;
  }, 0) || Date.now();
  return [
    ...messages,
    systemMessage(
      language === "zh" ? RESTORED_HISTORY_BOUNDARY_ZH : RESTORED_HISTORY_BOUNDARY_EN,
      timestamp + 1,
    ),
  ];
}

export function cleanRestoredAgentMessages(messages: AgentMessage[]): AgentMessage[] {
  const availableToolCalls = new Set<string>();
  for (const message of messages) {
    const raw = asRecord(message);
    if (raw && raw.role === "assistant") {
      for (const id of toolCallIds(raw)) availableToolCalls.add(id);
    }
  }

  const cleaned = messages.filter((message) => {
    const raw = asRecord(message);
    if (!raw) return false;
    if (raw.role === "toolResult") {
      return typeof raw.toolCallId === "string" && availableToolCalls.has(raw.toolCallId);
    }
    if (raw.role === "assistant") {
      return hasTextContent(raw) || hasToolCallContent(raw);
    }
    return raw.role === "user" || raw.role === "system";
  });

  if (cleaned.length === 0) return cleaned;
  const last = cleaned[cleaned.length - 1];
  if (asRecord(last)?.role === "assistant") {
    cleaned[cleaned.length - 1] = removeTrailingThinking(last);
  }

  return cleaned;
}

interface TargetModelIdentity {
  readonly api?: unknown;
  readonly provider?: unknown;
  readonly id?: unknown;
  readonly compat?: unknown;
}

function requiresAssistantAfterToolResult(target: TargetModelIdentity): boolean {
  return !!(
    target.compat &&
    typeof target.compat === "object" &&
    (target.compat as { requiresAssistantAfterToolResult?: unknown }).requiresAssistantAfterToolResult === true
  );
}

function isSameAssistantModel(message: Record<string, unknown>, target: TargetModelIdentity): boolean {
  return (
    typeof message.api === "string" &&
    typeof message.provider === "string" &&
    typeof message.model === "string" &&
    message.api === target.api &&
    message.provider === target.provider &&
    message.model === target.id
  );
}

export function adaptRestoredAgentMessagesForModel(
  messages: AgentMessage[],
  target: TargetModelIdentity,
): AgentMessage[] {
  const adapted: AgentMessage[] = [];

  const pushToolResultsAsSystem = (toolResults: AgentMessage[]) => {
    const lines = toolResults.flatMap((message) => {
      const raw: Record<string, unknown> = isObject(message) ? message : {};
      const toolName = typeof raw.toolName === "string" ? raw.toolName : "tool";
      const toolCallId = typeof raw.toolCallId === "string" ? raw.toolCallId : "unknown";
      const text = contentBlocks(raw)
        .map((block) => {
          if (isObject(block) && block.type === "text" && typeof block.text === "string") {
            return block.text;
          }
          return "";
        })
        .filter(Boolean)
        .join("\n")
        .trim() || "(empty tool result)";
      return [`- ${toolName} (${toolCallId}):`, text];
    });
    const timestamp = toolResults.reduce((max, message) => {
      if (isObject(message) && typeof message.timestamp === "number") {
        return Math.max(max, message.timestamp);
      }
      return max;
    }, 0) || Date.now();
    adapted.push(systemMessage([
      "[Historical tool results]",
      "These are completed historical tool results. They are state context, not a new user request.",
      ...lines,
    ].join("\n"), timestamp));
  };

  for (let index = 0; index < messages.length; index++) {
    const message = messages[index];
    const raw = asRecord(message);
    if (!raw) continue;

    if (raw.role === "assistant") {
      const content = contentBlocks(raw);
      const isBridge = content.length === 1 &&
        isObject(content[0]) &&
        content[0].type === "text" &&
        typeof content[0].text === "string" &&
        content[0].text.trim() === TOOL_RESULT_BRIDGE_TEXT;
      const previous = adapted[adapted.length - 1];
      const previousRaw = asRecord(previous);
      const previousRole = previousRaw?.role;
      if (
        isBridge &&
        previousRaw &&
        (previousRole === "user" || previousRole === "system") &&
        typeof previousRaw.content === "string" &&
        (previousRaw.content.startsWith("[Tool results]") || previousRaw.content.startsWith("[Historical tool results]"))
      ) {
        continue;
      }

      const contentWithoutThinking = content.filter((block) => !isThinkingBlock(block));
      const foreignToolCallIds = new Set(
        contentWithoutThinking
          .filter(
            (block): block is Record<string, unknown> =>
              isObject(block) && block.type === "toolCall" && typeof block.id === "string",
          )
          .map((block) => block.id as string),
      );

      if (foreignToolCallIds.size === 0) {
        if (Array.isArray(raw.content) && isSameAssistantModel(raw, target)) {
          adapted.push(message);
          continue;
        }
        const rewritten = contentWithoutThinking.length === content.length
          ? message
          : ({ ...raw, content: contentWithoutThinking } as AgentMessage);
        if (
          contentWithoutThinking.some(
            (block) =>
              isObject(block) &&
              block.type === "text" &&
              typeof block.text === "string" &&
              block.text.length > 0,
          )
        ) {
          adapted.push(rewritten);
        }
        continue;
      }

      const textContent = contentWithoutThinking.filter(
        (block): block is { type: "text"; text: string } =>
          isObject(block) &&
          block.type === "text" &&
          typeof block.text === "string" &&
          block.text.trim().length > 0,
      );
      if (textContent.length > 0) {
        adapted.push({ ...raw, content: textContent } as AgentMessage);
      }

      const toolResults: AgentMessage[] = [];
      let nextIndex = index + 1;
      while (nextIndex < messages.length) {
        const next = messages[nextIndex];
        const nextRaw = asRecord(next);
        if (
          !nextRaw ||
          nextRaw.role !== "toolResult" ||
          typeof nextRaw.toolCallId !== "string" ||
          !foreignToolCallIds.has(nextRaw.toolCallId)
        ) {
          break;
        }
        toolResults.push(next);
        nextIndex += 1;
      }
      if (toolResults.length > 0) {
        pushToolResultsAsSystem(toolResults);
        index = nextIndex - 1;
      }
      continue;
    }

    if (raw.role === "toolResult") {
      pushToolResultsAsSystem([message]);
      continue;
    }

    adapted.push(message);
  }

  const filtered = adapted.filter((message) => {
    const raw = asRecord(message);
    if (!raw || raw.role !== "assistant") return true;
    return hasTextContent(raw) || hasToolCallContent(raw);
  });

  return requiresAssistantAfterToolResult(target)
    ? addToolResultBridges(filtered)
    : filtered;
}

export function committedMessageEvents(events: TranscriptEvent[], sessionKind?: SessionKind): MessageEvent[] {
  const requestKinds = new Map(
    events
      .filter((event) => event.type === "request_started")
      .map((event) => [event.requestId, event.sessionKind]),
  );
  const committed = new Set(
    events
      .filter((event) => event.type === "request_committed")
      .filter((event) => {
        if (!sessionKind) return true;
        const kind = requestKinds.get(event.requestId);
        return kind === undefined || kind === sessionKind;
      })
      .map((event) => event.requestId),
  );

  return events
    .filter((event): event is MessageEvent => event.type === "message" && committed.has(event.requestId))
    .sort((a, b) => a.seq - b.seq);
}

function requestKindMap(events: TranscriptEvent[]): Map<string, SessionKind | undefined> {
  return new Map(
    events
      .filter((event) => event.type === "request_started")
      .map((event) => [event.requestId, event.sessionKind]),
  );
}

function textOnlyAgentMessage(event: MessageEvent): AgentMessage | null {
  const raw = event.message as Record<string, unknown>;
  if (!isObject(raw) || event.role === "toolResult") return null;

  if (event.role === "user" || event.role === "system") {
    const content = textFromContent(raw.content);
    return content ? { role: event.role, content, timestamp: event.timestamp } as AgentMessage : null;
  }

  if (event.role === "assistant") {
    const textBlocks = contentBlocks(raw).filter(
      (block): block is { type: "text"; text: string } =>
        isObject(block) &&
        block.type === "text" &&
        typeof block.text === "string" &&
        block.text.trim().length > 0,
    );
    if (textBlocks.length === 0) return null;
    return {
      ...raw,
      role: "assistant",
      content: textBlocks,
      timestamp: event.timestamp,
    } as AgentMessage;
  }

  return null;
}

function buildHistoricalToolSummary(
  events: TranscriptEvent[],
  sessionKind?: SessionKind,
): AgentMessage | null {
  const kinds = requestKindMap(events);
  const committed = committedMessageEvents(events, sessionKind);
  const calls = new Map<string, { tool: string; agent?: string; timestamp: number }>();
  const summaries: Array<{ timestamp: number; line: string }> = [];

  for (const event of committed) {
    const requestKind = kinds.get(event.requestId);
    if (sessionKind && requestKind !== sessionKind) {
      continue;
    }

    const raw = event.message as Record<string, unknown>;
    if (!isObject(raw)) continue;

    if (event.role === "assistant") {
      for (const block of contentBlocks(raw)) {
        if (!isObject(block) || block.type !== "toolCall") continue;
        const id = typeof block.id === "string" ? block.id : "";
        if (!id) continue;
        const tool = typeof block.name === "string" && block.name ? block.name : "tool";
        const args = isObject(block.arguments) ? block.arguments : undefined;
        const agent = typeof args?.agent === "string" ? args.agent : undefined;
        calls.set(`${event.requestId}\0${id}`, {
          tool,
          ...(agent ? { agent } : {}),
          timestamp: event.timestamp,
        });
      }
      continue;
    }

    if (event.role !== "toolResult") continue;
    const toolCallId = typeof raw.toolCallId === "string"
      ? raw.toolCallId
      : event.toolCallId ?? "";
    const call = calls.get(`${event.requestId}\0${toolCallId}`);
    const tool = typeof raw.toolName === "string" && raw.toolName
      ? raw.toolName
      : call?.tool ?? "tool";
    const agent = call?.agent;
    const status = raw.isError === true ? "failed" : "completed";
    const text = tool === "use_skill"
      ? "expired; instructions are not active for later turns"
      : textFromContent(raw.content).replace(/\s+/g, " ").trim();
    const trimmed = text.length > 180 ? `${text.slice(0, 180)}...` : text;
    summaries.push({
      timestamp: event.timestamp,
      line: `- ${tool}${agent ? `:${agent}` : ""} ${status}${trimmed ? ` — ${trimmed}` : ""}`,
    });
  }

  if (summaries.length === 0) return null;
  const latest = summaries.slice(-MAX_RESTORED_TOOL_SUMMARY_ITEMS);
  const timestamp = latest.reduce((max, item) => Math.max(max, item.timestamp), 0) || Date.now();
  return systemMessage([
    "[历史状态摘要]",
    "以下是已经完成的历史工具动作摘要，只说明当前状态；不是当前用户的新指令，也不表示本轮已经执行。",
    ...latest.map((item) => item.line),
  ].join("\n"), timestamp);
}

function requestIdsWithToolActivity(events: ReadonlyArray<MessageEvent>): Set<string> {
  const ids = new Set<string>();
  for (const event of events) {
    if (event.role === "toolResult") {
      ids.add(event.requestId);
      continue;
    }
    const raw = event.message as Record<string, unknown>;
    if (!isObject(raw)) continue;
    if (contentBlocks(raw).some((block) => isObject(block) && block.type === "toolCall")) {
      ids.add(event.requestId);
    }
  }
  return ids;
}

function requestIdsUsingSkill(events: ReadonlyArray<MessageEvent>): Set<string> {
  const ids = new Set<string>();
  for (const event of events) {
    if (event.role !== "assistant") continue;
    const raw = event.message as Record<string, unknown>;
    if (!isObject(raw)) continue;
    if (contentBlocks(raw).some((block) => (
      isObject(block)
      && block.type === "toolCall"
      && block.name === "use_skill"
    ))) {
      ids.add(event.requestId);
    }
  }
  return ids;
}

export async function restoreAgentMessagesFromTranscript(
  projectRoot: string,
  sessionId: string,
  sessionKind?: SessionKind,
): Promise<AgentMessage[]> {
  const events = await readTranscriptEvents(projectRoot, sessionId);
  const summary = buildHistoricalToolSummary(events, sessionKind);
  const committed = committedMessageEvents(events, sessionKind);
  const toolRequestIds = requestIdsWithToolActivity(committed);
  const kinds = requestKindMap(events);
  const dialogue = committed
    .filter((event) => {
      if (!toolRequestIds.has(event.requestId)) return true;
      // Legacy events have no sessionKind. Keep the user's own words as
      // conversation memory, but do not replay the old tool call/result.
      return Boolean(sessionKind && kinds.get(event.requestId) === undefined && event.role === "user");
    })
    .map((event) => textOnlyAgentMessage(event))
    .filter((message): message is AgentMessage => message !== null)
    .slice(-MAX_RESTORED_DIALOGUE_MESSAGES);

  return [
    ...(summary ? [summary] : []),
    ...dialogue,
  ];
}

function textFromContent(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .filter(
      (block): block is { type: string; text: string } =>
        isObject(block) && block.type === "text" && typeof block.text === "string",
    )
    .map((block) => block.text)
    .join("");
}

function thinkingFromContent(content: unknown): string | undefined {
  if (!Array.isArray(content)) return undefined;
  const value = content
    .filter((block): block is Record<string, unknown> => isObject(block) && block.type === "thinking")
    .map((block) => typeof block.thinking === "string" ? block.thinking : "")
    .join("");
  return value || undefined;
}

function joinThinking(parts: ReadonlyArray<string | undefined>): string | undefined {
  const values = parts
    .map((part) => part?.trim() ?? "")
    .filter((part) => part.length > 0);
  return values.length > 0 ? values.join("\n\n---\n\n") : undefined;
}

function firstUserMessageTitle(messages: InteractionMessage[]): string | null {
  for (const message of messages) {
    if (message.role !== "user") continue;
    const oneLine = message.content.trim().replace(/\s+/g, " ");
    if (!oneLine) return null;
    return oneLine.length > 20 ? `${oneLine.slice(0, 20)}…` : oneLine;
  }
  return null;
}

function messageEventToInteractionMessage(
  event: MessageEvent,
  restoredToolExecutions?: ToolExecution[],
  restoredThinking?: ReadonlyArray<string>,
  options: { suppressAssistantText?: boolean; suppressThinking?: boolean } = {},
): InteractionMessage | null {
  const raw = event.message as Record<string, unknown>;
  if (!isObject(raw)) return null;
  if (event.role === "toolResult") return null;

  if (event.role === "user") {
    const content = textFromContent(raw.content);
    return content ? { role: "user", content, timestamp: event.timestamp } : null;
  }

  if (event.role === "assistant") {
    const rawText = textFromContent(raw.content);
    const content = options.suppressAssistantText ? "" : rawText;
    const thinking = options.suppressThinking
      ? undefined
      : joinThinking([
          ...(restoredThinking ?? []),
          thinkingFromContent(raw.content),
          event.legacyDisplay?.thinking,
          options.suppressAssistantText ? rawText : undefined,
        ]);
    const toolExecutions = restoredToolExecutions?.length
      ? restoredToolExecutions
      : event.legacyDisplay?.toolExecutions as ToolExecution[] | undefined;
    if (!content && !thinking && !toolExecutions?.length) return null;
    if (!content && !toolExecutions?.length) return null;
    return {
      role: "assistant",
      content,
      ...(thinking ? { thinking } : {}),
      ...(toolExecutions?.length ? { toolExecutions } : {}),
      timestamp: event.timestamp,
    };
  }

  if (event.role === "system") {
    const content = textFromContent(raw.content);
    return content ? { role: "system", content, timestamp: event.timestamp } : null;
  }

  return null;
}

function messageEventsToInteractionMessages(events: MessageEvent[]): InteractionMessage[] {
  type RestoredToolCall = {
    id: string;
    tool: string;
    args?: Record<string, unknown>;
    timestamp: number;
  };

  const agentLabels: Record<string, string> = {
    architect: "建书",
    writer: "写作",
    auditor: "审计",
    reviser: "修订",
    exporter: "导出",
  };
  const toolLabels: Record<string, string> = {
    read: "读取文件",
    edit: "编辑文件",
    grep: "搜索",
    ls: "列目录",
    propose_action: "确认动作",
    short_fiction_run: "短篇生产",
    generate_cover: "生成封面",
    play_edit: "编辑互动世界",
    play_start: "启动互动世界",
    play_revise: "重做互动回合",
    play_step: "推进互动世界",
    create_narrative_forecast: "剧情多线推演",
    get_narrative_forecast: "核验剧情推演",
    select_narrative_branch: "采用候选分支",
  };

  const messages: InteractionMessage[] = [];
  const toolCalls = new Map<string, RestoredToolCall>();
  const skillRequestIds = requestIdsUsingSkill(events);
  let pendingToolExecutions: ToolExecution[] = [];
  let pendingThinking: string[] = [];
  let activeRequestId: string | null = null;

  const clearPending = () => {
    pendingToolExecutions = [];
    pendingThinking = [];
  };
  const flushPendingToolExecutions = () => {
    if (pendingToolExecutions.length === 0) return;
    const last = messages[messages.length - 1];
    const thinking = joinThinking(pendingThinking);
    if (last?.role === "assistant") {
      messages[messages.length - 1] = {
        ...last,
        ...(thinking ? { thinking: joinThinking([last.thinking, thinking]) } : {}),
        toolExecutions: [
          ...(last.toolExecutions ?? []),
          ...pendingToolExecutions,
        ],
      };
    } else {
      messages.push({
        role: "assistant",
        content: "",
        ...(thinking ? { thinking } : {}),
        toolExecutions: pendingToolExecutions,
        timestamp: pendingToolExecutions.reduce(
          (latest, execution) => Math.max(latest, execution.completedAt ?? execution.startedAt),
          0,
        ),
      });
    }
    clearPending();
  };
  const toolCallKey = (requestId: string, toolCallId: string): string =>
    `${requestId}\0${toolCallId}`;

  const objectArgs = (value: unknown): Record<string, unknown> | undefined => {
    if (isObject(value)) return value;
    if (typeof value !== "string" || !value.trim()) return undefined;
    try {
      const parsed: unknown = JSON.parse(value);
      return isObject(parsed) ? parsed : undefined;
    } catch {
      return undefined;
    }
  };

  const resolveToolLabel = (tool: string, agent?: string): string => {
    if (tool === "sub_agent" && agent) return agentLabels[agent] ?? agent;
    return toolLabels[tool] ?? tool;
  };

  const hasCompletedPlayTool = (executions: ReadonlyArray<ToolExecution>): boolean =>
    executions.some((execution) =>
      execution.status === "completed"
      && (execution.tool === "play_start" || execution.tool === "play_step" || execution.tool === "play_revise")
    );

  const rememberToolCalls = (event: MessageEvent, raw: Record<string, unknown>) => {
    for (const block of contentBlocks(raw)) {
      if (!isObject(block) || block.type !== "toolCall") continue;
      if (typeof block.id !== "string" || !block.id) continue;
      const tool = typeof block.name === "string" && block.name ? block.name : "tool";
      const args = objectArgs(block.arguments);
      toolCalls.set(toolCallKey(event.requestId, block.id), {
        id: block.id,
        tool,
        ...(args ? { args } : {}),
        timestamp: event.timestamp,
      });
    }
  };

  const toolExecutionFromResult = (event: MessageEvent): ToolExecution | null => {
    const raw = event.message as Record<string, unknown>;
    if (!isObject(raw)) return null;
    const toolCallId = typeof raw.toolCallId === "string"
      ? raw.toolCallId
      : typeof event.toolCallId === "string"
        ? event.toolCallId
        : "";
    if (!toolCallId) return null;

    const call = toolCalls.get(toolCallKey(event.requestId, toolCallId));
    const tool = typeof raw.toolName === "string" && raw.toolName
      ? raw.toolName
      : call?.tool ?? "tool";
    const args = call?.args;
    const agent = tool === "sub_agent" && typeof args?.agent === "string"
      ? args.agent
      : undefined;
    const text = textFromContent(raw.content).trim();
    const isError = raw.isError === true;
    const details = raw.details;
    const expiredSkill = tool === "use_skill";

    return {
      id: toolCallId,
      tool,
      ...(agent ? { agent } : {}),
      label: resolveToolLabel(tool, agent),
      status: isError ? "error" : "completed",
      ...(args ? { args } : {}),
      ...(isError
        ? { error: text.slice(0, 500) || "Tool execution failed" }
        : expiredSkill
          ? { result: EXPIRED_SKILL_RESULT_TEXT }
          : text
          ? { result: text.slice(0, 200) }
          : {}),
      ...(expiredSkill
        ? { details: { kind: "skill_expired" } }
        : details !== undefined
          ? { details }
          : {}),
      startedAt: call?.timestamp ?? event.timestamp,
      completedAt: event.timestamp,
    };
  };

  for (const event of events) {
    const raw = event.message as Record<string, unknown>;
    if (activeRequestId !== event.requestId) {
      flushPendingToolExecutions();
      activeRequestId = event.requestId;
      clearPending();
    }

    if (event.role === "assistant" && isObject(raw)) {
      rememberToolCalls(event, raw);
      const isSkillRequest = skillRequestIds.has(event.requestId);
      const currentThinking = isSkillRequest ? undefined : thinkingFromContent(raw.content);
      const currentText = textFromContent(raw.content).trim();
      const legacyToolExecutions = event.legacyDisplay?.toolExecutions as ToolExecution[] | undefined;
      const hasLegacyDisplay = !!currentThinking
        || (!isSkillRequest && !!event.legacyDisplay?.thinking)
        || !!legacyToolExecutions?.length;
      if (
        pendingToolExecutions.length > 0
        && !hasCompletedPlayTool(pendingToolExecutions)
        && !currentText
        && !currentThinking
        && !hasToolCallContent(raw)
        && !hasLegacyDisplay
      ) {
        // Empty terminal assistant messages only close the model turn. Keep
        // non-play tool results pending so they attach to the previous prose
        // message; play results are intentionally restored as standalone cards.
        continue;
      }
      const message = messageEventToInteractionMessage(
        event,
        pendingToolExecutions.length > 0 ? pendingToolExecutions : undefined,
        pendingThinking.length > 0 ? pendingThinking : undefined,
        {
          suppressAssistantText: hasCompletedPlayTool(pendingToolExecutions),
          suppressThinking: isSkillRequest,
        },
      );
      if (message) {
        messages.push(message);
        clearPending();
      } else if (hasToolCallContent(raw) && currentThinking) {
        pendingThinking.push(currentThinking);
      }
      continue;
    }

    if (event.role === "toolResult") {
      const execution = toolExecutionFromResult(event);
      if (execution) pendingToolExecutions.push(execution);
      continue;
    }

    clearPending();
    const message = messageEventToInteractionMessage(event);
    if (message) messages.push(message);
  }

  flushPendingToolExecutions();
  return messages;
}

export async function deriveBookSessionFromTranscript(
  projectRoot: string,
  sessionId: string,
): Promise<BookSession | null> {
  const events = await readTranscriptEvents(projectRoot, sessionId);
  if (events.length === 0) return null;

  const created = events.find((event) => event.type === "session_created");
  let bookId = created?.type === "session_created" ? created.bookId : null;
  let sessionKind = created?.type === "session_created" ? created.sessionKind : undefined;
  let playMode: PlayMode | undefined = created?.type === "session_created" ? created.playMode : undefined;
  let title = created?.type === "session_created" ? created.title : null;
  const createdAt = created?.type === "session_created"
    ? created.createdAt
    : events[0]?.timestamp ?? Date.now();
  const latestActivityTimestamp = events.reduce((max, event) => {
    if (event.type === "session_created" || event.type === "session_metadata_updated") {
      return Math.max(max, event.updatedAt);
    }
    return Math.max(max, event.timestamp);
  }, 0);
  let updatedAt = created?.type === "session_created"
    ? created.updatedAt
    : events[events.length - 1]?.timestamp ?? createdAt;
  updatedAt = Math.max(updatedAt, latestActivityTimestamp);

  for (const event of events) {
    if (event.type !== "session_metadata_updated") continue;
    if ("bookId" in event && event.bookId !== undefined) bookId = event.bookId;
    if ("sessionKind" in event && event.sessionKind !== undefined) sessionKind = event.sessionKind;
    if ("playMode" in event && event.playMode !== undefined) playMode = event.playMode;
    if ("title" in event && event.title !== undefined) title = event.title;
    updatedAt = Math.max(updatedAt, event.updatedAt);
  }

  const messages = messageEventsToInteractionMessages(committedMessageEvents(events));

  if (title === null) {
    title = firstUserMessageTitle(messages);
  }

  return BookSessionSchema.parse({
    sessionId,
    bookId,
    sessionKind,
    playMode,
    title,
    messages,
    draftRounds: [],
    events: [],
    createdAt,
    updatedAt,
  });
}
