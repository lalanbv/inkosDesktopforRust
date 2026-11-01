import { describe, expect, it } from "vitest";
import type { HookRecord } from "../models/runtime-state.js";
import { filterActiveHooks, normalizeStoredHookStatus, resolveHookStatusAlias } from "../utils/hook-lifecycle.js";
import { parseMarkdownTableRows, parsePendingHooksMarkdown } from "../utils/story-markdown.js";
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

  // 811 号：撤回 notes（既有 + 换行 + 依据）经 md 往返无损——渲染 <br> 转义 ↔ 解析还原成对，
  // 且多行单元格不得把表格行截断（rust projections/story_markdown 同集）。
  it("keeps multi-line withdrawal notes intact across the markdown round trip", () => {
    const withdrawn = { ...hook("gone", "superseded"), notes: "母亲留下的碎镜在镜界发光。\n作者修订撤回：碎镜线并入 H01。" };
    const markdown = renderHooksProjection({ hooks: [withdrawn] }, "zh");
    expect(markdown.split("\n").filter((line) => line.includes("gone"))).toHaveLength(1);
    expect(markdown).toContain("<br>");
    const parsed = parsePendingHooksMarkdown(markdown);
    expect(parsed.find((h) => h.hookId === "gone")?.notes).toBe(withdrawn.notes);
    // 三种 <br> 形态均还原，普通单元格不受影响。
    expect(parseMarkdownTableRows("| a |\n| --- |\n| x<br>y<BR/>z<br />w |\n")).toEqual([["a"], ["x\ny\nz\nw"]]);
  });
});
