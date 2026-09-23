import { describe, expect, it } from "vitest";
import { InMemoryTelemetryContext } from "@earendil-works/pi-telemetry";
import {
  INKOS_AI_REQUEST_SPAN,
  NOOP_TELEMETRY_CONTEXT,
  inkosAiRequestEndAttributes,
  inkosAiRequestStartAttributes,
  loadInkosAiRequestSchema,
  runInkosAiRequestSpan,
} from "../telemetry/inkos-ai-request.js";
import type { InkosTelemetrySchema } from "../telemetry/inkos-ai-request.js";

/**
 * R33 遥测 schema 测试（551 号；543 号施工图 §4）：
 *  - golden 第 31 守门域加载与结构校验（双端共享单一事实源）；
 *  - 属性构造器覆盖 schema required 全集（镜像键+inkos.* 扩展）；
 *  - NOOP 缺省零行为 + InMemory 参考实现记录（上游 pi-telemetry 原语）。
 *  Rust 侧对偶校验见 engine-rs/tests/golden_inkos_telemetry_schema_diff.rs。
 */

const BASE_INPUT = {
  operation: "stream",
  provider: "deepseek",
  model: "duel-model",
  api: "openai-completions",
  streaming: true,
  agent: "writer",
  attemptIndex: 0,
  round: 1,
  tookOver: false,
} as const;

describe("inkos.ai.request schema (R33 / golden 第 31 守门域)", () => {
  it("loads and validates the shared golden schema", () => {
    const schema: InkosTelemetrySchema = loadInkosAiRequestSchema();
    expect(schema.version).toBe(1);
    const span = schema.spans[INKOS_AI_REQUEST_SPAN];
    expect(span).toBeDefined();
    expect(Object.keys(span.startAttributes)).toEqual([
      "pi.ai.operation",
      "pi.ai.provider",
      "pi.ai.model",
      "pi.ai.api",
      "pi.ai.streaming",
      "inkos.agent",
      "inkos.attempt_index",
      "inkos.round",
      "inkos.took_over",
    ]);
    expect(Object.keys(span.endAttributes)).toEqual(["pi.ai.response.model", "inkos.error_kind"]);
    expect(span.status).toEqual({ default: "ok", errorWhen: expect.any(String) });
  });

  it("keeps required start attributes in exact sync with the schema", () => {
    const schema = loadInkosAiRequestSchema();
    const span = schema.spans[INKOS_AI_REQUEST_SPAN];
    const required = Object.entries(span.startAttributes)
      .filter(([, definition]) => definition.required)
      .map(([name]) => name);
    // 552 号粒度修订后：只传 required 语义（不带 pi.ai.* 可选透出键）时，
    // 构造器输出必须与 required 全集精确相等。
    const constructed = Object.keys(
      inkosAiRequestStartAttributes({
        model: "duel-model",
        agent: "writer",
        attemptIndex: 0,
        round: 1,
        tookOver: false,
      }),
    );
    expect(constructed).toEqual(required);
  });

  it("exposes optional pi.ai.* keys only when provided (552 granularity revision)", () => {
    const withOptional = inkosAiRequestStartAttributes({
      ...BASE_INPUT,
      operation: "stream",
      provider: "deepseek",
      api: "openai-completions",
      streaming: true,
    });
    expect(withOptional["pi.ai.operation"]).toBe("stream");
    expect(withOptional["pi.ai.provider"]).toBe("deepseek");
    expect(withOptional["pi.ai.api"]).toBe("openai-completions");
    expect(withOptional["pi.ai.streaming"]).toBe(true);
  });

  it("mirrors upstream pi.ai.* semantic keys on the streaming surface", () => {
    const schema = loadInkosAiRequestSchema();
    const start = schema.spans[INKOS_AI_REQUEST_SPAN].startAttributes;
    expect(start["pi.ai.operation"].values).toContain("stream");
    // 552 号修订：provider 层细节四键降 optional（span=per governed attempt）。
    for (const key of ["pi.ai.operation", "pi.ai.provider", "pi.ai.api", "pi.ai.streaming"]) {
      expect(start[key].required).toBe(false);
    }
    expect(start["pi.ai.model"].required).toBe(true);
    expect(Object.keys(schema.spans[INKOS_AI_REQUEST_SPAN].endAttributes)).toContain("pi.ai.response.model");
  });
});

describe("runInkosAiRequestSpan", () => {
  it("is a zero-behavior pass-through under the NOOP default", async () => {
    const marker = Symbol("result");
    const result = await runInkosAiRequestSpan(undefined, BASE_INPUT, async () => marker);
    expect(result).toBe(marker);
    expect(NOOP_TELEMETRY_CONTEXT).toBeDefined();
  });

  it("records start attributes, status and end attributes on the InMemory reference backend", async () => {
    const telemetry = new InMemoryTelemetryContext();
    const outcome = await runInkosAiRequestSpan(telemetry, BASE_INPUT, async (span) => {
      span.setAttributes(inkosAiRequestEndAttributes({ responseModel: "resp-model" }));
      return "ok";
    });
    expect(outcome).toBe("ok");
    const spans = telemetry.getSpans();
    expect(spans).toHaveLength(1);
    const recorded = spans[0]!;
    expect(recorded.name).toBe(INKOS_AI_REQUEST_SPAN);
    expect(recorded.attributes).toMatchObject({
      "pi.ai.model": "duel-model",
      "pi.ai.provider": "deepseek",
      "inkos.agent": "writer",
      "inkos.attempt_index": 0,
      "inkos.round": 1,
      "inkos.took_over": false,
    });
    expect(recorded.status).toEqual({ status: "ok" });

    // 失败路径：errorKind 进 end 属性、span status=error。
    const failed = await runInkosAiRequestSpan(telemetry, { ...BASE_INPUT, attemptIndex: 1, tookOver: true }, async (span) => {
      span.setAttributes(inkosAiRequestEndAttributes({ errorKind: "transient" }));
      span.setStatus({ status: "error", error: { name: "LLMError", message: "503 upstream" } });
      return "fallback";
    });
    expect(failed).toBe("fallback");
    const failure = telemetry.getSpans()[1]!;
    expect(failure.attributes["inkos.attempt_index"]).toBe(1);
    expect(failure.attributes["inkos.took_over"]).toBe(true);
    expect(failure.status.status).toBe("error");
  });
});
