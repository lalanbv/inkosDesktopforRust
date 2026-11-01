import { describe, expect, it } from "vitest";
import type { HookRecord } from "../models/runtime-state.js";
import { filterActiveHooks, normalizeStoredHookStatus, resolveHookStatusAlias } from "../utils/hook-lifecycle.js";
import { parsePendingHooksMarkdown } from "../utils/story-markdown.js";
import { renderHooksProjection } from "../state/state-projections.js";

// 805 号：撤回终态在「markdown 字符串 → 状态」解析层的对等——HookStatusSchema 早已
// 含 superseded，但别名表未跟，md 投影往返会把已撤回前提复活为 open。

function hook(hookId: string, status: HookRecord["status"]): HookRecord {
  return { hookId, startChapter: 1, type: "promise", status, lastAdvancedChapter: 1, expectedPayoff: "soon", notes: "n" };
}

describe("superseded hook status in the string parsing layer", () => {
  it("resolves superseded aliases and keeps them out of the open fallback", () => {
    expect(resolveHookStatusAlias("superseded")).toBe("superseded");
    expect(normalizeStoredHookStatus(" Withdrawn ")).toBe("superseded");
    expect(normalizeStoredHookStatus("已撤回")).toBe("superseded");
    expect(normalizeStoredHookStatus("unknown")).toBe("open");
  });

  it("treats superseded as non-active debt alongside resolved and deferred", () => {
    const stored = [
      { ...hook("open", "open") },
      { ...hook("gone", "superseded") },
      { ...hook("done", "resolved") },
      { ...hook("later", "deferred") },
      { ...hook("live", "progressing") },
    ];
    expect(filterActiveHooks(stored).map((h) => h.hookId)).toEqual(["open", "live"]);
  });

  it("round-trips a superseded hook through the markdown projection without reviving it", () => {
    const markdown = renderHooksProjection({ hooks: [hook("gone", "superseded"), hook("open", "open")] }, "zh");
    const parsed = parsePendingHooksMarkdown(markdown);
    expect(parsed.find((h) => h.hookId === "gone")?.status).toBe("superseded");
    expect(parsed.find((h) => h.hookId === "open")?.status).toBe("open");
  });
});
