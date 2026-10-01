import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import type { Static, TSchema } from "@sinclair/typebox";
import { Value } from "@sinclair/typebox/value";
import type {
  AssistantMessage,
  AssistantMessageEventStream,
  Model,
  Api,
} from "@earendil-works/pi-ai";
import type { LLMMessage, LLMResponse } from "../llm/provider.js";
import type { WorkerResultTool } from "./worker-agent.js";

export function isLlmStubEnabled(): boolean {
  return Boolean(process.env.INKOS_AGENT_LLM_STUB);
}

// Mirrors EMPTY_USAGE in agent-session.ts exactly.
const EMPTY_USAGE = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
};

function lastUserText(context: { messages?: Array<{ role: string; content: unknown }> }): string {
  const msgs = context.messages ?? [];
  for (let i = msgs.length - 1; i >= 0; i--) {
    const m = msgs[i];
    if (m.role === "user") {
      return typeof m.content === "string" ? m.content : JSON.stringify(m.content);
    }
  }
  return "";
}

function alreadyProposed(
  context: { messages?: Array<{ role: string; content: unknown; toolName?: string }> },
): boolean {
  return (context.messages ?? []).some((m) => {
    // Agent format (non-openai-completions): role="toolResult" with toolName
    if (m.role === "toolResult" && (m as { toolName?: string }).toolName === "propose_action") {
      return true;
    }
    // LLM format (openai-completions): assistant message with toolCall content
    if (m.role === "assistant" && Array.isArray(m.content)) {
      if (
        (m.content as Array<{ type: string; name?: string }>).some(
          (c) => c.type === "toolCall" && c.name === "propose_action",
        )
      ) {
        return true;
      }
    }
    // LLM format (openai-completions folded): tool result folded into user message string
    if (m.role === "user" && typeof m.content === "string") {
      if (/- propose_action \(/.test(m.content as string)) {
        return true;
      }
    }
    return false;
  });
}

/**
 * Returns a deterministic AssistantMessageEventStream that either emits a
 * propose_action toolCall (when the latest user text mentions "结构/骨架/structure"
 * and propose_action hasn't run yet) or a plain "好的。" text reply.
 *
 * Mirrors localAssistantStopStream in agent-session.ts exactly — same
 * createAssistantMessageEventStream() + queueMicrotask pattern.
 */
export function stubAgentStream(model: Model<Api>, context: unknown): AssistantMessageEventStream {
  const stream = createAssistantMessageEventStream();
  const text = lastUserText(context as { messages?: Array<{ role: string; content: unknown }> });
  const proposed = alreadyProposed(
    context as { messages?: Array<{ role: string; content: unknown; toolName?: string }> },
  );
  const wantStructure = !proposed && /结构|骨架|structure/i.test(text);

  const content = wantStructure
    ? [
        {
          type: "toolCall" as const,
          id: "stub-draft",
          name: "propose_action",
          arguments: {
            action: "draft_structure",
            title: "搭建结构",
            summary: "确认后生成三幕骨架",
            instruction: "搭建一个三幕分支结构",
            draftStructure: { instruction: "三幕分支结构" },
          },
        },
      ]
    : [{ type: "text" as const, text: "好的。" }];

  const stopReason = wantStructure ? ("toolUse" as const) : ("stop" as const);

  const message: AssistantMessage = {
    role: "assistant",
    content: content as AssistantMessage["content"],
    api: model.api,
    provider: model.provider,
    model: model.id,
    usage: EMPTY_USAGE as AssistantMessage["usage"],
    stopReason,
    timestamp: Date.now(),
  };

  queueMicrotask(() => {
    stream.push({ type: "done", reason: stopReason, message });
    stream.end(message);
  });

  return stream;
}

const STRUCTURE_JSON = JSON.stringify({
  nodes: [
    {
      id: "s",
      type: "start",
      title: "开场",
      sceneDesc: "宫门前",
      choices: [{ id: "c1", text: "查账", targetNodeId: "b" }],
    },
    {
      id: "b",
      type: "branch",
      title: "抉择",
      sceneDesc: "账房",
      choices: [
        { id: "c2", text: "公开", targetNodeId: "e1" },
        { id: "c3", text: "隐瞒", targetNodeId: "e2" },
      ],
    },
    { id: "e1", type: "ending", title: "真相", choices: [] },
    { id: "e2", type: "ending", title: "沉沦", choices: [] },
  ],
});

const NODE_JSON = JSON.stringify({
  type: "branch",
  title: "新场景",
  sceneDesc: "夜色",
  dialogue: [{ speaker: "阿梅", text: "账不能错", emotion: "坚定" }],
  choices: [],
});

/**
 * Deterministic replacement for the chatCompletion network call.
 * Returns STRUCTURE_JSON when the prompt mentions structure/骨架/nodes,
 * otherwise a single node JSON.
 */
export function stubChatCompletion(
  messages: ReadonlyArray<LLMMessage>,
  _model: string,
): LLMResponse {
  const joined = messages.map((m) => m.content).join("\n");
  const content = /骨架|nodes|结构/i.test(joined) ? STRUCTURE_JSON : NODE_JSON;
  return {
    content,
    usage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
  };
}

/**
 * R45/567 号：结构化 worker 面的 stub 提供方实现——结果 JSON 的解析是 stub
 * 实现细节（stubChatCompletion 产出 resultTool 形状的 JSON），归 Provider
 * 角色；worker（Consumer）收到 undefined 即回落真传输，不分支于提供方身份。
 * undefined 哨兵安全：结构化 worker 结果是 TypeBox 对象形状，不会是 undefined。
 */
export function stubWorkerStructuredResult<TParameters extends TSchema>(
  messages: ReadonlyArray<LLMMessage>,
  modelId: string,
  resultTool: WorkerResultTool<TParameters>,
): Static<TParameters> | undefined {
  if (!isLlmStubEnabled()) return undefined;
  const response = stubChatCompletion(messages, modelId);
  return Value.Parse(resultTool.parameters, JSON.parse(response.content)) as Static<TParameters>;
}
