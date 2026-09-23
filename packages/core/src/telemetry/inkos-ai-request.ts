import { z } from "zod";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import type { TelemetryContext, TelemetrySpan } from "@earendil-works/pi-telemetry";
import { NOOP_TELEMETRY_CONTEXT } from "@earendil-works/pi-telemetry";

/**
 * R33 遥测（551 号；543 号施工图 §4）：`inkos.ai.request` span 契约。
 *
 * - schema 单一事实源 = golden 第 31 守门域
 *   `__tests__/golden/inkos-ai-request-schema.json`（双端共享；Rust serde 侧
 *   只投影不引运行时）。语义重合键镜像上游 `pi.ai.request`（AI_TELEMETRY_SCHEMA
 *   0.87 实测），域扩展键 `inkos.*`（RunLogEntry 403 号九字段映射：
 *   model→`pi.ai.model`、durationMs→span 时长、ok→status、agent/attemptIndex/
 *   round/tookOver/errorKind→`inkos.*`）。
 * - 原语直接消费上游 pi-telemetry（0.87 传递依赖升直接依赖）：
 *   `TelemetryContext` 显式传参 + `NOOP_TELEMETRY_CONTEXT` 缺省（零行为）+
 *   `InMemoryTelemetryContext` 参考/测试实现（上游同款）。
 * - run-log 环形缓冲维持为存储投影（403 号不动）；隐私红线不变——只元数据，
 *   不含 prompt/正文。
 */

export { NOOP_TELEMETRY_CONTEXT } from "@earendil-works/pi-telemetry";
export type { TelemetryContext, TelemetrySpan } from "@earendil-works/pi-telemetry";

export const INKOS_AI_REQUEST_SPAN = "inkos.ai.request";

// ── schema 校验（golden JSON 加载契约）──

const attributeDefinitionSchema = z.object({
  type: z.enum(["string", "number", "boolean", "string[]", "number[]", "boolean[]"]),
  required: z.boolean().optional(),
  values: z.array(z.union([z.string(), z.number(), z.boolean()])).optional(),
  elementValues: z.array(z.string()).optional(),
  examples: z.array(z.any()).optional(),
  description: z.string(),
  cardinality: z.enum(["low", "high"]).optional(),
  sensitive: z.boolean().optional(),
});

export const telemetrySchemaSchema = z.object({
  version: z.number(),
  spans: z.record(
    z.string(),
    z.object({
      description: z.string(),
      parents: z.object({ kind: z.enum(["any", "root_or_external", "spans"]) }),
      startAttributes: z.record(z.string(), attributeDefinitionSchema),
      endAttributes: z.record(z.string(), attributeDefinitionSchema),
      status: z.object({ default: z.literal("ok"), errorWhen: z.string() }),
    }),
  ),
});

export type InkosTelemetrySchema = z.infer<typeof telemetrySchemaSchema>;

function loadGoldenSchema(): InkosTelemetrySchema {
  const path = join(fileURLToPath(new URL(".", import.meta.url)), "..", "__tests__", "golden", "inkos-ai-request-schema.json");
  const parsed = telemetrySchemaSchema.parse(JSON.parse(readFileSync(path, "utf8")));
  if (!parsed.spans[INKOS_AI_REQUEST_SPAN]) {
    throw new Error(`telemetry schema 缺少 ${INKOS_AI_REQUEST_SPAN} span 定义`);
  }
  return parsed;
}

let cachedSchema: InkosTelemetrySchema | undefined;

/** 加载并校验第 31 守门域 schema（进程内缓存；dist 形态下 golden 文件须随包发布）。 */
export function loadInkosAiRequestSchema(): InkosTelemetrySchema {
  cachedSchema ??= loadGoldenSchema();
  return cachedSchema;
}

// ── 属性构造器（RunLogEntry 语义 → span 属性）──

export type InkosAiRequestOperation = "stream" | "fetch_deferred" | "cancel_deferred" | "generate_images";

export interface InkosAiRequestInput {
  readonly operation: InkosAiRequestOperation;
  /** provider id（service-resolver 的 piProvider 映射）。 */
  readonly provider: string;
  readonly model: string;
  readonly api: string;
  readonly streaming: boolean;
  /** agent 名 / 调用标签（RunLogEntry.agent）。 */
  readonly agent: string;
  /** 链内序号，0 = primary。 */
  readonly attemptIndex: number;
  /** 轮次（1 起）。 */
  readonly round: number;
  /** R25 联动：非 primary 首轮命中即接管。 */
  readonly tookOver: boolean;
}

export interface InkosAiRequestOutcome {
  readonly responseModel?: string;
  readonly errorKind?: "transient" | "fatal";
}

/** start 属性全集（schema required 全覆盖）。 */
export function inkosAiRequestStartAttributes(
  input: InkosAiRequestInput,
): Record<string, string | number | boolean> {
  return {
    "pi.ai.operation": input.operation,
    "pi.ai.provider": input.provider,
    "pi.ai.model": input.model,
    "pi.ai.api": input.api,
    "pi.ai.streaming": input.streaming,
    "inkos.agent": input.agent,
    "inkos.attempt_index": input.attemptIndex,
    "inkos.round": input.round,
    "inkos.took_over": input.tookOver,
  };
}

/** end 属性（可缺省键按 schema 可选面）。 */
export function inkosAiRequestEndAttributes(
  outcome: InkosAiRequestOutcome,
): Record<string, string | number | boolean> {
  const attributes: Record<string, string | number | boolean> = {};
  if (outcome.responseModel !== undefined) attributes["pi.ai.response.model"] = outcome.responseModel;
  if (outcome.errorKind !== undefined) attributes["inkos.error_kind"] = outcome.errorKind;
  return attributes;
}

/**
 * 上游模式接线：span 内执行请求体。telemetry 缺省 NOOP——callback 直接执行，
 * 零行为零开销；InMemoryTelemetryContext 供测试/独立录制域。
 */
export async function runInkosAiRequestSpan<T>(
  telemetry: TelemetryContext | undefined,
  input: InkosAiRequestInput,
  request: (span: TelemetrySpan) => Promise<T>,
): Promise<T> {
  return (telemetry ?? NOOP_TELEMETRY_CONTEXT).startSpan(
    { name: INKOS_AI_REQUEST_SPAN, attributes: inkosAiRequestStartAttributes(input) },
    request,
  );
}
