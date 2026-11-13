import type { BeforeToolCallContext } from "@mariozechner/pi-agent-core";
import { Value } from "@sinclair/typebox/value";
import type { TSchema } from "@sinclair/typebox";
import { decodeStructuredFields } from "./structured-arguments.js";

/** Bounded repair facts from the declared schema, without echoing submitted prose. */
export function toolArgumentIssues(schema: TSchema, value: unknown) {
  return [...Value.Errors(schema, value)].slice(0, 16).map(({path,type,message,schema: rawFailed,value: received}) => {
    // 1017/1018 号：failed（错误所在子 schema）的结构消费面用结构类型——
    // typebox 1.x 的错误对象 schema 字段类型与 0.x 不同，运行时字段
    // （anyOf/enum/const/type）双版本同在（duck-typing 与 1.x 实测一致）。
    const failed = rawFailed as unknown as {
      readonly type?: unknown;
      readonly const?: unknown;
      readonly enum?: unknown;
      readonly anyOf?: ReadonlyArray<Record<string, unknown>>;
    };
    const branches = failed.anyOf as Array<Record<string, unknown>> | undefined;
    const choices: unknown[] | undefined = Array.isArray(failed.enum) ? failed.enum
      : branches?.every(branch => Object.hasOwn(branch, "const")) ? branches.map(branch => branch.const)
      : Object.hasOwn(failed, "const") ? [failed.const] : undefined;
    const allowedValues = choices && choices.length <= 32
      && choices.every(choice => choice === null || ["string", "number", "boolean"].includes(typeof choice))
      && JSON.stringify(choices).length <= 1024 ? choices : undefined;
    const actualType = received === null ? "null" : Array.isArray(received) ? "array" : typeof received;
    const expectedType = typeof failed.type === "string" ? failed.type : undefined;
    const instruction = actualType === "string" && (expectedType === "array" || expectedType === "object")
      ? `Submit this field as a native JSON ${expectedType}, not a string containing JSON. Correct the listed fields and resubmit the tool arguments.` : undefined;
    return {path,type,message,actualType,...(expectedType ? {expectedType} : {}),
      ...(instruction ? {instruction} : {}),...(allowedValues ? {allowedValues} : {})};
  });
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
