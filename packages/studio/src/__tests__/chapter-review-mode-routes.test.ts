import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 709 号：章节审核模式四路由回收端到端（118 号三态：book 覆盖 project，
 * 均未设回 auto；inherit=解绑删键）。fork 原实现逐字，engine-rs
 * project_config_routes.rs / books_state_routes.rs review-mode 同水位对偶。
 * schema 收敛（writing 族可选键）后 loadBookConfig 不再拒收落盘键。
 */
describe("chapter-review-mode routes (709)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "review-mode-"));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const seedProject = async (extra: Record<string, unknown> = {}) => {
    await writeFile(join(root, "inkos.json"), JSON.stringify({ name: "t", ...extra }), "utf-8");
  };
  const seedBook = async (bookId: string, extra: Record<string, unknown> = {}) => {
    const dir = join(root, "books", bookId);
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, "book.json"), JSON.stringify({ id: bookId, title: "镜花缘", ...extra }), "utf-8");
  };

  it("project GET/PUT: normalize 仅 manual 精确命中 + writing.reviewMode 落盘", async () => {
    await seedProject();
    const app = createStudioServer({} as never, root);

    // GET：无 writing 段 → auto
    const get1 = await app.request("/api/v1/project/chapter-review-mode");
    expect(get1.status).toBe(200);
    expect(await get1.json()).toEqual({ mode: "auto" });

    // PUT：非 manual 一律归 auto
    const putAuto = await app.request("/api/v1/project/chapter-review-mode", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ mode: "yolo" }),
    });
    expect(putAuto.status).toBe(200);
    expect(await putAuto.json()).toEqual({ ok: true, mode: "auto" });

    // PUT：manual 落盘（保留同段其他键）
    await seedProject({ writing: { reviewRetries: 2 } });
    const putManual = await app.request("/api/v1/project/chapter-review-mode", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ mode: "manual" }),
    });
    expect(await putManual.json()).toEqual({ ok: true, mode: "manual" });
    const raw = JSON.parse(await readFile_(join(root, "inkos.json"))) as { writing?: Record<string, unknown> };
    expect(raw.writing?.reviewMode).toBe("manual");
    expect(raw.writing?.reviewRetries).toBe(2);

    // GET：manual 回读
    const get2 = await app.request("/api/v1/project/chapter-review-mode");
    expect(await get2.json()).toEqual({ mode: "manual" });
  });

  it("book GET/PUT: 三态覆盖 + inherit 解绑删键 + 404 + writing 空则整体移除", async () => {
    await seedProject({ writing: { reviewMode: "manual" } });
    await seedBook("b1");
    const app = createStudioServer({} as never, root);

    // GET：book 未设 → project 兜底
    const get1 = await app.request("/api/v1/books/b1/chapter-review-mode");
    expect(get1.status).toBe(200);
    expect(await get1.json()).toEqual({ mode: "manual", bookMode: null, projectMode: "manual" });

    // PUT：book 覆盖为 auto
    const putBook = await app.request("/api/v1/books/b1/chapter-review-mode", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ mode: "auto" }),
    });
    expect(await putBook.json()).toEqual({ ok: true, mode: "auto", bookMode: "auto", projectMode: "manual" });
    const raw = JSON.parse(await readFile_(join(root, "books", "b1", "book.json"))) as { writing?: Record<string, unknown> };
    expect(raw.writing?.reviewMode).toBe("auto");

    // GET：bookMode 在位
    const get2 = await app.request("/api/v1/books/b1/chapter-review-mode");
    expect(await get2.json()).toEqual({ mode: "auto", bookMode: "auto", projectMode: "manual" });

    // PUT：inherit → 删 writing.reviewMode；writing 空则整体移除
    const putInherit = await app.request("/api/v1/books/b1/chapter-review-mode", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ mode: "inherit" }),
    });
    expect(putInherit.status).toBe(200);
    expect(await putInherit.json()).toEqual({ ok: true, mode: "manual", bookMode: null, projectMode: "manual" });
    const rawAfter = JSON.parse(await readFile_(join(root, "books", "b1", "book.json"))) as { writing?: Record<string, unknown> };
    expect(rawAfter.writing).toBeUndefined();

    // inherit 保留同段其他键（autoTimelineBeats 不被连带删除）
    await seedBook("b1", { writing: { reviewMode: "auto", autoTimelineBeats: true } });
    await app.request("/api/v1/books/b1/chapter-review-mode", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ mode: "inherit" }),
    });
    const rawKeep = JSON.parse(await readFile_(join(root, "books", "b1", "book.json"))) as { writing?: Record<string, unknown> };
    expect(rawKeep.writing).toEqual({ autoTimelineBeats: true });

    // 缺书 404 + 非法 id 400（编码路径穿越，2.0 INVALID_BOOK_ID 中间件同口径）
    expect((await app.request("/api/v1/books/ghost/chapter-review-mode")).status).toBe(404);
    expect((await app.request("/api/v1/books/a%2Fb%2F..%2Fevil/chapter-review-mode")).status).toBe(400);
  });

  it("schema 收敛回归锁：四写面落盘后的 book.json 必须 BookConfigSchema 可析（潜伏自毁键清偿）", async () => {
    // book 级 PUT 前置读 inkos.json（projectMode 兜底面；缺失=双端一致 404）
    await seedProject();
    // 完整合法基线书（schema 必填面齐备，隔离「写键」这一单一变量）
    await seedBook("b1", {
      platform: "qidian", genre: "xuanhuan", status: "active",
      targetChapters: 12, chapterWordCount: 3000, language: "zh",
      createdAt: "2026-10-16T00:00:00.000Z", updatedAt: "2026-10-16T00:00:00.000Z",
    });
    const app = createStudioServer({} as never, root);
    await app.request("/api/v1/books/b1/chapter-review-mode", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ mode: "manual" }),
    });
    // 701 best-of-n 写 governance + 707 auto-beats 写 autoTimelineBeats +
    // series-id 写 seriesId + 本轮 reviewMode——四键共存于 book.json，
    // strict schema 解析必须存活（701/707 时代会 ZodError unrecognized key）。
    await app.request("/api/v1/books/b1/best-of-n", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled: true, candidates: 3, minScore: 80 }),
    });
    await app.request("/api/v1/books/b1/timeline-auto-beats", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled: true }),
    });
    await app.request("/api/v1/books/b1/series-id", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ seriesId: "jing_hua_yuan" }),
    });

    const stored = JSON.parse(await readFile_(join(root, "books", "b1", "book.json")));
    const { BookConfigSchema, StateManager } = await import("@actalk/inkos-core");
    const parsed = BookConfigSchema.parse(stored);
    expect(parsed.writing?.reviewMode).toBe("manual");
    expect(parsed.writing?.autoTimelineBeats).toBe(true);
    expect(parsed.governance?.bestOfN?.candidates).toBe(3);
    expect(parsed.seriesId).toBe("jing_hua_yuan");

    // StateManager 强类型加载同锁（canonical works 布局同键共存形态：
    // 迁移副本与 legacy 树同构，写面键集一致即双布局皆可加载）。
    const { mkdir: mkdirFs, copyFile } = await import("node:fs/promises");
    const worksDir = join(root, "works", "b1", "source");
    await mkdirFs(worksDir, { recursive: true });
    await copyFile(join(root, "books", "b1", "book.json"), join(worksDir, "book.json"));
    const loaded = await new StateManager(root).loadBookConfig("b1");
    expect(loaded.id).toBe("b1");
    expect(loaded.writing?.autoTimelineBeats).toBe(true);
  });
});

async function readFile_(path: string): Promise<string> {
  const { readFile } = await import("node:fs/promises");
  return readFile(path, "utf-8");
}
