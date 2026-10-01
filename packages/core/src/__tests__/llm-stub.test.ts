import { describe, expect, it, afterEach } from "vitest";
import { isLlmStubEnabled, stubChatCompletion, stubWorkerStructuredResult } from "../agent/llm-stub.js";
import { guardedAgentStream } from "../agent/pi-stream.js";
import { Type } from "@sinclair/typebox";

describe("llm-stub", () => {
  const prev = process.env.INKOS_AGENT_LLM_STUB;
  afterEach(() => {
    if (prev === undefined) delete process.env.INKOS_AGENT_LLM_STUB;
    else process.env.INKOS_AGENT_LLM_STUB = prev;
  });

  it("isLlmStubEnabled reflects the env var", () => {
    process.env.INKOS_AGENT_LLM_STUB = "1";
    expect(isLlmStubEnabled()).toBe(true);
    delete process.env.INKOS_AGENT_LLM_STUB;
    expect(isLlmStubEnabled()).toBe(false);
  });

  it("stubChatCompletion returns a valid structure JSON for a structure prompt", () => {
    const res = stubChatCompletion(
      [
        { role: "system", content: "生成分支骨架 JSON：{nodes:[...]}" },
        { role: "user", content: "三幕" },
      ],
      "stub-model",
    );
    const parsed = JSON.parse(res.content) as { nodes: unknown[] };
    expect(Array.isArray(parsed.nodes)).toBe(true);
    expect(parsed.nodes.length).toBeGreaterThanOrEqual(2);
  });

  it("stubWorkerStructuredResult parses only when the stub provider is active (R45/567)", () => {
    // 选择语义测试：schema 放宽为 Unknown（stub JSON 形状契约由
    // agent-session 测试经真实 resultTool 覆盖）。
    const resultTool = {
      name: "submit_result",
      parameters: Type.Unknown(),
    } as never;
    delete process.env.INKOS_AGENT_LLM_STUB;
    expect(stubWorkerStructuredResult([{ role: "user", content: "三幕" }], "m", resultTool)).toBeUndefined();

    process.env.INKOS_AGENT_LLM_STUB = "1";
    const parsed = stubWorkerStructuredResult(
      [{ role: "user", content: "生成分支骨架 JSON：{nodes:[...]}" }],
      "m",
      resultTool,
    );
    expect(parsed).toBeDefined();
  });

  it("guardedAgentStream selects by provider, not by consumer (R45/567)", async () => {
    delete process.env.INKOS_AGENT_LLM_STUB;
    expect(isLlmStubEnabled()).toBe(false);
    process.env.INKOS_AGENT_LLM_STUB = "1";
    // stub 面走 stubAgentStream：对未解析 pi model 不炸（stub 不触网），
    // 事件流可同步产出到 done——真传输则会先做窗口守卫断言。
    const stream = guardedAgentStream({ id: "stub", api: "openai-completions" } as never, { messages: [{ role: "user", content: "你好", toolName: undefined }] } as never);
    expect(typeof stream[Symbol.asyncIterator]).toBe("function");
  });
});
