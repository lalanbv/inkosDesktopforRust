import {
  RuntimeStateDeltaSchema,
  type RuntimeStateDelta,
} from "../models/runtime-state.js";
import { normalizeHookKind } from "../utils/hook-kind.js";

export interface SettlerDeltaOutput {
  readonly postSettlement: string;
  readonly runtimeStateDelta: RuntimeStateDelta;
}

function sanitizeJSON(str: string): string {
  return str
    .replace(/[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]/g, "")
    .replace(/,\s*([}\]])/g, "$1");
}

/**
 * R23/394 号：LLM 边界的 kind 容错归一化——upsert 条目与候选的 kind 值
 * 经别名表归一；词表外的值删除（undefined）而不是拒收整个结算增量。
 */
function sanitizeHookKinds(parsed: unknown): unknown {
  if (typeof parsed !== "object" || parsed === null) return parsed;
  const raw = parsed as Record<string, unknown>;
  const hookOps = raw.hookOps as Record<string, unknown> | undefined;
  if (Array.isArray(hookOps?.upsert)) {
    raw.hookOps = {
      ...hookOps,
      upsert: hookOps.upsert.map(sanitizeEntryKind),
    };
  }
  if (Array.isArray(raw.newHookCandidates)) {
    raw.newHookCandidates = raw.newHookCandidates.map(sanitizeEntryKind);
  }
  return parsed;
}

function sanitizeEntryKind(entry: unknown): unknown {
  if (typeof entry !== "object" || entry === null) return entry;
  const record = { ...(entry as Record<string, unknown>) };
  if ("kind" in record) {
    const normalized = normalizeHookKind(String(record.kind ?? ""));
    if (normalized) record.kind = normalized;
    else delete record.kind;
  }
  return record;
}

export function parseSettlerDeltaOutput(content: string): SettlerDeltaOutput {
  const extract = (tag: string): string => {
    const regex = new RegExp(
      `=== ${tag} ===\\s*([\\s\\S]*?)(?==== [A-Z_]+ ===|$)`,
    );
    const match = content.match(regex);
    return match?.[1]?.trim() ?? "";
  };

  const rawDelta = extract("RUNTIME_STATE_DELTA");
  if (!rawDelta) {
    throw new Error("runtime state delta block is missing");
  }

  const jsonPayload = stripCodeFence(rawDelta);
  let parsed: unknown;
  try {
    parsed = JSON.parse(sanitizeJSON(jsonPayload));
  } catch (error) {
    throw new Error(`runtime state delta is not valid JSON: ${String(error)}`);
  }

  try {
    return {
      postSettlement: extract("POST_SETTLEMENT"),
      runtimeStateDelta: RuntimeStateDeltaSchema.parse(sanitizeHookKinds(parsed)),
    };
  } catch (error) {
    throw new Error(`runtime state delta failed schema validation: ${String(error)}`);
  }
}

function stripCodeFence(value: string): string {
  const trimmed = value.trim();
  const fenced = trimmed.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/i);
  return fenced?.[1]?.trim() ?? trimmed;
}
