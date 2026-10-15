import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { LocalSearchIndex } from "@actalk/inkos-core";

/**
 * 718 号：面板族尾批三路由回收端到端——hybrid-search（RecallTestCard 召回
 * 测试；FTS 面走当前演进版 LocalSearchIndex，语义面 2.0 无 MemoryDB 写链
 * 降级 fts5-fallback）/ style-binding GET/PUT（写法绑定面板；profile 存在
 * 性校验）/ fanfic/refresh（同人正典刷新；真载荷 LLM 链留走查）。fork 逐字
 * + 路径正典化。Rust 对偶 ops_routes.rs（get/save_style_binding）/
 * books_routes.rs（post_hybrid_search）/ fanfic_routes.rs（fanfic_refresh）。
 */
describe("panel family recycle routes (718)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "panel-family-recycle-"));
    const dir = join(root, ".inkos");
    await mkdir(dir, { recursive: true });
    await writeFile(
      join(root, "inkos.json"),
      JSON.stringify({ name: "probe", version: "0.1.0", language: "zh" }),
      "utf-8",
    );
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const seedBook = async (bookId: string) => {
    const sourceDir = join(root, "works", bookId, "source");
    await mkdir(join(sourceDir, "story"), { recursive: true });
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
        createdAt: "2026-10-16T00:00:00.000Z",
        updatedAt: "2026-10-16T00:00:00.000Z",
      }),
      "utf-8",
    );
    return sourceDir;
  };

  it("hybrid-search: 空 query 400 + 无库缺省分支 + memory.db 在位 FTS 命中", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1");

    // 空 query → 400（fork/Rust 同护栏）。
    const empty = await app.request("/api/v1/books/b1/hybrid-search", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "   " }),
    });
    expect(empty.status).toBe(400);
    expect(((await empty.json()) as { error: string }).error).toContain("query");

    // memory.db 缺失 → 缺省零结果分支（fts5-fallback）。
    const noDb = await app.request("/api/v1/books/b1/hybrid-search", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "祖符" }),
    });
    expect(noDb.status).toBe(200);
    expect(await noDb.json()).toEqual({ fts: [], semantic: [], fused: [], mode: "fts5-fallback" });

    // memory.db 在位（LocalSearchIndex 播种 story-memory 语料）→ FTS 命中。
    const dbPath = join(root, "works", "b1", "source", "story", "memory.db");
    const index = new LocalSearchIndex(dbPath);
    index.replaceScope("story-memory", [
      {
        id: "chapter_summaries-1",
        scope: "story-memory",
        kind: "chapter_summary",
        source: "story/chapter_summaries.md",
        title: "第1章 镜中醒来",
        body: "苏檀在坊市对峙中夺回祖符玉佩，玉符异象初显，正式踏上修炼路。",
      },
      {
        id: "current_state-1",
        scope: "story-memory",
        kind: "state",
        source: "story/current_state.md",
        title: "当前状态",
        body: "林动修为浅薄，正为七日内应对雷家寻仇做准备。",
      },
    ]);
    index.close();

    const hit = await app.request("/api/v1/books/b1/hybrid-search", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "祖符", k: 5 }),
    });
    expect(hit.status).toBe(200);
    const hitBody = (await hit.json()) as {
      results: ReadonlyArray<{ id: string; score: number }>;
      mode: string;
    };
    expect(hitBody.mode).toBe("fts5-fallback");
    expect(hitBody.results.length).toBeGreaterThan(0);
    expect(hitBody.results[0]?.id).toBe("chapter_summaries-1");

    // 坏 id 400。
    const evil = await app.request("/api/v1/books/a%2Fb%2F..%2Fevil/hybrid-search", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "祖符" }),
    });
    expect(evil.status).toBe(400);
  });

  it("style-binding GET/PUT: 缺省 null + profileName/profile 校验 + 绑定落盘往返", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1");

    // GET：无绑定文件 → binding null。
    const empty = await app.request("/api/v1/books/b1/style-binding");
    expect(empty.status).toBe(200);
    expect(await empty.json()).toEqual({ binding: null });

    // PUT：profileName 缺失 400；profile 不存在 404。
    const noName = await app.request("/api/v1/books/b1/style-binding", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(noName.status).toBe(400);
    const noProfile = await app.request("/api/v1/books/b1/style-binding", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ profileName: "probe-missing" }),
    });
    expect(noProfile.status).toBe(404);
    expect(((await noProfile.json()) as { error: string }).error).toContain("profile not found");

    // 播 style profile → 合法绑定落盘往返。
    const profilesDir = join(root, ".inkos", "style-profiles");
    await mkdir(profilesDir, { recursive: true });
    await writeFile(
      join(profilesDir, "wuxia-prose.json"),
      JSON.stringify({ name: "wuxia-prose", content: "短句白描，动作见骨。" }),
      "utf-8",
    );
    const put = await app.request("/api/v1/books/b1/style-binding", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ profileName: "wuxia-prose", maxGuidanceChars: 1200 }),
    });
    expect(put.status).toBe(200);
    const putBody = (await put.json()) as { ok: boolean; binding: Record<string, unknown> };
    expect(putBody.ok).toBe(true);
    expect(putBody.binding.profileName).toBe("wuxia-prose");
    expect(putBody.binding.maxGuidanceChars).toBe(1200);
    expect(putBody.binding.enabledIds).toBeUndefined();

    const round = await app.request("/api/v1/books/b1/style-binding");
    const roundBody = (await round.json()) as { binding: Record<string, unknown> | null };
    expect(roundBody.binding?.profileName).toBe("wuxia-prose");
  });

  it("fanfic/refresh: 空载荷 400 + 坏 id 400（真载荷 LLM 链留走查）", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1");

    const empty = await app.request("/api/v1/books/b1/fanfic/refresh", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sourceText: "   " }),
    });
    expect(empty.status).toBe(400);
    expect(((await empty.json()) as { error: string }).error).toContain("sourceText");

    const evil = await app.request("/api/v1/books/a%2Fb%2F..%2Fevil/fanfic/refresh", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sourceText: "正文" }),
    });
    expect(evil.status).toBe(400);
  });
});
