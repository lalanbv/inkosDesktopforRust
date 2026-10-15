import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 715 号：卷级归并面回收端到端——POST /books/:id/consolidate（BookDetail
 * 「归并记忆」活消费）。ConsolidatorAgent 为 fork 逐字移植（含 Phase 7
 * hotfix 2 归档前 promote 重跑）；无完成卷边界时提前返回零归档（不打 LLM
 * ——单测无 LLM 环境的确定面）；rerunAdvancedCountPromotion 的 advancedCount
 * ≥2 翻转独立于摘要归并（新书写法无卷也生效）。Rust 对偶
 * engine-rs/src/agents/consolidator.rs（ConsolidationResult 四字段同形）。
 */
describe("consolidate recycle route (715)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "consolidate-recycle-"));
    const dir = join(root, ".inkos");
    await mkdir(dir, { recursive: true });
    await writeFile(
      join(root, "inkos.json"),
      JSON.stringify({
        name: "probe",
        version: "0.1.0",
        language: "zh",
        llm: {
          service: "custom:Mock",
          defaultModel: "lm-mock-model",
          services: [{ service: "custom", name: "Mock", baseUrl: "http://127.0.0.1:1246/v1" }],
        },
      }),
      "utf-8",
    );
    await writeFile(
      join(dir, "secrets.json"),
      JSON.stringify({ services: { "custom:Mock": { apiKey: "sk-mock" } } }),
      "utf-8",
    );
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const storyFile = async (bookId: string, name: string, content: string) => {
    const dir = join(root, "works", bookId, "source", "story");
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, name), content, "utf-8");
  };

  it("consolidate: 无卷边界幂等零归档（不打 LLM）+ 坏 id 400", async () => {
    const app = createStudioServer({} as never, root);
    // 空 story（无 chapter_summaries/volume_map）→ 提前返回分支。
    const res = await app.request("/api/v1/books/b1/consolidate", { method: "POST" });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      volumeSummaries: "",
      archivedVolumes: 0,
      retainedChapters: 0,
      promotedHookCount: 0,
    });

    const evil = await app.request("/api/v1/books/a%2Fb%2F..%2Fevil/consolidate", { method: "POST" });
    expect(evil.status).toBe(400);
  });

  it("consolidate: promote 重跑独立生效（advancedCount≥2 翻转，无卷也写回台账）", async () => {
    const app = createStudioServer({} as never, root);
    // 台账两条：H01 advancedCount 已达阈值未升级；H02 未推进。
    const hooksTable = [
      "| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 | 分类 |",
      "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
      "| H01 | 1 | main | progressing | 3 | 第10章 |  |  |  |  |  | 否 | 种子伏笔 |  |",
      "| H02 | 2 | sub | open | 0 | 第12章 |  |  |  |  |  | 否 | 未推进 |  |",
    ].join("\n");
    await storyFile("b1", "pending_hooks.md", hooksTable);
    // 摘要表含 H01 推进行（deriveAdvancedCountsFromSummaries 从 hookActivity 列计数）。
    const summaries = [
      "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 | 冲突强度 | 揭示强度 |",
      "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
      "| 1 | 初入 | 林动 | 祖符觉醒 | 无 | H01 推进 | 平静 | 开局 |  |  |",
      "| 2 | 坊市 | 林动 | 夺符 | 微澜 | H01 再度推进 | 紧张 | 冲突 |  |  |",
    ].join("\n");
    await storyFile("b1", "chapter_summaries.md", summaries);

    const res = await app.request("/api/v1/books/b1/consolidate", { method: "POST" });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { archivedVolumes: number; promotedHookCount: number };
    // 无卷边界 → 零归档；promote 重跑翻转 H01（advancedCount≥2）。
    expect(body.archivedVolumes).toBe(0);
    expect(body.promotedHookCount).toBe(1);

    // 台账写回：H01 升级列翻转为「是」（renderHookSnapshot 往返）。
    const { readFile } = await import("node:fs/promises");
    const ledger = await readFile(join(root, "works", "b1", "source", "story", "pending_hooks.md"), "utf-8");
    expect(ledger).toContain("H01");
    expect(ledger).toMatch(/\|\s*是\s*\|/);
    expect(ledger).toMatch(/\|\s*否\s*\|/);
  });
});
