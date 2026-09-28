import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import {
  DEFAULT_COMPACTION_SETTINGS,
  estimateContextTokens,
  findCutPoint,
  serializeConversation,
  shouldCompact,
} from "@earendil-works/pi-agent-core";
import type { AgentMessage } from "@earendil-works/pi-agent-core";
import type { Message } from "@earendil-works/pi-ai";
import { appendTranscriptEvent, readTranscriptEvents } from "../interaction/session-transcript.js";
import {
  restoreAgentMessagesFromTranscript,
} from "../interaction/session-transcript-restore.js";
import type { MessageEvent } from "../interaction/session-transcript-schema.js";
import {
  COMPACTION_PROMPTS,
  buildSummaryPrompt,
  generateSessionSummary,
  maybeCompactSession,
  planCompaction,
  scanSessionForCompaction,
  shouldCompactSession,
} from "../agent/session-compaction.js";

/** 上游 dist 源文件定位（不写死 pnpm 哈希路径）。 */
function upstreamCompactionSource(): string {
  const require = createRequire(import.meta.url);
  const packageJsonPath = require.resolve("@earendil-works/pi-agent-core/package.json");
  return readFileSync(
    join(dirname(packageJsonPath), "dist/harness/compaction/compaction.js"),
    "utf8",
  );
}

// R31 惯例：JSON golden 以运行时文件读取（NodeNext 下 JSON import 需属性断言）。
const golden = JSON.parse(
  readFileSync(
    join(fileURLToPath(new URL(".", import.meta.url)), "golden", "r32-compaction-vectors.json"),
    "utf8",
  ),
) as {
  settings: { enabled: boolean; reserveTokens: number; keepRecentTokens: number };
  summaryMaxTokens: number;
  prompts: { system: string; summarization: string; updateSummarization: string };
  triggerVectors: Array<{
    name: string;
    contextWindow: number;
    settings: { enabled: boolean; reserveTokens: number; keepRecentTokens: number } | null;
    messages: Array<{ role: "user" | "assistant"; content: string; usage?: Record<string, number>; stopReason?: string }>;
    expected: { tokens: number; usageTokens: number; trailingTokens: number; compact: boolean };
  }>;
  cutPointVectors: Array<{
    name: string;
    keepRecentTokens: number;
    messages: Array<{ role: "user" | "assistant"; content: string }>;
    expected: { firstKeptEntryIndex: number; turnStartIndex: number; isSplitTurn: boolean };
  }>;
  serializeVectors: Array<{ name: string; messages: Array<{ role: "user" | "assistant"; content: string }>; expected: string }>;
  promptAssemblyVectors: Array<{ name: string; serialized: string; previousSummary: string | null; expected: string }>;
};

const EMPTY_USAGE = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
};

interface VectorMessage {
  role: "user" | "assistant";
  content: string;
  usage?: { input: number; output: number; cacheRead: number; cacheWrite: number; totalTokens: number };
  stopReason?: string;
}

/** golden 向量消息 → AgentMessage（assistant 补齐 usage/stopReason 形态）。 */
function vectorMessagesToAgentMessages(vector: ReadonlyArray<VectorMessage>): AgentMessage[] {
  return vector.map((message, index) => {
    if (message.role === "user") {
      return { role: "user", content: message.content, timestamp: index } as AgentMessage;
    }
    return {
      role: "assistant",
      content: [{ type: "text", text: message.content }],
      api: "openai-completions",
      provider: "test",
      model: "vector",
      usage: { ...EMPTY_USAGE, ...(message.usage ?? {}) },
      stopReason: message.stopReason ?? "stop",
      timestamp: index,
    } as unknown as AgentMessage;
  });
}

describe("R32 session compaction golden", () => {
  it("settings 与上游 DEFAULT_COMPACTION_SETTINGS 全等", () => {
    expect(DEFAULT_COMPACTION_SETTINGS).toEqual(golden.settings);
  });

  it("摘要 maxTokens 预算=0.8 × reserveTokens", () => {
    expect(golden.summaryMaxTokens).toBe(
      Math.floor(0.8 * DEFAULT_COMPACTION_SETTINGS.reserveTokens),
    );
  });

  it("三 prompt 码点与 golden 一致", () => {
    expect(COMPACTION_PROMPTS.system).toBe(golden.prompts.system);
    expect(COMPACTION_PROMPTS.summarization).toBe(golden.prompts.summarization);
    expect(COMPACTION_PROMPTS.updateSummarization).toBe(golden.prompts.updateSummarization);
    // 上游源文件仍持有同码点（机械提取源校验——上游升级漂移在此红）。
    const upstreamDist = upstreamCompactionSource();
    expect(upstreamDist).toContain(golden.prompts.system);
    expect(upstreamDist).toContain(golden.prompts.summarization);
    expect(upstreamDist).toContain(golden.prompts.updateSummarization);
  });

  it.each(golden.triggerVectors)("$name", (vector) => {
    const messages = vectorMessagesToAgentMessages(vector.messages as VectorMessage[]);
    const usage = estimateContextTokens(messages);
    expect(usage.tokens).toBe(vector.expected.tokens);
    expect(usage.usageTokens).toBe(vector.expected.usageTokens);
    expect(usage.trailingTokens).toBe(vector.expected.trailingTokens);
    // settings=null → 默认常量；invalid-window 短路在本仓 shouldCompactSession。
    const settings = (vector.settings ?? DEFAULT_COMPACTION_SETTINGS) as typeof DEFAULT_COMPACTION_SETTINGS;
    const scan = { messages, uuids: messages.map((_, i) => `u${i}`), activeCompaction: null };
    const compact = shouldCompactSession(scan, vector.contextWindow, settings);
    if (vector.name === "invalid-window-short-circuits") {
      // 无效窗口短路是本仓防御（上游 shouldCompact 无此分支，0 窗口会恒真）。
      expect(compact).toBe(vector.expected.compact);
      expect(shouldCompact(usage.tokens, vector.contextWindow, settings)).toBe(true);
    } else {
      expect(compact).toBe(vector.expected.compact);
      expect(shouldCompact(usage.tokens, vector.contextWindow, settings)).toBe(vector.expected.compact);
    }
  });

  it.each(golden.cutPointVectors)("$name", (vector) => {
    const messages = vectorMessagesToAgentMessages(vector.messages as VectorMessage[]);
    const scan = { messages, uuids: messages.map((_, i) => `u${i}`), activeCompaction: null };
    const plan = planCompaction(scan, { ...DEFAULT_COMPACTION_SETTINGS, keepRecentTokens: vector.keepRecentTokens });
    // 与上游原语双跑对照（本仓=伪 Entry 消费上游，行为必须全等）。
    const entries = messages.map((message, index) => ({
      type: "message" as const,
      id: `scan:${index}`,
      parentId: index === 0 ? null : `scan:${index - 1}`,
      seq: index,
      timestamp: index,
      message,
    }));
    const cut = findCutPoint(entries, 0, entries.length, vector.keepRecentTokens);
    expect(cut.firstKeptEntryIndex).toBe(vector.expected.firstKeptEntryIndex);
    expect(cut.turnStartIndex).toBe(vector.expected.turnStartIndex);
    expect(cut.isSplitTurn).toBe(vector.expected.isSplitTurn);
    if (vector.expected.firstKeptEntryIndex === 0) {
      expect(plan).toBeNull();
    } else {
      expect(plan?.firstKeptIndex).toBe(vector.expected.firstKeptEntryIndex);
      expect(plan?.retainedTail).toEqual(messages.slice(vector.expected.firstKeptEntryIndex));
    }
  });

  it.each(golden.serializeVectors)("$name", (vector) => {
    const messages = vectorMessagesToAgentMessages(vector.messages as VectorMessage[]);
    expect(serializeConversation(messages as unknown as Message[])).toBe(vector.expected);
  });

  it.each(golden.promptAssemblyVectors)("$name", (vector) => {
    expect(buildSummaryPrompt(vector.serialized, vector.previousSummary ?? undefined)).toBe(vector.expected);
  });
});

describe("R32 restore compaction window", () => {
  let projectRoot: string;

  beforeEach(async () => {
    projectRoot = await mkdtemp(join(tmpdir(), "inkos-compaction-"));
  });

  afterEach(async () => {
    await rm(projectRoot, { recursive: true, force: true });
  });

  async function commitRound(
    requestId: string,
    seqBase: number,
    rounds: Array<Array<{ role: "user" | "assistant"; text: string }>>,
  ): Promise<number> {
    let seq = seqBase;
    await appendTranscriptEvent(projectRoot, {
      type: "request_started", version: 1, sessionId: "s1", requestId, seq: seq++, timestamp: seq, input: "",
    } as never);
    for (const round of rounds) {
      for (const item of round) {
        const message = item.role === "user"
          ? { role: "user", content: item.text, timestamp: seq }
          : {
              role: "assistant", content: [{ type: "text", text: item.text }], api: "openai-completions",
              provider: "test", model: "m", usage: EMPTY_USAGE, stopReason: "stop", timestamp: seq,
            };
        await appendTranscriptEvent(projectRoot, {
          type: "message", version: 1, sessionId: "s1", requestId, uuid: `${requestId}-${item.role}-${seq}`,
          parentUuid: null, seq: seq++, role: item.role, timestamp: seq, message,
        } as MessageEvent);
      }
    }
    await appendTranscriptEvent(projectRoot, {
      type: "request_committed", version: 1, sessionId: "s1", requestId, seq: seq++, timestamp: seq,
    } as never);
    return seq;
  }

  it("无 compaction 时恢复行为与既有一致", async () => {
    let seq = 1;
    seq = await commitRound("r1", seq, [[{ role: "user", text: "hi" }, { role: "assistant", text: "hello" }]]);
    expect(seq).toBeGreaterThan(0);
    const restored = await restoreAgentMessagesFromTranscript(projectRoot, "s1", "chat");
    expect(restored.map((message) => (message as { role: string }).role)).toEqual(["user", "assistant"]);
  });

  it("compaction 后只回放 firstKeptUuid 起的对话并注入头部摘要", async () => {
    let seq = 1;
    await commitRound("r1", seq, [[{ role: "user", text: "early question" }, { role: "assistant", text: "early answer" }]]);
    seq += 10;
    const scanBefore = await scanSessionForCompaction(projectRoot, "s1", "chat");
    expect(scanBefore.messages).toHaveLength(2);
    const firstKeptUuid = scanBefore.uuids[1];

    await appendTranscriptEvent(projectRoot, {
      type: "compaction", version: 1, sessionId: "s1", requestId: "cmp-1", seq: seq++,
      timestamp: 1000, summary: "EARLY SUMMARY", firstKeptUuid, tokensBefore: 1234, trigger: "threshold",
    } as never);
    await commitRound("r2", seq, [[{ role: "user", text: "later question" }, { role: "assistant", text: "later answer" }]]);

    const restored = await restoreAgentMessagesFromTranscript(projectRoot, "s1", "chat");
    const texts = restored.map((message) => (message as { content?: unknown; role: string }) as {
      role: string; content: unknown;
    }).map((message) => ({
      role: message.role,
      text: typeof message.content === "string"
        ? message.content
        : (message.content as Array<{ text?: string }>).map((block) => block.text ?? "").join(""),
    }));
    expect(texts).toHaveLength(4);
    expect(texts[0].role).toBe("system");
    expect(texts[0].text).toContain("历史对话摘要");
    expect(texts[0].text).toContain("EARLY SUMMARY");
    // firstKeptUuid 起的保留段（early answer）+ 后续轮次
    expect(texts[1].text).toBe("early answer");
    expect(texts[2].text).toBe("later question");
    expect(texts[3].text).toBe("later answer");
  });

  it("firstKeptUuid=null 时清空对话只留摘要；坏 uuid 安全网回退全量", async () => {
    let seq = 1;
    await commitRound("r1", seq, [[{ role: "user", text: "old" }, { role: "assistant", text: "older" }]]);
    seq += 10;
    await appendTranscriptEvent(projectRoot, {
      type: "compaction", version: 1, sessionId: "s1", requestId: "cmp-1", seq: seq++,
      timestamp: 1000, summary: "ALL SUMMARY", firstKeptUuid: null, tokensBefore: 9, trigger: "overflow",
    } as never);
    let restored = await restoreAgentMessagesFromTranscript(projectRoot, "s1", "chat");
    expect(restored).toHaveLength(1);
    expect((restored[0] as unknown as { content: string }).content).toContain("ALL SUMMARY");

    await appendTranscriptEvent(projectRoot, {
      type: "compaction", version: 1, sessionId: "s1", requestId: "cmp-2", seq: seq++,
      timestamp: 2000, summary: "BROKEN", firstKeptUuid: "missing-uuid", tokensBefore: 9, trigger: "threshold",
    } as never);
    restored = await restoreAgentMessagesFromTranscript(projectRoot, "s1", "chat");
    // 坏 uuid → 忽略该 compaction，全量恢复（安全网）。
    expect(restored.map((message) => (message as { content?: unknown }).content as string)).toContain("old");
    expect(JSON.stringify(restored)).not.toContain("BROKEN");
  });
});

describe("R32 maybeCompactSession end-to-end", () => {
  let projectRoot: string;
  const summaryRequest = vi.fn();

  beforeEach(async () => {
    projectRoot = await mkdtemp(join(tmpdir(), "inkos-compact-e2e-"));
    summaryRequest.mockReset();
  });

  afterEach(async () => {
    await rm(projectRoot, { recursive: true, force: true });
  });

  async function seedMessages(requestId: string, items: Array<{ role: "user" | "assistant"; text: string }>): Promise<void> {
    let seq = 1;
    await appendTranscriptEvent(projectRoot, {
      type: "request_started", version: 1, sessionId: "s1", requestId, seq: seq++, timestamp: 1, input: "",
    } as never);
    for (const item of items) {
      const message = item.role === "user"
        ? { role: "user", content: item.text, timestamp: seq }
        : {
            role: "assistant", content: [{ type: "text", text: item.text }], api: "openai-completions",
            provider: "test", model: "m", usage: EMPTY_USAGE, stopReason: "stop", timestamp: seq,
          };
      await appendTranscriptEvent(projectRoot, {
        type: "message", version: 1, sessionId: "s1", requestId, uuid: `${requestId}-${seq}`,
        parentUuid: null, seq: seq++, role: item.role, timestamp: seq, message,
      } as MessageEvent);
    }
    await appendTranscriptEvent(projectRoot, {
      type: "request_committed", version: 1, sessionId: "s1", requestId, seq: seq++, timestamp: 1,
    } as never);
  }

  // 生产形态窗口：threshold = 40960 − 16384 = 24576；seed ≈ 27013 tokens
  // （12×assistant 9000 字符=2250 token + 13×user 1 token）超阈可触发，且
  // 总量 > keepRecentTokens(20000) 使上游 findCutPoint 有切点可选（总量小于
  // keepRecent 时上游语义=保留全部→无可压，plan 为 null——小窗口模型压不动的
  // 放行语义，maybeCompactSession 靠它安全退出）。
  const productionWindowModel = { contextWindow: 40_960 } as never;
  const bigSeed: Array<{ role: "user" | "assistant"; text: string }> = [];
  for (let i = 0; i < 12; i++) {
    bigSeed.push({ role: "user", text: `q${i}` });
    bigSeed.push({ role: "assistant", text: "w".repeat(9000) });
  }
  bigSeed.push({ role: "user", text: "q-final keep" });

  it("超阈值触发压缩：摘要码点/落盘事件/保留段全链", async () => {
    await seedMessages("r-big", bigSeed);
    summaryRequest.mockResolvedValue({
      stopReason: "stop",
      content: [{ type: "text", text: "STRUCTURED SUMMARY" }],
    });

    const outcome = await maybeCompactSession({
      projectRoot, sessionId: "s1", sessionKind: "chat",
      requestId: "cmp-req", model: productionWindowModel, trigger: "threshold",
      summaryRequest,
    });
    expect(outcome).not.toBeNull();
    expect(outcome?.summary).toBe("STRUCTURED SUMMARY");

    // 摘要请求码点断言（system+组装模板+maxTokens 预算）。
    expect(summaryRequest).toHaveBeenCalledTimes(1);
    const [aiContext, options] = summaryRequest.mock.calls[0] as unknown as [
      { systemPrompt: string; messages: Array<{ content: Array<{ text: string }> }> },
      { maxTokens: number },
    ];
    expect(aiContext.systemPrompt).toBe(golden.prompts.system);
    expect(options.maxTokens).toBe(golden.summaryMaxTokens);
    const promptText = aiContext.messages[0].content[0].text;
    expect(promptText).toContain("<conversation>");
    expect(promptText).toContain("[User]: q0");
    expect(promptText).toContain(golden.prompts.summarization);

    // 落盘事件与保留段。
    const events = await readTranscriptEvents(projectRoot, "s1");
    const compaction = events.find((event) => event.type === "compaction");
    expect(compaction && compaction.type === "compaction").toBe(true);
    if (compaction?.type === "compaction") {
      expect(compaction.trigger).toBe("threshold");
      expect(compaction.summary).toBe("STRUCTURED SUMMARY");
      expect(compaction.tokensBefore).toBeGreaterThan(24_576);
    }
    const restored = await restoreAgentMessagesFromTranscript(projectRoot, "s1", "chat");
    // 摘要 system + 保留段经 MAX_RESTORED_DIALOGUE_MESSAGES(12) 裁剪后的对话。
    expect(restored.length).toBeGreaterThan(1);
    expect(restored.length).toBeLessThanOrEqual(13);
    expect((restored[0] as unknown as { content: string }).content).toContain("STRUCTURED SUMMARY");
    expect(JSON.stringify(restored)).toContain("q-final keep");
    // 被摘要掉的前缀对话不再回放。
    expect(JSON.stringify(restored)).not.toContain("[User]: q0");
  });

  it("迭代链式：上一条 compaction 摘要作 previousSummary 走 UPDATE prompt", async () => {
    await seedMessages("r-1", bigSeed);
    summaryRequest.mockResolvedValue({
      stopReason: "stop",
      content: [{ type: "text", text: "FIRST SUMMARY" }],
    });
    const first = await maybeCompactSession({
      projectRoot, sessionId: "s1", sessionKind: "chat",
      requestId: "cmp-1", model: productionWindowModel, trigger: "threshold",
      summaryRequest,
    });
    expect(first?.summary).toBe("FIRST SUMMARY");

    // 压缩后新增消息再次超阈（保留段+新消息 usage 口径）。
    const events = await readTranscriptEvents(projectRoot, "s1");
    const compaction = events.find((event) => event.type === "compaction");
    let seq = (compaction?.seq ?? 0) + 1;
    await appendTranscriptEvent(projectRoot, {
      type: "request_started", version: 1, sessionId: "s1", requestId: "r-2", seq: seq++, timestamp: 1, input: "",
    } as never);
    await appendTranscriptEvent(projectRoot, {
      type: "message", version: 1, sessionId: "s1", requestId: "r-2", uuid: "r-2-a",
      parentUuid: null, seq: seq++, role: "assistant", timestamp: 1,
      message: {
        role: "assistant", content: [{ type: "text", text: "y".repeat(9000) }], api: "openai-completions",
        provider: "test", model: "m", usage: { ...EMPTY_USAGE, totalTokens: 30_000 }, stopReason: "stop", timestamp: 1,
      },
    } as MessageEvent);
    await appendTranscriptEvent(projectRoot, {
      type: "request_committed", version: 1, sessionId: "s1", requestId: "r-2", seq: seq++, timestamp: 1,
    } as never);

    summaryRequest.mockResolvedValue({
      stopReason: "stop",
      content: [{ type: "text", text: "UPDATED SUMMARY" }],
    });
    const second = await maybeCompactSession({
      projectRoot, sessionId: "s1", sessionKind: "chat",
      requestId: "cmp-2", model: productionWindowModel, trigger: "threshold",
      summaryRequest,
    });
    expect(second?.summary).toBe("UPDATED SUMMARY");
    const [secondContext] = summaryRequest.mock.calls[1] as unknown as [
      { messages: Array<{ content: Array<{ text: string }> }> },
    ];
    const secondPrompt = secondContext.messages[0].content[0].text;
    expect(secondPrompt).toContain("<previous-summary>\nFIRST SUMMARY\n</previous-summary>");
    expect(secondPrompt).toContain(golden.prompts.updateSummarization);
  });

  it("摘要调用失败放行（不落盘、返回 null）", async () => {
    await seedMessages("r-big", bigSeed);
    summaryRequest.mockRejectedValue(new Error("provider down"));
    const outcome = await maybeCompactSession({
      projectRoot, sessionId: "s1", sessionKind: "chat",
      requestId: "cmp-req", model: productionWindowModel, trigger: "overflow",
      summaryRequest,
    });
    expect(outcome).toBeNull();
    const events = await readTranscriptEvents(projectRoot, "s1");
    expect(events.some((event) => event.type === "compaction")).toBe(false);
  });

  it("低于阈值不触发（NOOP 缺省语义）", async () => {
    await seedMessages("r-small", [{ role: "user", text: "q1" }]);
    const outcome = await maybeCompactSession({
      projectRoot, sessionId: "s1", sessionKind: "chat",
      requestId: "cmp-req", model: { contextWindow: 200_000 } as never, trigger: "threshold",
    });
    expect(outcome).toBeNull();
    expect(summaryRequest).not.toHaveBeenCalled();
  });
});
