import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 808 号：quality-trend 正典派生——2.0 原生书无 review_metrics 账本（TS 无写链），
 * 账本缺席时不再返回死空壳（currentChapter 0），改从章节索引（observations 审改
 * 痕迹）与正典 state hooks.json 派生。审稿人数值分在 2.0 审改协议中不存在，
 * scoredChapters/averageScore 诚实为空。Rust 对偶 ops_routes get_quality_trend
 * （终态排除集同源：resolved 组 + superseded 组）。
 */
describe("quality-trend canonical derivation (808)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "quality-trend-canonical-"));
    await mkdir(join(root, ".inkos"), { recursive: true });
    await writeFile(join(root, "inkos.json"), JSON.stringify({ name: "probe", version: "0.1.0", language: "zh" }), "utf-8");
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const sourcePath = (bookId: string, ...parts: ReadonlyArray<string>) =>
    join(root, "works", bookId, "source", ...parts);

  it("derives currentChapter/points from reviewed index rows and urgency from canonical hooks when the ledger is absent", async () => {
    const app = createStudioServer({} as never, root);
    await mkdir(sourcePath("b1", "chapters"), { recursive: true });
    await mkdir(sourcePath("b1", "story", "state"), { recursive: true });
    const stamp = "2026-11-02T00:00:00.000Z";
    const base = { wordCount: 100, createdAt: stamp, updatedAt: stamp };
    await writeFile(sourcePath("b1", "chapters", "index.json"), JSON.stringify([
      // 导入种子：无 observations 键 = 未审（不产趋势点）。
      { number: 1, title: "一", provenance: "imported", ...base },
      // 已审且有 issue：passed=false，issueCount 只计 assessment=issue。
      {
        number: 2, title: "二", ...base,
        observations: [
          { code: "pacing", summary: "节奏拖沓", evidence: [], assessment: "issue" },
          { code: "note", summary: "风格观察", evidence: [], assessment: "observation" },
        ],
      },
      // 已审零发现：passed=true。
      { number: 3, title: "三", ...base, observations: [] },
    ]), "utf-8");
    await writeFile(sourcePath("b1", "story", "state", "hooks.json"), JSON.stringify({
      hooks: [
        { hookId: "H01", startChapter: 1, type: "suspense", status: "open", lastAdvancedChapter: 2, expectedPayoff: "第8章", notes: "" },
        { hookId: "H02", startChapter: 1, type: "plot", status: "resolved", lastAdvancedChapter: 3, expectedPayoff: "", notes: "" },
        { hookId: "H03", startChapter: 2, type: "emotion", status: "superseded", lastAdvancedChapter: 2, expectedPayoff: "第9章", notes: "撤回" },
      ],
    }), "utf-8");

    const res = await app.request("/api/v1/books/b1/quality-trend");
    expect(res.status).toBe(200);
    const body = await res.json() as {
      currentChapter: number;
      trend: { points: Array<{ chapter: number; passed: boolean; issueCount: number; overallScore?: number }>; scoredChapters: number; failingChapters: number[]; averageScore?: number };
      urgency: Array<{ hookId: string }>;
    };
    expect(body.currentChapter).toBe(4); // 最后已审章 3 + 1
    expect(body.trend.points).toEqual([
      { chapter: 2, passed: false, issueCount: 1 },
      { chapter: 3, passed: true, issueCount: 0 },
    ]);
    expect(body.trend.scoredChapters).toBe(0); // 2.0 审改协议无数值分
    expect(body.trend.averageScore).toBeUndefined();
    expect(body.trend.failingChapters).toEqual([2]);
    // resolved 与 superseded 均不进紧迫度。
    expect(body.urgency.map((item) => item.hookId)).toEqual(["H01"]);
  });

  it("falls back to an empty-but-live shape when neither ledger nor index exists", async () => {
    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/books/missing/quality-trend");
    expect(res.status).toBe(200);
    const body = await res.json() as { currentChapter: number; trend: { points: unknown[] }; urgency: unknown[] };
    expect(body.currentChapter).toBe(1); // 账本路径同语义：无已审章 → 1（此前死空壳为 0）
    expect(body.trend.points).toEqual([]);
    expect(body.urgency).toEqual([]);
  });
});
