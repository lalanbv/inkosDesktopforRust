import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdtemp, mkdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { readTranscriptEvents } from "../interaction/session-transcript.js";
import {
  evictAgentCache,
  markPendingSessionAbort,
  runAgentSession,
  takePendingSessionAbort,
} from "../agent/agent-session.js";

const EMPTY_USAGE = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
};

const { heldStreamCompletions, heldStreamWaiters, piMocks } = vi.hoisted(() => ({
  heldStreamCompletions: [] as Array<() => void>,
  heldStreamWaiters: [] as Array<() => void>,
  piMocks: {
    streamSimple: vi.fn(),
    getEnvApiKey: vi.fn(() => "fake-key"),
    completeSimple: vi.fn(),
    getModel: vi.fn((provider: string, id: string) => ({
      provider,
      id,
      name: id,
      api: "anthropic-messages",
      baseUrl: "",
      reasoning: false,
      input: ["text"],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 200_000,
      maxTokens: 4096,
    })),
  },
}));

vi.mock("@earendil-works/pi-ai", async () => {
  const actual = await vi.importActual<any>("@earendil-works/pi-ai");

  function assistant(text: string, timestamp = Date.now()) {
    return {
      role: "assistant",
      content: [{ type: "text", text }],
      api: "anthropic-messages",
      provider: "anthropic",
      model: "fake",
      usage: EMPTY_USAGE,
      stopReason: "stop",
      timestamp,
    };
  }

  const streamSimple = piMocks.streamSimple;
  streamSimple.mockImplementation((model: any, context: any, options: any) => {
    void model;
    void options;
    const last = context.messages.at(-1);
    const prompt = last?.role === "user" && typeof last.content === "string"
      ? last.content
      : Array.isArray(last?.content)
        ? (last.content as any[]).filter((b) => b?.type === "text").map((b) => b.text).join("")
        : "";
    const stream = actual.createAssistantMessageEventStream();
    const done = () => stream.push({ type: "done", reason: "stop", message: assistant(`回复：${prompt}`) });
    if (prompt.startsWith("hold")) {
      heldStreamCompletions.push(done);
      heldStreamWaiters.splice(0).forEach((resolve) => resolve());
    } else {
      done();
    }
    return stream;
  });

  return { ...actual, ...piMocks };
});

vi.mock("../llm/pi-dispatch.js", () => ({
  piStreamSimple: piMocks.streamSimple,
  piCompleteSimple: piMocks.completeSimple,
}));
vi.mock("../llm/pi-env-keys.js", () => ({
  getEnvApiKey: piMocks.getEnvApiKey,
}));

let projectRoot: string;

beforeEach(async () => {
  projectRoot = await mkdtemp(join(tmpdir(), "inkos-pending-abort-"));
  await mkdir(join(projectRoot, "books"), { recursive: true });
  heldStreamCompletions.length = 0;
  heldStreamWaiters.length = 0;
  config = {
    sessionId: "pa-session",
    bookId: null,
    language: "zh",
    pipeline,
    projectRoot,
    model,
  };
});

afterEach(async () => {
  evictAgentCache("pa-session");
  await rm(projectRoot, { recursive: true, force: true });
});

const model = { provider: "x", id: "y", api: "anthropic-messages" } as any;
const pipeline = {} as any;
// config 在 beforeEach 重建：projectRoot 由 mkdtemp 每例生成，顶层字面量会
// 捕获 undefined（对象字面量求值时机陷阱，非闭包引用）。
let config: Parameters<typeof runAgentSession>[0];

function waitForHold(): Promise<void> {
  if (heldStreamCompletions.length > 0) return Promise.resolve();
  return new Promise((resolve) => heldStreamWaiters.push(resolve));
}

describe("pending-abort 排队态取消（640 号）", () => {
  it("排队轮在等待期间被 abort：受理即中止（started+user+failed，无 committed，errorMessage=aborted）", async () => {
    const turnA = runAgentSession(config, "hold 第一轮在飞");
    await waitForHold(); // A 已入队并在飞（hold 占住队列）
    const turnB = runAgentSession(config, "排队第二轮"); // B 排队等待
    await new Promise((resolve) => setTimeout(resolve, 30)); // 确保 B 已入队
    markPendingSessionAbort(projectRoot, "pa-session"); // 用户停止：在飞 A 常规链 + 排队 B 置标记

    heldStreamCompletions.splice(0).forEach((done) => done()); // 放行 A
    const resultA = await turnA;
    expect(resultA.errorMessage).toBeUndefined();
    const resultB = await turnB;
    expect(resultB.errorMessage).toBe("aborted");
    expect(resultB.responseText).toBe("");
    expect(resultB.messages).toHaveLength(0);

    const events = await readTranscriptEvents(projectRoot, "pa-session");
    const byType = events.reduce<Record<string, number>>((acc, event) => {
      acc[event.type] = (acc[event.type] ?? 0) + 1;
      return acc;
    }, {});
    // A 轮完整（started+user+assistant+committed），B 轮受理即中止（started+user+failed）
    expect(byType["request_started"]).toBe(2);
    expect(byType["request_committed"]).toBe(1);
    expect(byType["request_failed"]).toBe(1);
    const failed = events.find((event) => event.type === "request_failed");
    expect(failed && "error" in failed && failed.error).toBe("aborted");
    // B 轮 user 消息事件在场（与在飞中止 transcript 同形）
    const userTexts = events
      .filter((event): event is Extract<typeof event, { type: "message" }> => event.type === "message")
      .filter((event) => event.role === "user")
      .map((event) => (event.message as { content?: unknown })?.content);
    expect(userTexts).toContain("排队第二轮");
  });

  it("标记不误杀后续新轮：中止后新消息正常受理并 committed（自愈）", async () => {
    const turnA = runAgentSession(config, "hold 占队列");
    await waitForHold();
    const turnB = runAgentSession(config, "将被取消的排队轮");
    await new Promise((resolve) => setTimeout(resolve, 30));
    markPendingSessionAbort(projectRoot, "pa-session");
    heldStreamCompletions.splice(0).forEach((done) => done());
    await turnA;
    expect((await turnB).errorMessage).toBe("aborted");

    // 标记已被 B 的受理消费：新轮正常
    const turnC = await runAgentSession(config, "中止后的新消息");
    expect(turnC.errorMessage).toBeUndefined();
    expect(turnC.responseText).toBe("回复：中止后的新消息");
    const events = await readTranscriptEvents(projectRoot, "pa-session");
    expect(events.filter((event) => event.type === "request_committed")).toHaveLength(2);
  });

  it("空闲时置位不误杀下一轮：标记因入队序更新未命中并自清除", async () => {
    markPendingSessionAbort(projectRoot, "pa-session"); // 空闲停止（无目标）
    const result = await runAgentSession(config, "空闲后的新消息");
    expect(result.errorMessage).toBeUndefined();
    expect(result.responseText).toBe("回复：空闲后的新消息");
    const events = await readTranscriptEvents(projectRoot, "pa-session");
    expect(events.filter((event) => event.type === "request_committed")).toHaveLength(1);
    expect(events.filter((event) => event.type === "request_failed")).toHaveLength(0);
  });
});
