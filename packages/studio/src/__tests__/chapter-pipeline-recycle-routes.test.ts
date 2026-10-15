import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { CHAPTER_STATUSES } from "@actalk/inkos-core";

/**
 * 714 号：章节管线五面回收端到端——plan/compose（LLM 管线面，Runner 三方法）
 * + approve/reject 审改流域（纯状态流转）+ repair-state（fork/Rust 同款护栏：
 * 仅最新 state-degraded 章可修）。Rust 对偶 books_routes.rs（run_plan/
 * run_compose/run_repair_state）与 books_state_routes.rs（approve/reject）。
 * reject 快乐路径（rollback 快照链）留 walkthrough 活体——单测覆盖护栏与
 * 状态写面 roundtrip（ChapterMeta.status 可选键 709 号自毁键教训）。
 */
describe("chapter pipeline recycle routes (714)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "chapter-pipeline-recycle-"));
    await seedProject();
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const seedProject = async () => {
    await mkdir(join(root, ".inkos"), { recursive: true });
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
      join(root, ".inkos", "secrets.json"),
      JSON.stringify({ services: { "custom:Mock": { apiKey: "sk-mock" } } }),
      "utf-8",
    );
  };

  const meta = (number: number, extra: Record<string, unknown> = {}) => ({
    number,
    title: `第${number}章`,
    wordCount: 100,
    createdAt: "2026-09-14T00:00:00.000Z",
    updatedAt: "2026-09-14T00:00:00.000Z",
    observations: [],
    provenance: "imported" as const,
    ...extra,
  });

  // 709 号正典布局锁：2.0 StateManager bookDir = works/{id}/source——索引与
  // book.json 必须播 canonical 树（books/ legacy 播种 StateManager 读不到）。
  const seedBook = async (bookId: string, metas: ReadonlyArray<Record<string, unknown>>) => {
    const sourceDir = join(root, "works", bookId, "source");
    const dir = join(sourceDir, "chapters");
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, "index.json"), JSON.stringify(metas, null, 2), "utf-8");
    for (const m of metas) {
      await writeFile(join(dir, `${String(m.number).padStart(4, "0")}_${m.title}.md`), `# ${m.title}\n正文。`, "utf-8");
    }
    await writeFile(
      join(sourceDir, "book.json"),
      JSON.stringify({
        id: bookId,
        title: "镜花缘",
        platform: "qidian",
        genre: "xuanhuan",
        status: "active",
        targetChapters: 12,
        chapterWordCount: 3000,
        language: "zh",
        createdAt: "2026-09-14T00:00:00.000Z",
        updatedAt: "2026-09-14T00:00:00.000Z",
      }),
      "utf-8",
    );
  };

  it("approve: 状态写面 200 + status 键 roundtrip 可读（strict schema 自毁键回归锁）", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1", [meta(1)]);

    const put = await app.request("/api/v1/books/b1/chapters/1/approve", { method: "POST" });
    expect(put.status).toBe(200);
    expect(await put.json()).toEqual({ ok: true, chapterNumber: 1, status: "approved" });

    // roundtrip：写入后索引仍可被加载（709 号教训——写入成功 ≠ 产物可再消费）。
    const detail = await app.request("/api/v1/books/b1");
    expect(detail.status).toBe(200);
    const body = (await detail.json()) as { chapters: ReadonlyArray<{ number: number; status?: string }> };
    expect(body.chapters[0]?.status).toBe("approved");

    // 未命中章节：fork 语义静默 200（map 无命中原样保存；Rust approve 同款）。
    const missing = await app.request("/api/v1/books/b1/chapters/9/approve", { method: "POST" });
    expect(missing.status).toBe(200);
  });

  it("reject: 未命中 404 + 无快照护栏 500（rollbackTo 0）+ 章态枚举真源对齐", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1", [meta(1)]);

    // 未命中 → 404 typed 错误体（fork/Rust 同款文案）。
    const missing = await app.request("/api/v1/books/b1/chapters/2/reject", { method: "POST" });
    expect(missing.status).toBe(404);
    expect(((await missing.json()) as { error: string }).error).toContain("Chapter 2 not found");

    // 命中但 rollbackTo 0 快照缺失 → 500（fork/Rust 同护栏；快乐路径留走查）。
    const noSnapshot = await app.request("/api/v1/books/b1/chapters/1/reject", { method: "POST" });
    expect(noSnapshot.status).toBe(500);

    // 714 号：章态枚举以 Rust ChapterStatus 14 态为真源（approve/reject 写入值在列）。
    expect(CHAPTER_STATUSES).toContain("approved");
    expect(CHAPTER_STATUSES).toContain("rejected");
    expect(CHAPTER_STATUSES).toContain("state-degraded");
    expect(CHAPTER_STATUSES).toHaveLength(14);
  });

  it("plan/compose: LLM 管线面无 LLM 500（错误体非 TypeError）+ 坏 id 400", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1", [meta(1)]);

    for (const face of ["plan", "compose"] as const) {
      // 管线面需要 LLM（planner memo）——无 LLM 环境失败形态须是管线错误
      // （非 TypeError 崩溃；713 号判别锁口径）。
      const res = await app.request(`/api/v1/books/b1/${face}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({}),
      });
      expect(res.status).toBe(500);
      const body = (await res.json()) as { error: string };
      expect(body.error).toBeTruthy();
      expect(body.error).not.toContain("TypeError");
    }

    // 路径穿越守卫（709 批次同标准；编码形态绕过 fetch 归一化）。
    const evil = await app.request("/api/v1/books/a%2Fb%2F..%2Fevil/plan", { method: "POST" });
    expect(evil.status).toBe(400);
  });

  it("repair-state: 护栏三连——空书 500 / 非 state-degraded 500 / 非最新章 500", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1", [meta(1)]);

    // 非最新章（2 不在索引 → not found 文案）与最新章（非 state-degraded）。
    const notFound = await app.request("/api/v1/books/b1/repair-state/2", { method: "POST" });
    expect(notFound.status).toBe(500);
    expect(((await notFound.json()) as { error: string }).error).toContain("Chapter 2 not found");

    const notDegraded = await app.request("/api/v1/books/b1/repair-state/1", { method: "POST" });
    expect(notDegraded.status).toBe(500);
    expect(((await notDegraded.json()) as { error: string }).error).toContain("is not state-degraded");

    // 空 index 书：护栏首查（has no persisted chapters）。
    await seedBook("b2", []);
    const empty = await app.request("/api/v1/books/b2/repair-state/1", { method: "POST" });
    expect(empty.status).toBe(500);
    expect(((await empty.json()) as { error: string }).error).toContain("has no persisted chapters to repair");
  });
});
