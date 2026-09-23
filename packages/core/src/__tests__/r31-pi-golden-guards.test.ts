import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import {
  STREAM_GUARD_DEFAULTS,
  estimatePiContextTokens,
  estimateTextTokens,
  guardAssistantMessageStream,
} from "../llm/provider.js";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import type {
  Api,
  AssistantMessageEvent,
  AssistantMessageEventStream,
  Model,
} from "@earendil-works/pi-ai";
import { Agent } from "@earendil-works/pi-agent-core";
import type { AgentEvent, AgentMessage } from "@earendil-works/pi-agent-core";

/**
 * R31 golden 护栏（549 号；542 号施工图 §5）——pi-ai 0.87 迁移后的四组守门：
 *  1. 流守卫参数表（chat/pipeline 死线 + 瞬时重试预算）快照；
 *  2. AssistantMessageEvent 流形态透传快照 + adaptive 新变体忽略不崩断言；
 *  3. AgentEvent 十变体名单快照（0.73→0.87 零漂移面）+ 真实 Agent 回放出现面校验；
 *  4. estimatePiContextTokens 双语义守门：两形态数值快照 + 恒定偏移 parity
 *     （R31-4，548 号迁移审计点的永久化——system 内容计入路径变化即红）。
 */

const golden = JSON.parse(
  readFileSync(join(fileURLToPath(new URL(".", import.meta.url)), "golden", "r31-pi-guard-vectors.json"), "utf8"),
) as {
  streamGuardDefaults: Record<string, number>;
  assistantEventStreamShape: string[];
  adaptiveTolerance: { injectedEvent: string; mustReachTerminal: boolean };
  agentEventVariants: string[];
  estimatePiContextTokensVectors: Array<{
    name: string;
    systemPrompt: string | null;
    messages: Array<{ role: string; content: string }>;
    expectedTokens: number;
  }>;
};

const testModel: Model<Api> = {
  id: "r31-guard-model",
  name: "r31-guard-model",
  api: "openai-completions",
  provider: "r31",
  baseUrl: "",
  reasoning: false,
  input: ["text"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 100_000,
  maxTokens: 4_096,
};

const MOCK_USAGE = {
  input: 1, output: 1, cacheRead: 0, cacheWrite: 0,
  totalTokens: 2, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 2 },
};

function terminalAssistant() {
  return {
    role: "assistant",
    content: [{ type: "text", text: "ok" }],
    api: "openai-completions",
    provider: "r31",
    model: "r31-guard-model",
    usage: MOCK_USAGE,
    stopReason: "stop",
    timestamp: Date.now(),
  } as unknown as Parameters<AssistantMessageEventStream["end"]>[0];
}

/** 以受控序列驱动 AssistantMessageEventStream（事件入队后终态收尾）。 */
function replayAssistantEvents(events: Array<Record<string, unknown>>): AssistantMessageEventStream {
  const stream = createAssistantMessageEventStream();
  queueMicrotask(() => {
    for (const event of events) stream.push(event as AssistantMessageEvent);
    const message = terminalAssistant();
    stream.push({ type: "done", reason: "stop", message } as AssistantMessageEvent);
    stream.end(message);
  });
  return stream;
}

async function collectGuarded(
  start: (signal: AbortSignal) => AssistantMessageEventStream,
): Promise<string[]> {
  const guarded = guardAssistantMessageStream(testModel, start);
  const seen: string[] = [];
  for await (const event of guarded) {
    seen.push(event.type);
    if (event.type === "done" || event.type === "error") break;
  }
  return seen;
}

// ── 第 1 组：流守卫参数表 ──

describe("R31 group 1 — stream guard defaults snapshot", () => {
  it("matches the golden parameter table exactly (chat/pipeline deadlines + transient retries)", () => {
    expect(STREAM_GUARD_DEFAULTS).toEqual(golden.streamGuardDefaults);
  });
});

// ── 第 2 组：AssistantMessageEvent 流形态 + adaptive 容忍 ──

describe("R31 group 2 — assistant event stream shape", () => {
  it("forwards the canonical event sequence unchanged (start→…→done)", async () => {
    const seen = await collectGuarded(() =>
      replayAssistantEvents([
        { type: "start" },
        { type: "thinking_start" },
        { type: "thinking_delta", delta: "让我想想" },
        { type: "thinking_end" },
        { type: "text_start" },
        { type: "text_delta", delta: "OK" },
        { type: "text_end" },
      ]),
    );
    expect(seen).toEqual(golden.assistantEventStreamShape);
  });

  it("tolerates the 0.87 adaptive event without dropping the terminal done", async () => {
    const seen = await collectGuarded(() =>
      replayAssistantEvents([
        { type: "start" },
        { type: "text_start" },
        { type: "text_delta", delta: "OK" },
        { type: "text_end" },
        { type: golden.adaptiveTolerance.injectedEvent },
      ]),
    );
    expect(seen[seen.length - 1]).toBe("done");
    expect(seen).toContain("text_delta");
  });
});

// ── 第 3 组：AgentEvent 十变体名单 + 真实 Agent 回放 ──

describe("R31 group 3 — agent event variant surface", () => {
  it("locks the ten AgentEvent variants of pi-agent-core (0.73→0.87 zero-drift face)", async () => {
    const agent = new Agent({
      initialState: {
        model: { ...testModel, api: "anthropic-messages" },
        systemPrompt: "R31",
        tools: [],
        messages: [],
      },
      toolExecution: "sequential",
      streamFn: (model) =>
        guardAssistantMessageStream(
          model as never,
          () =>
            replayAssistantEvents([
              { type: "start" },
              { type: "text_start" },
              { type: "text_delta", delta: "ok" },
              { type: "text_end" },
            ]),
        ),
    });
    const seen: string[] = [];
    const unsubscribe = agent.subscribe(async (event: AgentEvent) => {
      if (!seen.includes(event.type)) seen.push(event.type);
    });
    try {
      await agent.prompt([{ role: "user", content: "hi", timestamp: Date.now() }]);
    } finally {
      unsubscribe();
    }
    // 名单是上限锁：出现过的变体必须全部在 golden 名单内（新变体=显式更新）。
    for (const event of seen) {
      expect(golden.agentEventVariants, `未知 AgentEvent 变体：${event}`).toContain(event);
    }
    expect(seen).toContain("agent_start");
    expect(seen).toContain("agent_end");
    expect(seen).toContain("message_end");
  });
});

// ── 第 4 组：estimatePiContextTokens 同值断言（R31-4） ──

describe("R31 group 4 — estimatePiContextTokens parity across system-prompt semantics", () => {
  it("scores both system-prompt semantics per the golden token snapshot", () => {
    for (const vector of golden.estimatePiContextTokensVectors) {
      const context = {
        ...(vector.systemPrompt !== null ? { systemPrompt: vector.systemPrompt } : {}),
        messages: vector.messages.map((message, index) => ({
          role: message.role,
          content: message.content,
          timestamp: 1_700_000_000_000 + index,
        })),
      };
      // 数值级快照：估算口径变化必须显式更新 golden（同值断言锚 R31-4）。
      expect(estimatePiContextTokens(context as never)).toBe(vector.expectedTokens);
    }
  });

  it("keeps the two forms equal for the same system text (parity invariant)", () => {
    const [legacy, transcript] = golden.estimatePiContextTokensVectors;
    const contextA = {
      systemPrompt: legacy.systemPrompt,
      messages: legacy.messages.map((message, index) => ({
        role: message.role,
        content: message.content,
        timestamp: 1_700_000_000_000 + index,
      })),
    };
    const contextB = {
      messages: transcript.messages.map((message, index) => ({
        role: message.role,
        content: message.content,
        timestamp: 1_700_000_000_000 + index,
      })),
    };
    // 0.87 transcript 形态多一条 message，其 role 标签（"system"）按既有口径
    // 计入——差值必须恒等于该标签成本；若 system 内容计入路径变化（丢失或
    // 重复计），本断言先红（R31-4 的守门本体）。
    const estimateA = estimatePiContextTokens(contextA as never);
    const estimateB = estimatePiContextTokens(contextB as never);
    expect(estimateB).toBe(estimateA + estimateTextTokens("system"));
  });
});
