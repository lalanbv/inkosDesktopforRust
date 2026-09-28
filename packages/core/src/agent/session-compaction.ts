// R32 会话压缩（553 号）：轮间 threshold 压缩 + 溢出 compact-and-retry。
//
// 施工图（543 号 §3）裁决：三压缩体系分工不变——写作链预算制与故事稿摘录
// 原样保留，protected 不可压语义不迁移 Pi 形态；本模块只补**会话面**缺口。
// 形态对齐上游 pi-agent-core harness（structural.js）：
// - threshold 触发 = `usageTokens + trailingEstimate > contextWindow − reserveTokens`
//   （上游 shouldCompact，token 口径 usage 优先，estimateContextTokens 同构）；
// - 切点 = 轮次边界回溯，keepRecentTokens 累积估算（上游 findCutPoint 消费——
//   本仓恢复产物是 AgentMessage[]，伪 Entry 包装后直接用上游原语，防双实现漂移）；
// - 摘要 = 上游 generateSummaryWithRequest（prompt/序列化内部持有零漂移），
//   迭代链式 = 上一条 compaction 的 summary 作 previousSummary 走 UPDATE prompt；
// - 溢出 = ContextWindowExceededError（guardedPiStream 每次模型调用前估算抛出）
//   触发一次压缩+重建重试；失败保原错误（上游 overflowRecoveryUsed 一次守卫同构）。
//
// 与上游的结构差异（备案）：压缩条目落本仓 transcript（线性 JSONL 的
// compaction 事件，firstKeptUuid 定位保留段），不引 Pi session-manager 树。
// 恢复端在 session-transcript-restore.restoreCommittedDialogueScan 应用窗口。
import type { AgentMessage, } from "@earendil-works/pi-agent-core";
import {
  DEFAULT_COMPACTION_SETTINGS,
  estimateContextTokens,
  findCutPoint,
  serializeConversation,
  shouldCompact,
  type CompactionSettings,
} from "@earendil-works/pi-agent-core";
// R30b 正统化：completeSimple 走自持分发、env key 走自持解析（替代 0.87
// deprecated 的 ./compat 子路径）。
import { piCompleteSimple } from "../llm/pi-dispatch.js";
import { getEnvApiKey } from "../llm/pi-env-keys.js";
import type { Api, AssistantMessage, Context as PiContext, Message, Model, SimpleStreamOptions } from "@earendil-works/pi-ai";
import { appendTranscriptEvents, readTranscriptEvents } from "../interaction/session-transcript.js";
import { restoreCommittedDialogueScan } from "../interaction/session-transcript-restore.js";
import type { CompactionEvent, SessionKind } from "../interaction/session-transcript-schema.js";
import type { ContextCompressionCallback } from "../models/context-compression.js";

export {
  DEFAULT_COMPACTION_SETTINGS,
  shouldCompact,
  estimateContextTokens,
};

/** 摘要生成的一次性请求死线：内部维护调用，不直连用户见面流。 */
const SUMMARY_REQUEST_TIMEOUT_MS = 180_000;

/** 上游 generateSummaryWithRequest 的摘要输出预算公式（0.8 × reserveTokens）。 */
const SUMMARY_MAX_TOKENS = Math.floor(0.8 * DEFAULT_COMPACTION_SETTINGS.reserveTokens);

/** 与上游 generateSummaryWithRequest 的 SummaryRequest 形态一致的闭包签名。 */
export type SummaryRequestFn = (aiContext: PiContext, options: SimpleStreamOptions) => Promise<AssistantMessage>;

export interface CompactionScan {
  /** 窗口化后的对话消息（compaction 窗口已应用，未做条数裁剪）。 */
  messages: AgentMessage[];
  /** 与 messages 平行的 transcript 定位 uuid。 */
  uuids: string[];
  /** 生效中的压缩条目（迭代链式摘要的 previousSummary 来源）。 */
  activeCompaction: CompactionEvent | null;
}

export interface CompactionOutcome {
  summary: string;
  firstKeptUuid: string | null;
  tokensBefore: number;
  trigger: "threshold" | "overflow";
  compaction: CompactionEvent;
}

/**
 * 扫描会话（compaction 窗口已应用）。工具历史摘要 system 消息不参与触发
 * 估算——它由保留段工具活动重建（8 条×180 字符量级，相对 20000 keepRecent
 * 预算可忽略），与上游 estimateTokens 对 system role 记 0 的语义一致（备案）。
 */
export async function scanSessionForCompaction(
  projectRoot: string,
  sessionId: string,
  sessionKind?: SessionKind,
): Promise<CompactionScan> {
  const events = await readTranscriptEvents(projectRoot, sessionId);
  const scan = restoreCommittedDialogueScan(events, sessionKind);
  return {
    messages: scan.messages,
    uuids: scan.uuids,
    activeCompaction: scan.activeCompaction,
  };
}

/**
 * threshold 判定：与上游 prepareCompaction→shouldCompact 两步同构——先估算
 * （usage 优先），再对 `contextWindow − reserveTokens` 判定。恢复端窗口化
 * 使"最新 compaction 遮蔽更早的"守卫隐含成立（压缩后恢复产物变小，下轮
 * 不再触发），无需显式 newest-compaction 检查。
 */
export function shouldCompactSession(
  scan: CompactionScan,
  contextWindow: number,
  settings: CompactionSettings = DEFAULT_COMPACTION_SETTINGS,
): boolean {
  if (!settings.enabled || scan.messages.length === 0) return false;
  // 无效窗口不判定（对齐 assertWithinContextWindow 的短路语义——未知模型卡
  // 不因估算是有限值而误触发压缩）。
  if (!Number.isFinite(contextWindow) || contextWindow <= 0) return false;
  const usage = estimateContextTokens(scan.messages);
  return usage.tokens > 0 && shouldCompact(usage.tokens, contextWindow, settings);
}

interface CompactionPlan {
  messagesToSummarize: AgentMessage[];
  firstKeptIndex: number;
  retainedTail: AgentMessage[];
  tokensBefore: number;
}

/**
 * 切点选择：伪 Entry 包装消费上游 findCutPoint（轮次边界= user 消息起始，
 * 保留段累积估算 ≥ keepRecentTokens）。
 *
 * 与上游 prepareCompaction 的差异（备案）：isSplitTurn 时上游把轮前缀
 * （turnPrefix）走独立 TURN_PREFIX 摘要；本仓恢复产物是 text-only 消息
 * （无工具块/bash），前缀独立摘要无增量价值，并入主摘要一次生成。
 * 保留段语义不变（firstKeptIndex 仍为 findCutPoint 的 firstKeptEntryIndex）。
 */
export function planCompaction(
  scan: CompactionScan,
  settings: CompactionSettings = DEFAULT_COMPACTION_SETTINGS,
): CompactionPlan | null {
  if (scan.messages.length === 0 || scan.uuids.length !== scan.messages.length) return null;
  // 伪 MessageEntry 包装：上游 findCutPoint/estimateTokens 只读 entry.type 与
  // entry.message（role/content），id/parentId/seq/timestamp 满足类型即可。
  const entries = scan.messages.map((message, index) => ({
    type: "message" as const,
    id: `scan:${index}`,
    parentId: index === 0 ? null : `scan:${index - 1}`,
    seq: index,
    timestamp: index,
    message,
  }));
  const cut = findCutPoint(entries, 0, entries.length, settings.keepRecentTokens);
  if (cut.firstKeptEntryIndex <= 0) return null;
  return {
    messagesToSummarize: scan.messages.slice(0, cut.firstKeptEntryIndex),
    firstKeptIndex: cut.firstKeptEntryIndex,
    retainedTail: scan.messages.slice(cut.firstKeptEntryIndex),
    tokensBefore: estimateContextTokens(scan.messages).tokens,
  };
}

/**
 * 上游 pi-agent-core 0.87 摘要 prompt 码点副本（R32，553 号）。
 * 源=dist/harness/compaction/compaction.js 的 SUMMARIZATION_SYSTEM_PROMPT /
 * SUMMARIZATION_PROMPT / UPDATE_SUMMARIZATION_PROMPT（后两者未导出，机械提取
 * 非手抄）；golden r32-compaction-vectors.json 三方锁定（本文件 == golden ==
 * Rust 自持副本），上游升级漂移时 golden 测试红，须双端考古后显式同步。
 */
const SUMMARIZATION_SYSTEM_PROMPT = `You are a context summarization assistant. Your task is to read a conversation between a user and an AI assistant, then produce a structured summary following the exact format specified.

Do NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.`;

const SUMMARIZATION_PROMPT = `The messages above are a conversation to summarize. Create a structured context checkpoint summary that another LLM will use to continue the work.

Use this EXACT format:

## Goal
[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]

## Constraints & Preferences
- [Any constraints, preferences, or requirements mentioned by user]
- [Or "(none)" if none were mentioned]

## Progress
### Done
- [x] [Completed tasks/changes]

### In Progress
- [ ] [Current work]

### Blocked
- [Issues preventing progress, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [Ordered list of what should happen next]

## Critical Context
- [Any data, examples, or references needed to continue]
- [Or "(none)" if not applicable]

Keep each section concise. Preserve exact file paths, function names, and error messages.`;

const UPDATE_SUMMARIZATION_PROMPT = `The messages above are NEW conversation messages to incorporate into the existing summary provided in <previous-summary> tags.

Update the existing structured summary with new information. RULES:
- PRESERVE all existing information from the previous summary
- ADD new progress, decisions, and context from the new messages
- UPDATE the Progress section: move items from "In Progress" to "Done" when completed
- UPDATE "Next Steps" based on what was accomplished
- PRESERVE exact file paths, function names, and error messages
- If something is no longer relevant, you may remove it

Use this EXACT format:

## Goal
[Preserve existing goals, add new ones if the task expanded]

## Constraints & Preferences
- [Preserve existing, add new ones discovered]

## Progress
### Done
- [x] [Include previously done items AND newly completed items]

### In Progress
- [ ] [Current work - update based on progress]

### Blocked
- [Current blockers - remove if resolved]

## Key Decisions
- **[Decision]**: [Brief rationale] (preserve all previous, add new)

## Next Steps
1. [Update based on current state]

## Critical Context
- [Preserve important context, add new if needed]

Keep each section concise. Preserve exact file paths, function names, and error messages.`;

/** 三 prompt 只读导出（R31 STREAM_GUARD_DEFAULTS 先例：快照源单点）。
 * golden r32-compaction-vectors.json 与 Rust 自持副本对其三方锁定。 */
export const COMPACTION_PROMPTS = {
  system: SUMMARIZATION_SYSTEM_PROMPT,
  summarization: SUMMARIZATION_PROMPT,
  updateSummarization: UPDATE_SUMMARIZATION_PROMPT,
} as const;

/**
 * 摘要 prompt 组装（上游 generateSummaryWithRequest 内联模板镜像）：
 * `<conversation>` 包序列化对话，迭代链式时前置 `<previous-summary>`，
 * 尾接首轮/更新 prompt。golden promptAssemblyVectors 锁定。
 */
export function buildSummaryPrompt(serializedConversationText: string, previousSummary: string | undefined): string {
  let promptText = `<conversation>\n${serializedConversationText}\n</conversation>\n\n`;
  if (previousSummary) {
    promptText += `<previous-summary>\n${previousSummary}\n</previous-summary>\n\n`;
  }
  return promptText + (previousSummary ? UPDATE_SUMMARIZATION_PROMPT : SUMMARIZATION_PROMPT);
}

/**
 * 生成会话摘要（上游 generateSummaryWithRequest 行为镜像，R32 裁决改自持：
 * 上游 request 注入式入口未从主入口导出、Models 集合构造重——prompt 码点与
 * 组装顺序以 golden 三方锁定防漂移）。序列化复用上游导出的
 * serializeConversation（本仓消息 user/assistant 透传语义同其 convertToLlm）。
 * 返回 null = 摘要调用失败（放行，不阻断会话）。
 */
export async function generateSessionSummary(
  messagesToSummarize: AgentMessage[],
  previousSummary: string | undefined,
  request: SummaryRequestFn,
): Promise<string | null> {
  if (messagesToSummarize.length === 0) return null;
  const conversationText = serializeConversation(messagesToSummarize as Message[]);
  const aiContext: PiContext = {
    systemPrompt: COMPACTION_PROMPTS.system,
    messages: [{
      role: "user",
      content: [{ type: "text", text: buildSummaryPrompt(conversationText, previousSummary) }],
      timestamp: Date.now(),
    }],
  };
  try {
    const response = await request(aiContext, { maxTokens: SUMMARY_MAX_TOKENS });
    if (response.stopReason === "aborted" || response.stopReason === "error") return null;
    const text = response.content
      .filter((block): block is { type: "text"; text: string } => block.type === "text")
      .map((block) => block.text)
      .join("")
      .trim();
    return text.length > 0 ? text : null;
  } catch {
    return null;
  }
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${label} timed out after ${timeoutMs}ms`)), timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer) clearTimeout(timer);
  });
}

/**
 * 生产摘要请求闭包：一次性 piCompleteSimple（R30b 自持分发）。maxTokens 直接
 * 消费上游 generateSummaryWithRequest 传入的 options（`0.8 × reserveTokens`
 * 公式在计算侧单点持有）；header/代理沿 pi-ai 默认 fetch。
 */
export function completeSummaryRequest(
  model: Model<Api>,
  apiKey: string | undefined,
): SummaryRequestFn {
  return async (aiContext, options) => {
    return withTimeout(
      piCompleteSimple(model, aiContext, {
        apiKey: apiKey ?? getEnvApiKey(model.provider),
        maxTokens: options.maxTokens ?? 8192,
        ...(options.reasoning ? { reasoning: options.reasoning } : {}),
      }),
      SUMMARY_REQUEST_TIMEOUT_MS,
      "session compaction summary",
    );
  };
}

/** 落盘 compaction 条目并返回事件（恢复端从下轮起应用窗口）。 */
async function appendCompactionEvent(
  projectRoot: string,
  sessionId: string,
  requestId: string,
  outcome: Omit<CompactionOutcome, "compaction">,
): Promise<CompactionEvent> {
  const events = await appendTranscriptEvents(projectRoot, sessionId, ({ nextSeq }) => [{
    type: "compaction",
    version: 1,
    sessionId,
    requestId,
    seq: nextSeq,
    timestamp: Date.now(),
    summary: outcome.summary,
    firstKeptUuid: outcome.firstKeptUuid,
    tokensBefore: outcome.tokensBefore,
    trigger: outcome.trigger,
  }]);
  const event = events[0];
  if (!event || event.type !== "compaction") {
    throw new Error(`Failed to append compaction event for session "${sessionId}"`);
  }
  return event;
}

export interface MaybeCompactParams {
  projectRoot: string;
  sessionId: string;
  sessionKind?: SessionKind;
  requestId: string;
  model: Model<Api>;
  apiKey?: string;
  trigger: "threshold" | "overflow";
  onContextCompression?: ContextCompressionCallback;
  /** 测试注入缝；缺省 completeSummaryRequest（completeSimple 一次性调用）。 */
  summaryRequest?: SummaryRequestFn;
}

/**
 * 单次压缩尝试：扫描→判定→切点→摘要→落盘。任一步失败返回 null（放行现状，
 * 会话照常进行——压缩是维护性优化，绝不阻断会话）；落盘前抛错不产生半态。
 */
export async function maybeCompactSession(
  params: MaybeCompactParams,
): Promise<CompactionOutcome | null> {
  const { projectRoot, sessionId, sessionKind, requestId, model, apiKey, trigger } = params;
  try {
    const scan = await scanSessionForCompaction(projectRoot, sessionId, sessionKind);
    // threshold 触发走阈值判定；overflow 触发不做阈值检查（溢出错误本身就是
    // 信号——上游 prepareOverflowCompaction 同样只 prepare 不 shouldCompact）。
    if (trigger === "threshold" && !shouldCompactSession(scan, model.contextWindow)) return null;
    const plan = planCompaction(scan);
    if (!plan) return null;

    params.onContextCompression?.({
      category: "session_context",
      phase: "start",
      sources: ["session compaction"],
      compressibleTokens: plan.tokensBefore,
    });

    const summary = await generateSessionSummary(
      plan.messagesToSummarize,
      scan.activeCompaction?.summary,
      params.summaryRequest ?? completeSummaryRequest(model, apiKey),
    );
    if (!summary) return null;

    const firstKeptUuid = plan.firstKeptIndex < scan.uuids.length
      ? scan.uuids[plan.firstKeptIndex]
      : null;
    const outcome: Omit<CompactionOutcome, "compaction"> = {
      summary,
      firstKeptUuid,
      tokensBefore: plan.tokensBefore,
      trigger,
    };
    const compaction = await appendCompactionEvent(projectRoot, sessionId, requestId, outcome);

    params.onContextCompression?.({
      category: "session_context",
      phase: "end",
      sources: ["session compaction"],
      compressibleTokens: plan.tokensBefore,
    });
    return { ...outcome, compaction };
  } catch {
    params.onContextCompression?.({
      category: "session_context",
      phase: "error",
      sources: ["session compaction"],
    });
    return null;
  }
}
