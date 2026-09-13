//! R23/394 号：伏笔类型接线行为断言（LLM 边界容错 + 台账往返 + 归并语义）。
import { describe, expect, it } from "vitest";
import { parseSettlerDeltaOutput } from "../agents/settler-delta-parser.js";
import { parsePendingHooksMarkdown, renderHookSnapshot } from "../utils/story-markdown.js";

function deltaJson(upsert: unknown[], candidates: unknown[] = []): string {
  return `=== POST_SETTLEMENT ===\n结算。\n\n=== RUNTIME_STATE_DELTA ===\n\`\`\`json\n${JSON.stringify({
    chapter: 12,
    hookOps: { upsert, mention: [], resolve: [], defer: [] },
    newHookCandidates: candidates,
  })}\n\`\`\`\n`;
}

describe("hook kind wiring (R23)", () => {
  it("normalizes llm kinds at the delta boundary and drops unknowns", () => {
    const got = parseSettlerDeltaOutput(deltaJson([
      {
        hookId: "mentor-oath", startChapter: 8, type: "relationship",
        kind: "情感", status: "progressing", lastAdvancedChapter: 12,
      },
      {
        hookId: "sword-origin", startChapter: 3, type: "mystery",
        kind: "plot", status: "open", lastAdvancedChapter: 12,
      },
    ]));
    const [first, second] = got.runtimeStateDelta.hookOps.upsert;
    expect(first!.kind).toBe("emotion");
    expect(second!.kind).toBeUndefined();
  });

  it("round-trips kind through the 14-column ledger", () => {
    const markdown = [
      "| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 | 分类 |",
      "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
      "| mentor-oath | 8 | relationship | progressing | 12 | 揭开师债真相 | slow-burn |  |  |  |  |  | 师债线 | emotion |",
      "| legacy-hook | 2 | mystery | open | 2 | 真相 |  |  |  |  |  |  | 旧表无 kind | |",
    ].join("\n");
    const hooks = parsePendingHooksMarkdown(markdown);
    expect(hooks[0]!.kind).toBe("emotion");
    expect(hooks[1]!.kind).toBeUndefined();

    const rendered = renderHookSnapshot(hooks, "zh");
    expect(rendered).toContain("| emotion |");
    expect(rendered).toContain("备注 | 分类 |");
    // 渲染产物再解析：kind 保持（往返闭环）。
    const reparsed = parsePendingHooksMarkdown(rendered);
    expect(reparsed[0]!.kind).toBe("emotion");
    expect(reparsed[1]!.kind).toBeUndefined();
  });
});
