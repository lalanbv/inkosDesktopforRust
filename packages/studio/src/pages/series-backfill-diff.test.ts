import { describe, expect, it } from "vitest";
import {
  buildBackfillDiff,
  diffLines,
  renderBackfillMarkdown,
} from "./series-backfill-diff";

describe("series-backfill diff（186 号回填差异预览）", () => {
  const items = [
    { id: "it-1", category: "worldview", title: "元气体系", content: "灵气分九品。" },
    { id: "it-2", category: "character", title: "林动", content: "隐忍坚韧的主角。" },
  ];

  it("markdown 渲染与双端 apply 写入格式一致", () => {
    const md = renderBackfillMarkdown("b1", "2026-09-07T00:00:00.000Z", items);
    expect(md).toContain("# 系列设定回填");
    expect(md).toContain("勾选 2 条");
    expect(md).toContain("## [worldview] 元气体系");
    expect(md).toContain("灵气分九品。");
  });

  it("diffLines：保留/移除/新增分类", () => {
    const diff = diffLines(["# 头", "旧行A", "共有行"], ["# 头", "共有行", "新行B"]);
    const kinds = diff.map((line) => line.kind);
    expect(kinds).toEqual(["kept", "removed", "kept", "added"]);
    expect(diff.find((line) => line.kind === "removed")?.text).toBe("旧行A");
    expect(diff.find((line) => line.kind === "added")?.text).toBe("新行B");
  });

  it("existing 为 null（首次写入）→ 非空行全部 added", () => {
    const diff = buildBackfillDiff(null, "b1", "2026-09-07T00:00:00.000Z", items);
    const nonEmpty = diff.filter((line) => line.text !== "");
    expect(nonEmpty.length).toBeGreaterThan(0);
    expect(nonEmpty.every((line) => line.kind === "added")).toBe(true);
  });

  it("覆盖语义显性化：取消勾选的既有条目出现在 removed", () => {
    // 上一次 apply 写了两条；这次只勾选 it-2 → it-1 的行应标记 removed。
    const existing = renderBackfillMarkdown("b1", "t0", items);
    const diff = buildBackfillDiff(existing, "b1", "t1", [items[1]]);
    const removed = diff.filter((line) => line.kind === "removed").map((line) => line.text);
    expect(removed.some((text) => text.includes("元气体系"))).toBe(true);
    // it-2 内容保留（新旧共有 → kept，而非 added）。
    const kept = diff.filter((line) => line.kind === "kept").map((line) => line.text);
    expect(kept.some((text) => text.includes("隐忍坚韧"))).toBe(true);
    expect(diff.some((line) => line.kind === "added" && line.text.includes("隐忍坚韧"))).toBe(false);
  });
});
