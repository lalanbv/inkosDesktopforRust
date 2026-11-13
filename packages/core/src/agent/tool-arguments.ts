import type { BeforeToolCallContext } from "@mariozechner/pi-agent-core";
import { Value } from "@sinclair/typebox/value";
import type { TSchema } from "@sinclair/typebox";
import { decodeStructuredFields } from "./structured-arguments.js";

// 1019 号：typebox 1.x 的 Value.Errors 产物=JSON Schema 2020-12 校验器形态
// （keyword/instancePath/params/message——探针实锤），与 0.x（path/type/
// message/schema/value）完全不同。本适配层从 1.x 形态重建 toolArgumentIssues
// 的输出契约（path/type/message/actualType/expectedType/allowedValues/
// instruction——给 LLM 的结构化修复提示格式不变）：
// - path ← instancePath（格式同形 `/a/0`）；
// - expectedType ← keyword==="type" 的 params.type；
// - allowedValues ← Union-of-Literal 在 1.x 展开为多条 keyword="const"
//   （params.allowedValue），按 instancePath 聚合重建 0.x 的 enum 语义；
// - received ← 1.x 错误对象不带 value，从调用方的 value 按 instancePath 定位。
interface RawError {
  readonly keyword?: string;
  readonly instancePath?: string;
  readonly params?: Record<string, unknown>;
  readonly message?: string;
  readonly type?: string;
  readonly path?: string;
  readonly schema?: unknown;
  readonly value?: unknown;
}

interface ShapedError {
  readonly keyword: string;
  readonly path: string;
  readonly params: Record<string, unknown>;
  readonly message: string;
  readonly failedSchema?: unknown;
  readonly received?: unknown;
}

function valueAtPath(root: unknown, instancePath: string): unknown {
  if (!instancePath) return root;
  let current: unknown = root;
  for (const rawSegment of instancePath.split("/").slice(1)) {
    const segment = rawSegment.replace(/~1/g, "/").replace(/~0/g, "~");
    if (Array.isArray(current)) {
      const index = Number(segment);
      if (!Number.isInteger(index) || index < 0 || index >= current.length) return undefined;
      current = current[index];
    } else if (current && typeof current === "object") {
      current = (current as Record<string, unknown>)[segment];
    } else {
      return undefined;
    }
  }
  return current;
}

/** Bounded repair facts from the declared schema, without echoing submitted prose. */
export function toolArgumentIssues(schema: TSchema, value: unknown) {
  // 1019 号：双版本归一——0.34 错误对象={path,type,message,schema,value}；
  // 1.x（typebox 新包名）={keyword,instancePath,params,message}（2020-12 校验
  // 器形态，不带 value）。字段双源读取，消费面输出契约不变。
  const raw: ReadonlyArray<ShapedError> = [...Value.Errors(schema, value)]
    .slice(0, 32)
    .map((e): ShapedError => {
      const dual = e as unknown as RawError;
      return {
        keyword: dual.keyword ?? dual.type ?? "",
        path: dual.path ?? dual.instancePath ?? "",
        params: dual.params ?? {},
        message: dual.message ?? "",
        failedSchema: (dual as { schema?: unknown }).schema,
        received: (dual as { value?: unknown }).value,
      };
    });
  // Union-of-Literal 在 1.x 展开为多条 const（每字面量一条）——按 path 聚合
  // 重建 0.x 的 allowedValues（enum）语义；0.34 下 failed.schema.enum/const
  // 直接给值（双源）。
  const allowedByPath = new Map<string, unknown[]>();
  for (const e of raw) {
    if (e.keyword === "const" && "allowedValue" in e.params) {
      const list = allowedByPath.get(e.path) ?? [];
      list.push(e.params.allowedValue);
      allowedByPath.set(e.path, list);
    }
    const failed = e.failedSchema as { enum?: unknown[]; const?: unknown; anyOf?: ReadonlyArray<Record<string, unknown>> } | undefined;
    if (Array.isArray(failed?.enum)) {
      allowedByPath.set(e.path, [...failed.enum]);
    } else if (failed && "const" in failed) {
      allowedByPath.set(e.path, [failed.const]);
    } else if (Array.isArray(failed?.anyOf) && failed.anyOf.every(branch => Object.hasOwn(branch, "const"))) {
      // 0.34 的 Union-of-Literal：failed=union schema，分支 const 聚合（原实现路径）
      allowedByPath.set(e.path, failed.anyOf.map(branch => (branch as { const: unknown }).const));
    }
  }
  return raw.map((e) => {
    const received = e.received !== undefined ? e.received : valueAtPath(value, e.path);
    const actualType = received === null ? "null" : Array.isArray(received) ? "array" : typeof received;
    const failed = e.failedSchema as { type?: unknown } | undefined;
    const expectedType = e.keyword === "type" && typeof e.params.type === "string"
      ? e.params.type
      : typeof failed?.type === "string" ? failed.type : undefined;
    const choices = allowedByPath.get(e.path);
    const allowedValues = choices && choices.length <= 32
      && choices.every(choice => choice === null || ["string", "number", "boolean"].includes(typeof choice))
      && JSON.stringify(choices).length <= 1024 ? choices : undefined;
    const instruction = actualType === "string" && (expectedType === "array" || expectedType === "object")
      ? `Submit this field as a native JSON ${expectedType}, not a string containing JSON. Correct the listed fields and resubmit the tool arguments.` : undefined;
    return {path:e.path,type:e.keyword,message:e.message,actualType,...(expectedType ? {expectedType} : {}),
      ...(instruction ? {instruction} : {}),...(allowedValues ? {allowedValues} : {})};
  }).slice(0, 16);
}

/**
 * Pi validates a cloned argument object with scalar coercion enabled. A union
 * can therefore turn a valid boolean or numeric string into a number. Domain
 * actions must receive the original, schema-valid JSON types.
 */
export async function preserveToolArgumentTypes(context: BeforeToolCallContext): Promise<undefined> {
  const tool = context.context.tools?.find((candidate) => candidate.name === context.toolCall.name);
  if (!tool) return;
  const original = decodeStructuredFields(tool.parameters, context.toolCall.arguments);
  const issues = toolArgumentIssues(tool.parameters, original);
  if (issues.length) {
    throw Object.assign(new Error(JSON.stringify({
      code: "TOOL_SCHEMA_INVALID", tool: tool.name, issues,
    })), { code: "TOOL_SCHEMA_INVALID" });
  }
  // BeforeToolCall receives the same object Pi will pass to execute. Replace
  // its values only after non-coercing validation; never guess missing values.
  const args = context.args;
  if (!args || typeof args !== "object" || Array.isArray(args)) {
    throw Object.assign(new Error("Tool arguments must be an object"), { code: "TOOL_SCHEMA_INVALID" });
  }
  const fields = args as Record<string, unknown>;
  for (const key of Object.keys(fields)) delete fields[key];
  Object.assign(fields, structuredClone(original));
}
