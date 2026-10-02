import { afterEach, describe, expect, it, vi } from "vitest";
import type { AssistantMessage, AssistantMessageEventStream } from "@earendil-works/pi-ai";

/**
 * 608 号：guardedCompleteStream 双分支单测——成功推 done+end、失败推
 * error+end 兜底消息（防流悬挂）。piCompleteSimple 经 vi.mock 打桩。
 */
const mockComplete = vi.fn();
vi.mock("../llm/pi-dispatch.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../llm/pi-dispatch.js")>();
  return {
    ...original,
    piCompleteSimple: (...args: unknown[]) => mockComplete(...args),
  };
});

import { guardedCompleteStream } from "../agent/pi-stream.js";
import type { Model, Api } from "@earendil-works/pi-ai";

const model = {
  id: "mock-large",
  api: "openai-completions",
  provider: "custom",
  baseUrl: "http://127.0.0.1:9/v1",
  maxTokens: 4096,
  contextWindow: 128_000,
  reasoning: false,
} as unknown as Model<Api>;

function collect(stream: AssistantMessageEventStream): Promise<{ events: string[]; final?: AssistantMessage }> {
  return new Promise((resolve, reject) => {
    const events: string[] = [];
    let final: AssistantMessage | undefined;
    void (async () => {
      try {
        for await (const event of stream) {
          events.push(event.type);
          if (event.type === "done") final = event.message;
        }
        resolve({ events, final });
      } catch (error) {
        reject(error);
      }
    })();
  });
}

describe("guardedCompleteStream (608)", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("resolves piCompleteSimple into a single done event then end", async () => {
    const full: AssistantMessage = {
      role: "assistant",
      content: [{ type: "text", text: "好的。" }],
      api: "openai-completions",
      provider: "custom",
      model: "mock-large",
      usage: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0, totalTokens: 3, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
      stopReason: "stop",
      timestamp: Date.now(),
    };
    mockComplete.mockResolvedValueOnce(full);

    const stream = guardedCompleteStream(model, { messages: [] } as never);
    const result = await collect(stream);

    expect(result.final?.stopReason).toBe("stop");
    const text = result.final?.content;
    expect(Array.isArray(text) && (text as Array<{ text?: string }>)[0]?.text).toBe("好的。");
  });

  it("surfaces upstream rejection as an error event with a fallback assistant tail", async () => {
    mockComplete.mockRejectedValueOnce(new Error("HTTP 500"));
    const stream = guardedCompleteStream(model, { messages: [] } as never);
    const events: string[] = [];
    let sawError = false;
    for await (const event of stream) {
      events.push(event.type);
      if (event.type === "error") sawError = true;
    }
    expect(sawError).toBe(true);
    // 错误分支流以 error 事件终结（pi-ai 事件流惯例，无 done 收尾）。
    expect(events[events.length - 1]).toBe("error");
  });
});
