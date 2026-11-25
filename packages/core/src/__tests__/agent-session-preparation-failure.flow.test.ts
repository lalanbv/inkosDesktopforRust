// 配置文件状态契约轮取证确认的缺陷：回合 `try` 之前的步骤抛错时，缓存里的 Agent
// 留下残留。续投路径在 try 之前已把用户 / 动作消息推进 Agent 记忆、开了 episode、
// 之后还会挂事件订阅；其中送达账本核对（回执指向的修订不存在 → ARTIFACT_NOT_FOUND）
// 与送达状态落盘都会抛错。残留的后果：下一次请求再叠一层订阅（同一事件落盘两
// 遍、用量记两遍）、续投交换在模型上下文里出现两次；而 currentEpisode 非 null 的
// 缓存项 removeCachedAgent 拒收、TTL 清理也不收，直到进程重启都在。
import { afterEach, expect, it, vi } from "vitest";
import type { Model } from "@mariozechner/pi-ai";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { AgentSessionConfig } from "../agent/agent-session.js";
import { evictAgentCache, runAgentSession } from "../agent/agent-session.js";
import { syncWorkSourceArtifacts } from "../harness/source-sync.js";
import { createWorkManifest, saveWorkManifest } from "../harness/work-store.js";
import { readTranscriptEvents } from "../interaction/session-transcript.js";
import type { TranscriptEvent } from "../interaction/session-transcript-schema.js";
import { createLLMClient } from "../llm/provider.js";
import { PipelineRunner } from "../pipeline/runner.js";

/** 测试凭据一律 env 注入 + 非凭据回退。 */
const testKey = process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";

type MessageEvent = Extract<TranscriptEvent, { type: "message" }>;
/** 转录里消息事件的 `message` 在 schema 上是 `unknown`；这里只读用得到的字段。 */
type DurableMessage = { toolCallId?: string; content?: Array<{ type: string; name?: string }> };
const messageEvents = (events: TranscriptEvent[]): MessageEvent[] =>
  events.filter((event): event is MessageEvent => event.type === "message");
const durable = (event: MessageEvent): DurableMessage => event.message as DurableMessage;

vi.mock("../utils/proxy-fetch.js", () => ({
  fetchWithProxy: async (_url: string, init: RequestInit) => {
    const body = JSON.parse(String(init.body)) as { tools: Array<{ function: { name: string } }> };
    const names = body.tools.map((tool) => tool.function.name);
    const reply = (name: string, args: unknown) => new Response(JSON.stringify({
      choices: [{
        finish_reason: "tool_calls",
        message: { tool_calls: [{ id: `call-${name}`, type: "function", function: { name, arguments: JSON.stringify(args) } }] },
      }],
    }));
    if (names.includes("finish_turn")) return reply("finish_turn", { status: "blocked", message: "Fixture completion." });
    if (names[0] === "submit_requested_operations") {
      return reply("submit_requested_operations", { coverQuote: "", preDraftReviewQuote: "", contentReviewQuote: "", exportQuote: "" });
    }
    throw new Error(`Unexpected fixture request: ${names.join(",")}`);
  },
}));

const model: Model<"openai-completions"> = {
  id: "test-model",
  name: "test-model",
  api: "openai-completions",
  provider: "openai",
  baseUrl: "https://example.test/v1",
  reasoning: false,
  input: ["text"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 128_000,
  maxTokens: 8192,
};

const SESSION_ID = "resume-preparation";

afterEach(async () => {
  evictAgentCache(SESSION_ID);
});

async function fixture(): Promise<{ root: string; config: AgentSessionConfig }> {
  const root = await mkdtemp(join(tmpdir(), "inkos-agent-preparation-"));
  await saveWorkManifest(root, createWorkManifest({ id: "work", title: "Gallery", profileId: "script", language: "en" }));
  await mkdir(join(root, "works/work/source"), { recursive: true });
  await syncWorkSourceArtifacts({
    projectRoot: root,
    workId: "work",
    accept: true,
    writes: [{ relativePath: "works/work/source/script.md", content: "Nora closes the gallery.\n" }],
  });
  const client = createLLMClient({
    provider: "openai",
    service: "custom",
    configSource: "studio",
    baseUrl: model.baseUrl,
    model: model.id,
    apiKey: testKey,
    apiFormat: "chat",
    stream: false,
    thinkingBudget: 0,
  });
  const config: AgentSessionConfig = {
    projectRoot: root,
    sessionId: SESSION_ID,
    sessionKind: "work",
    workId: "work",
    bookId: null,
    profileId: "script",
    language: "en",
    model,
    apiKey: testKey,
    stream: false,
    pipeline: new PipelineRunner({ client, model: model.id, projectRoot: root }),
  };
  return { root, config };
}

/** 一个已执行完的续投动作；`operationReceipts` 是它会核对的回执。 */
function resumedAction(operationReceipts: unknown[]): NonNullable<AgentSessionConfig["resumeAction"]> {
  return {
    toolCallId: "call-resume",
    capabilityId: "script",
    actionId: "export_work",
    parameters: { workId: "work" },
    result: {
      status: "success",
      summary: "Fixture export.",
      images: [],
      artifacts: [],
      observations: [],
      operationReceipts: operationReceipts as never,
    },
  };
}

it("续投路径：回合开始前的失败不留残留——请求关档、缓存项丢弃、下一次请求不叠加", async () => {
  const { root, config } = await fixture();
  try {
    // 回执指向不存在的修订 → 送达账本核对抛错（发生在回合的 try 之外）。
    const staleReceipt = { operation: "export", sources: [{ workId: "work", artifactId: "missing-artifact", revisionId: "missing-revision" }] };
    await expect(
      runAgentSession({ ...config, resumeAction: resumedAction([staleReceipt]) }, "Continue."),
    ).rejects.toMatchObject({ code: "ARTIFACT_NOT_FOUND" });

    // 请求关档：request_started 之后是 request_failed，没有半截的续投消息事件。
    const failed = await readTranscriptEvents(root, SESSION_ID);
    expect(failed.map((event) => event.type)).toEqual(["session_created", "request_started", "request_failed"]);
    expect(failed.at(-1)).toMatchObject({ type: "request_failed" });

    // 回执合法 → 照常完成。缓存里没有上一次留下的交换，也没有残留的订阅。
    const completed = await runAgentSession({ ...config, resumeAction: resumedAction([]) }, "Continue.");
    expect(completed.completion?.status).toBe("blocked");
    expect(
      completed.messages.filter((message) => message.role === "toolResult" && message.toolCallId === "call-resume"),
    ).toHaveLength(1);

    const events = await readTranscriptEvents(root, SESSION_ID);
    const resumedResults = messageEvents(events).filter(
      (event) => event.role === "toolResult" && durable(event).toolCallId === "call-resume",
    );
    expect(resumedResults).toHaveLength(1);
    // 残留订阅会把第二次请求的事件再落盘一遍；这里每个事件只有一份。
    const finishCalls = messageEvents(events).filter(
      (event) => event.role === "assistant"
        && (durable(event).content ?? []).some((part) => part.type === "toolCall" && part.name === "finish_turn"),
    );
    expect(finishCalls).toHaveLength(1);
    const committed = events.filter((event) => event.type === "request_committed");
    expect(committed).toHaveLength(1);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 30000);

it("普通回合：送达状态落盘失败同样关档并丢弃缓存项，下一次请求照常", async () => {
  const { root, config } = await fixture();
  try {
    let fail = true;
    const onDeliveryStateChange = async () => {
      if (fail) throw new Error("Fixture delivery sink failure");
    };
    await expect(
      runAgentSession({ ...config, onDeliveryStateChange }, "Describe the gallery."),
    ).rejects.toThrow("Fixture delivery sink failure");
    const failed = await readTranscriptEvents(root, SESSION_ID);
    expect(failed.at(-1)).toMatchObject({ type: "request_failed", error: "Fixture delivery sink failure" });
    // 没有模型调用发生过：turn_start 不会出现。
    expect(failed.some((event) => event.type === "message")).toBe(false);

    fail = false;
    const completed = await runAgentSession({ ...config, onDeliveryStateChange }, "Describe the gallery.");
    expect(completed.completion?.status).toBe("blocked");
    const finishCalls = messageEvents(await readTranscriptEvents(root, SESSION_ID)).filter(
      (event) => event.role === "assistant"
        && (durable(event).content ?? []).some((part) => part.type === "toolCall" && part.name === "finish_turn"),
    );
    expect(finishCalls).toHaveLength(1);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 30000);
