import { createServer, type Server } from "node:http";
import { once } from "node:events";
import type { AddressInfo } from "node:net";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { StateManager } from "@actalk/inkos-core";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 1667 号：fork 读/写面「幽灵树」路径族——2.0 原生书端到端。
 *
 * 701-707 号批次回收的 1.x 处理器沿用 `join(root,"books",id,…)`，而 2.0 规范
 * 书目录是 `state.bookDir(id)` = works/<id>/source。原生书（Studio 2.0 新建，
 * 没有 books/<id> 树）上：roster/confirm 写 legacy 而 roster-candidates 读 works
 * （确认即丢失）；timeline API 与管线 timeline-settle 各读各的文件；best-of-n /
 * auto-beats / series-id 对存在的书报 not found 或写进残缺幽灵文件。差分器抓不
 * 到——其 TS 夹具是 legacy 书经迁移注册，读写恰好在同一棵树。
 *
 * 规则：读 = works 优先、legacy 兜底（存量数据不丢，见 705/707/709 号旧测的
 * legacy 用例）；写 = 恒 works。本文件锁原生书侧（修复前恒红）。
 */
describe("book data path family on native books (1667)", () => {
  let root: string;
  let app: ReturnType<typeof createStudioServer>;
  const JSON_HEADERS = { "Content-Type": "application/json" };
  const bookDir = (id: string) => join(root, "works", id, "source");
  const configOf = (id: string, title = "镜花水月") => ({
    id, title, platform: "qidian", genre: "xuanhuan", status: "active",
    targetChapters: 12, chapterWordCount: 3000, language: "zh",
    createdAt: "2026-10-16T00:00:00.000Z", updatedAt: "2026-10-16T00:00:00.000Z",
  });

  async function seedNativeBook(id: string, summariesCharacters?: string): Promise<void> {
    await mkdir(join(bookDir(id), "story", "state"), { recursive: true });
    await writeFile(join(bookDir(id), "book.json"), JSON.stringify(configOf(id)), "utf-8");
    if (summariesCharacters !== undefined) {
      await writeFile(
        join(bookDir(id), "story", "state", "chapter_summaries.json"),
        JSON.stringify({ rows: [{ chapter: 1, characters: summariesCharacters }] }),
        "utf-8",
      );
    }
  }

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "book-data-path-"));
    app = createStudioServer({} as never, root);
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("roster confirm on a native book becomes visible to the roster reader and creates no books/ tree", async () => {
    await seedNativeBook("b1", "林昭、苏檀");

    const before = await app.request("/api/v1/books/b1/roster-candidates");
    expect(before.status).toBe(200);
    const beforeBody = (await before.json()) as { rosterExists: boolean; cards: Array<{ candidate: string; action: string }> };
    expect(beforeBody.rosterExists).toBe(false);
    expect(beforeBody.cards).toContainEqual({ candidate: "林昭", action: "new-entity", confidence: expect.any(String) });

    const confirmed = await app.request("/api/v1/books/b1/roster/confirm", {
      method: "POST",
      headers: JSON_HEADERS,
      body: JSON.stringify({ candidate: "林昭", action: "new-entity" }),
    });
    expect(confirmed.status).toBe(200);
    expect(await confirmed.json()).toMatchObject({ ok: true, applied: { kind: "new-entity" } });

    // 读面必须看到确认结果——此前写 books/<id>、读 works/<id>/source，候选恒为「新实体」。
    const after = await app.request("/api/v1/books/b1/roster-candidates");
    const afterBody = (await after.json()) as { rosterExists: boolean; cards: Array<{ candidate: string }> };
    expect(afterBody.rosterExists).toBe(true);
    expect(afterBody.cards.map((card) => card.candidate)).toEqual(["苏檀"]);

    const roster = await readFile(join(bookDir("b1"), "story", "entity_roster.md"), "utf-8");
    expect(roster).toContain("name: 林昭");
    // 不再留下一棵 books/<id> 杂树。
    await expect(readdir(join(root, "books", "b1"))).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("best-of-n / anti-ai rules / series-id land in the canonical book dir and stay schema-loadable", async () => {
    await seedNativeBook("b1");

    const bestOfN = await app.request("/api/v1/books/b1/best-of-n", {
      method: "PUT",
      headers: JSON_HEADERS,
      body: JSON.stringify({ enabled: true, candidates: 3, minScore: 80 }),
    });
    expect(await bestOfN.json()).toEqual({ enabled: true, candidates: 3, minScore: 80 });
    expect(await (await app.request("/api/v1/books/b1/best-of-n")).json())
      .toEqual({ enabled: true, candidates: 3, minScore: 80 });

    const rules = await app.request("/api/v1/books/b1/anti-ai-rules", {
      method: "PUT",
      headers: JSON_HEADERS,
      body: JSON.stringify({ rules: [] }),
    });
    expect(rules.status).toBe(200);
    expect(await (await app.request("/api/v1/books/b1/anti-ai-rules")).json()).toEqual({ rules: [], seeded: false });

    const series = await app.request("/api/v1/books/b1/series-id", {
      method: "PUT",
      headers: JSON_HEADERS,
      body: JSON.stringify({ seriesId: "jing_hua_yuan" }),
    });
    expect(await series.json()).toEqual({ ok: true, seriesId: "jing_hua_yuan" });
    expect(await (await app.request("/api/v1/books/b1/series-id")).json()).toEqual({ seriesId: "jing_hua_yuan" });

    // 配置写在真 book.json 上：其余字段不丢，strict schema 可析，StateManager 可加载。
    const stored = JSON.parse(await readFile(join(bookDir("b1"), "book.json"), "utf-8")) as Record<string, unknown>;
    expect(stored.title).toBe("镜花水月");
    expect(stored.seriesId).toBe("jing_hua_yuan");
    const loaded = await new StateManager(root).loadBookConfig("b1");
    expect(loaded.governance?.bestOfN?.candidates).toBe(3);
    expect(loaded.seriesId).toBe("jing_hua_yuan");
    expect(JSON.parse(await readFile(join(bookDir("b1"), "story", "anti_ai_rules.json"), "utf-8"))).toEqual({ version: 1, rules: [] });
    await expect(readdir(join(root, "books", "b1"))).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("best-of-n on a ghost book answers 404 and writes nothing", async () => {
    const put = await app.request("/api/v1/books/ghost/best-of-n", {
      method: "PUT",
      headers: JSON_HEADERS,
      body: JSON.stringify({ enabled: true }),
    });
    expect(put.status).toBe(404);
    // 此前会写出只含 governance 的残缺 book.json（现在落规范目录=一棵无清单的孤儿书树）。
    await expect(readdir(join(root, "works", "ghost"))).rejects.toMatchObject({ code: "ENOENT" });
    await expect(readdir(join(root, "books", "ghost"))).rejects.toMatchObject({ code: "ENOENT" });
    // 缺书 GET 仍为「未启用」而非 404（可选读面不制造噪音）。
    expect(await (await app.request("/api/v1/books/ghost/best-of-n")).json()).toEqual({ enabled: false });
  });

  it("a corrupt book.json is never overwritten by the best-of-n write", async () => {
    await mkdir(bookDir("b1"), { recursive: true });
    await writeFile(join(bookDir("b1"), "book.json"), "{broken", "utf-8");
    const put = await app.request("/api/v1/books/b1/best-of-n", {
      method: "PUT",
      headers: JSON_HEADERS,
      body: JSON.stringify({ enabled: true }),
    });
    expect(put.status).toBe(500);
    // 区分「拒写」与「写到一半失败」：旧码是往不存在的 books/<id> 目录写而 ENOENT。
    expect(await put.json()).toEqual({ error: "Book config is not valid JSON" });
    expect(await readFile(join(bookDir("b1"), "book.json"), "utf-8")).toBe("{broken");
  });

  it("chapter versions read and restore on a native book; invalid or missing ids leave the chapter untouched", async () => {
    const versionId = "1760000000000_manual_11111111-1111-1111-1111-111111111111";
    const missingId = "1760000000001_manual_22222222-2222-2222-2222-222222222222";
    await mkdir(join(bookDir("b1"), "chapters", ".versions", "0001"), { recursive: true });
    await writeFile(join(bookDir("b1"), "chapters", "0001_风起.md"), "# 风起\n\n现行正文。\n", "utf-8");
    await writeFile(join(bookDir("b1"), "chapters", ".versions", "0001", `${versionId}.md`), "# 风起\n\n旧版正文。\n", "utf-8");

    const read = await app.request(`/api/v1/books/b1/chapters/1/versions/${versionId}`);
    expect(read.status).toBe(200);
    expect(await read.json()).toEqual({ chapterNumber: 1, versionId, content: "# 风起\n\n旧版正文。\n" });

    const chapterPath = join(bookDir("b1"), "chapters", "0001_风起.md");
    // 1668 号：版本**查找**失败（非法 id / 缺文件 / 非法章节号 / 书不存在）= 资源
    // 不存在 → 404，与 GET …/versions/:versionId 及 Rust restore_chapter_version
    // 同码（1664 号立案 (b)：此前与替换事务错误一并落 500 裸串）。
    const lookupFailures: Array<[string, string]> = [
      ["/api/v1/books/b1/chapters/1/versions/v1/restore", "Invalid chapter version id"],
      [`/api/v1/books/b1/chapters/1/versions/${missingId}/restore`, "ENOENT"],
      ["/api/v1/books/b1/chapters/abc/versions/v1/restore", "Invalid chapter number"],
      [`/api/v1/books/ghost/chapters/1/versions/${versionId}/restore`, "ENOENT"],
    ];
    for (const [path, fragment] of lookupFailures) {
      const restore = await app.request(path, { method: "POST" });
      expect(restore.status, path).toBe(404);
      expect(String(((await restore.json()) as { error: string }).error), path).toContain(fragment);
      // 查找失败不得改写正文。
      expect(await readFile(chapterPath, "utf-8"), path).toBe("# 风起\n\n现行正文。\n");
    }

    const restored = await app.request(`/api/v1/books/b1/chapters/1/versions/${versionId}/restore`, { method: "POST" });
    expect(restored.status).toBe(200);
    expect(await restored.json()).toMatchObject({ ok: true, chapterNumber: 1, versionId });
    expect(await readFile(chapterPath, "utf-8")).toBe("# 风起\n\n旧版正文。\n");
  });
});

describe("series backfill reads the source book canon from the canonical dir (1667)", () => {
  let root: string;
  let app: ReturnType<typeof createStudioServer>;
  let upstream: Server;
  let upstreamBase = "";
  const JSON_HEADERS = { "Content-Type": "application/json" };
  const bookDir = (id: string) => join(root, "works", id, "source");
  const configOf = (id: string) => ({
    id, title: id, platform: "qidian", genre: "xuanhuan", status: "active",
    targetChapters: 12, chapterWordCount: 3000, language: "zh",
    createdAt: "2026-10-16T00:00:00.000Z", updatedAt: "2026-10-16T00:00:00.000Z",
  });

  beforeAll(async () => {
    upstream = createServer((req, res) => {
      let body = "";
      req.on("data", (chunk) => { body += chunk; });
      req.on("end", () => {
        let stream = false;
        try { stream = JSON.parse(body).stream === true; } catch { /* 非 JSON 体按非流式 */ }
        const content = JSON.stringify({
          items: [{ id: "it-1", category: "worldview", title: "残卷设定", content: "上古残卷共三枚" }],
        });
        if (stream) {
          res.writeHead(200, { "Content-Type": "text/event-stream" });
          res.end(`data: ${JSON.stringify({ id: "backfill", choices: [{ index: 0, delta: { role: "assistant", content }, finish_reason: "stop" }] })}\n\ndata: [DONE]\n\n`);
          return;
        }
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ choices: [{ index: 0, message: { role: "assistant", content }, finish_reason: "stop" }] }));
      });
    });
    upstream.listen(0, "127.0.0.1");
    await once(upstream, "listening");
    upstreamBase = `http://127.0.0.1:${(upstream.address() as AddressInfo).port}/v1`;
  });
  afterAll(async () => {
    await new Promise<void>((resolveClose) => upstream.close(() => resolveClose()));
  });

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "backfill-canon-"));
    await mkdir(join(root, ".inkos"), { recursive: true });
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "backfill-canon", version: "0.1.0", language: "zh",
      llm: { service: "custom:Mock", defaultModel: "lm-mock-model", services: [{ service: "custom", name: "Mock", baseUrl: upstreamBase }] },
    }), "utf-8");
    // 1015/855 号惯用形：客户端凭据走 env 注入 + 非凭据回退。
    await writeFile(join(root, ".inkos", "secrets.json"), JSON.stringify({
      services: { "custom:Mock": { apiKey: process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder" } },
    }), "utf-8");
    app = createStudioServer({} as never, root);
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("extracts from a native source book and applies into the canonical target dir", async () => {
    for (const id of ["target", "source"]) {
      await mkdir(join(bookDir(id), "story", "outline"), { recursive: true });
      await writeFile(join(bookDir(id), "book.json"), JSON.stringify(configOf(id)), "utf-8");
    }
    await writeFile(join(bookDir("source"), "story", "outline", "story_frame.md"), "# 故事框架\n\n三幕结构。\n", "utf-8");

    const extracted = await app.request("/api/v1/books/target/series-backfill/extract", {
      method: "POST",
      headers: JSON_HEADERS,
      body: JSON.stringify({ sourceBookId: "source" }),
    });
    // 此前恒 400「Source book has no canon files」——正典只从 books/<sourceBookId> 读。
    expect(extracted.status).toBe(200);
    const draft = (await extracted.json()) as { draft: { sourceBookId: string; items: Array<{ id: string }> } };
    expect(draft.draft.sourceBookId).toBe("source");
    expect(draft.draft.items.map((item) => item.id)).toEqual(["it-1"]);
    await readFile(join(bookDir("target"), "story", "series_backfill_draft.json"), "utf-8");

    const applied = await app.request("/api/v1/books/target/series-backfill/apply", {
      method: "POST",
      headers: JSON_HEADERS,
      body: JSON.stringify({}),
    });
    expect(applied.status).toBe(200);
    expect(await applied.json()).toMatchObject({ ok: true, path: "story/series_backfill.md", applied: 1, total: 1 });
    expect(await readFile(join(bookDir("target"), "story", "series_backfill.md"), "utf-8")).toContain("## [worldview] 残卷设定");
    await expect(readdir(join(root, "books"))).rejects.toMatchObject({ code: "ENOENT" });
  });
});
