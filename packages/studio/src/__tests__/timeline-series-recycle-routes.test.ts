import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 707 号：timeline 域 + series 域回收第五批端到端——181 号 C4-b 书籍时间线
 * 读写 + 189 号 auto-beats 开关 + R20/390 系列正典/书级绑定 + 184 号 C3-b
 * 系列回填三面。fork 原实现逐字对齐，engine-rs books_state_routes.rs /
 * ops_routes.rs / series_backfill_routes.rs 同水位对偶。
 */
describe("timeline-series recycle routes (707)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "timeline-series-recycle-"));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const validTimeline = {
    version: 1,
    bookId: "b1",
    updatedAt: "2026-10-16T00:00:00.000Z",
    plotlines: [
      { id: "main", name: "主线", cells: [{ chapter: 1, title: "镜中醒来" }] },
      { id: "sub", name: "支线", cells: [] },
    ],
  };

  const seedBook = async (bookId: string, extra: Record<string, unknown> = {}) => {
    const dir = join(root, "books", bookId);
    await mkdir(dir, { recursive: true });
    await writeFile(join(dir, "book.json"), JSON.stringify({ id: bookId, title: "镜花缘", ...extra }), "utf-8");
  };

  it("timeline GET/PUT: 缺失 {timeline:null} + schema 校验 400 + 原子落盘回读", async () => {
    const app = createStudioServer({} as never, root);

    // GET：缺文件 → 200 {timeline:null}（可选面不制造 404 噪音）
    const missing = await app.request("/api/v1/books/b1/timeline");
    expect(missing.status).toBe(200);
    expect(await missing.json()).toEqual({ timeline: null });

    // PUT：非法 payload（version 非 1）→ 400
    const invalid = await app.request("/api/v1/books/b1/timeline", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ...validTimeline, version: 2 }),
    });
    expect(invalid.status).toBe(400);

    // PUT：plotline id 重复 → 400（refine 唯一性）
    const dup = await app.request("/api/v1/books/b1/timeline", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        ...validTimeline,
        plotlines: [
          { id: "main", name: "主线", cells: [] },
          { id: "main", name: "复用", cells: [] },
        ],
      }),
    });
    expect(dup.status).toBe(400);

    // PUT：bookId 与路由不符 → 400
    const mismatch = await app.request("/api/v1/books/b1/timeline", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ...validTimeline, bookId: "b2" }),
    });
    expect(mismatch.status).toBe(400);

    // PUT：合法 → 200 {ok:true} + story/timeline.json 落盘（尾随换行）
    const put = await app.request("/api/v1/books/b1/timeline", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(validTimeline),
    });
    expect(put.status).toBe(200);
    expect(await put.json()).toEqual({ ok: true });
    const stored = await readFile(join(root, "books", "b1", "story", "timeline.json"), "utf-8");
    expect(stored.endsWith("\n")).toBe(true);

    // GET 回读：解析后的 timeline 与写入一致
    const round = await app.request("/api/v1/books/b1/timeline");
    expect(round.status).toBe(200);
    expect(await round.json()).toEqual({ timeline: validTimeline });

    // GET：坏载荷 → {timeline:null}（静默吞掉 + 警告）
    await writeFile(join(root, "books", "b1", "story", "timeline.json"), "{broken", "utf-8");
    const corrupt = await app.request("/api/v1/books/b1/timeline");
    expect(await corrupt.json()).toEqual({ timeline: null });
  });

  it("timeline-auto-beats GET/PUT: 开关落 writing.autoTimelineBeats + 关闭删键", async () => {
    const app = createStudioServer({} as never, root);

    // GET：书不存在 → 404
    const missing = await app.request("/api/v1/books/b1/timeline-auto-beats");
    expect(missing.status).toBe(404);

    await seedBook("b1");

    // GET：无 writing 段 → {enabled:false, bookEnabled:null}
    const off = await app.request("/api/v1/books/b1/timeline-auto-beats");
    expect(off.status).toBe(200);
    expect(await off.json()).toEqual({ enabled: false, bookEnabled: null });

    // PUT：enabled:true → book.json writing.autoTimelineBeats=true
    const on = await app.request("/api/v1/books/b1/timeline-auto-beats", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled: true }),
    });
    expect(on.status).toBe(200);
    expect(await on.json()).toEqual({ ok: true, enabled: true });
    let book = JSON.parse(await readFile(join(root, "books", "b1", "book.json"), "utf-8")) as { writing?: Record<string, unknown> };
    expect(book.writing?.autoTimelineBeats).toBe(true);

    // GET：开启态 → {enabled:true, bookEnabled:true}
    const enabled = await app.request("/api/v1/books/b1/timeline-auto-beats");
    expect(await enabled.json()).toEqual({ enabled: true, bookEnabled: true });

    // PUT：enabled:false → 删键；writing 空则整体移除（缺省省略语义）
    const disable = await app.request("/api/v1/books/b1/timeline-auto-beats", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled: false }),
    });
    expect(await disable.json()).toEqual({ ok: true, enabled: false });
    book = JSON.parse(await readFile(join(root, "books", "b1", "book.json"), "utf-8"));
    expect(book.writing).toBeUndefined();

    // 关闭时保留同段其他键：writing.reviewMode 不被连带删除
    await seedBook("b1", { writing: { reviewMode: "manual", autoTimelineBeats: true } });
    const disableKeep = await app.request("/api/v1/books/b1/timeline-auto-beats", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(await disableKeep.json()).toEqual({ ok: true, enabled: false });
    book = JSON.parse(await readFile(join(root, "books", "b1", "book.json"), "utf-8"));
    expect(book.writing).toEqual({ reviewMode: "manual" });
  });

  it("series-id GET/PUT: 绑定/解绑 + isValidSeriesId 守卫 + 404", async () => {
    const app = createStudioServer({} as never, root);

    // GET：书不存在 → {seriesId:null}（fork 语义；Rust 同位为 404——备案）
    const missing = await app.request("/api/v1/books/b1/series-id");
    expect(missing.status).toBe(200);
    expect(await missing.json()).toEqual({ seriesId: null });

    // PUT：书不存在 → 404
    const putMissing = await app.request("/api/v1/books/b1/series-id", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ seriesId: "jing_hua_yuan" }),
    });
    expect(putMissing.status).toBe(404);

    await seedBook("b1");

    // PUT：非法 seriesId（连字符/大写/空格）→ 400
    for (const bad of ["jing-hua", "JingHua", "jing hua", ""]) {
      const badPut = await app.request("/api/v1/books/b1/series-id", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ seriesId: bad }),
      });
      expect(badPut.status).toBe(400);
    }

    // PUT：合法绑定 → 200 + book.json seriesId 落盘
    const bind = await app.request("/api/v1/books/b1/series-id", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ seriesId: "jing_hua_yuan" }),
    });
    expect(bind.status).toBe(200);
    expect(await bind.json()).toEqual({ ok: true, seriesId: "jing_hua_yuan" });
    let book = JSON.parse(await readFile(join(root, "books", "b1", "book.json"), "utf-8")) as { seriesId?: string };
    expect(book.seriesId).toBe("jing_hua_yuan");

    // GET：绑定态回读
    const bound = await app.request("/api/v1/books/b1/series-id");
    expect(await bound.json()).toEqual({ seriesId: "jing_hua_yuan" });

    // PUT：null 解绑 → 删键
    const unbind = await app.request("/api/v1/books/b1/series-id", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ seriesId: null }),
    });
    expect(await unbind.json()).toEqual({ ok: true, seriesId: null });
    book = JSON.parse(await readFile(join(root, "books", "b1", "book.json"), "utf-8"));
    expect(book.seriesId).toBeUndefined();
  });

  it("series canon GET/PUT: 空基线 + 条目 sanitize/name 码元序落盘 + 守卫", async () => {
    const app = createStudioServer({} as never, root);

    // GET：缺文件 → 空基线
    const missing = await app.request("/api/v1/series/jing_hua_yuan/canon");
    expect(missing.status).toBe(200);
    expect(await missing.json()).toEqual({ version: 1, seriesId: "jing_hua_yuan", entries: [] });

    // GET：非法 seriesId → 400
    const badId = await app.request("/api/v1/series/jing-hua/canon");
    expect(badId.status).toBe(400);

    // PUT：非法 seriesId → 400
    const putBadId = await app.request("/api/v1/series/jing-hua/canon", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ entries: [] }),
    });
    expect(putBadId.status).toBe(400);

    // PUT：entries 非数组 → 400
    const putNotArray = await app.request("/api/v1/series/jing_hua_yuan/canon", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ entries: "nope" }),
    });
    expect(putNotArray.status).toBe(400);

    // PUT：合法条目 → sanitize（非法条目跳过）+ name 码元序 + 落盘
    const put = await app.request("/api/v1/series/jing_hua_yuan/canon", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        entries: [
          { name: "沈观澜", kind: "person", summary: "追查失窃案的捕头" },
          { name: "林昭", kind: "person", summary: "残卷发现者", aliases: ["阿昭"], facts: ["剑修"] },
          { name: "", kind: "person", summary: "无名条目应被跳过" },
          { name: "残卷", kind: "nope-kind", summary: "非法 kind 兜底 other" },
        ],
      }),
    });
    expect(put.status).toBe(200);
    const putBody = await put.json() as { ok: boolean; entries: number };
    expect(putBody.ok).toBe(true);
    expect(putBody.entries).toBe(3);

    const stored = JSON.parse(
      await readFile(join(root, ".inkos", "series", "jing_hua_yuan.json"), "utf-8"),
    ) as { version: number; seriesId: string; entries: Array<{ name: string; kind: string; aliases?: string[] }> };
    expect(stored.version).toBe(1);
    expect(stored.seriesId).toBe("jing_hua_yuan");
    // name 码元序：CJK 按 < 比较（禁 locale 感知排序）
    expect(stored.entries.map((e) => e.name)).toEqual(["林昭", "残卷", "沈观澜"]);
    expect(stored.entries[0]!.aliases).toEqual(["阿昭"]);
    expect(stored.entries[1]!.kind).toBe("other");

    // GET：落盘后回读完整条目集
    const round = await app.request("/api/v1/series/jing_hua_yuan/canon");
    const roundBody = await round.json() as { entries: Array<{ name: string }> };
    expect(roundBody.entries).toHaveLength(3);
  });

  it("series-backfill: extract 守卫链 + existing 缺失 null + apply 无草稿 400", async () => {
    const app = createStudioServer({} as never, root);

    // extract：缺 sourceBookId → 400
    const noSource = await app.request("/api/v1/books/b1/series-backfill/extract", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(noSource.status).toBe(400);

    // extract：source == target → 400
    const selfSource = await app.request("/api/v1/books/b1/series-backfill/extract", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sourceBookId: "b1" }),
    });
    expect(selfSource.status).toBe(400);

    // extract：源书不存在 → 400
    const ghostSource = await app.request("/api/v1/books/b1/series-backfill/extract", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sourceBookId: "ghost" }),
    });
    expect(ghostSource.status).toBe(400);

    // existing：缺文件 → {content:null}
    const existing = await app.request("/api/v1/books/b1/series-backfill/existing");
    expect(existing.status).toBe(200);
    expect(await existing.json()).toEqual({ content: null });

    // apply：无草稿 → 400
    const applyNoDraft = await app.request("/api/v1/books/b1/series-backfill/apply", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(applyNoDraft.status).toBe(400);

    // apply：播种草稿 → overwrite 全量渲染（头部与 Rust render_backfill_markdown 逐字）
    const storyDir = join(root, "books", "b1", "story");
    await mkdir(storyDir, { recursive: true });
    await writeFile(
      join(storyDir, "series_backfill_draft.json"),
      JSON.stringify({
        version: 1,
        bookId: "b1",
        sourceBookId: "src",
        updatedAt: "2026-10-16T00:00:00.000Z",
        items: [
          { id: "it-1", category: "worldview", title: "残卷设定", content: "上古残卷共三枚" },
          { id: "it-2", category: "character", title: "林昭", content: "残卷发现者" },
        ],
      }),
      "utf-8",
    );
    const applyAll = await app.request("/api/v1/books/b1/series-backfill/apply", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(applyAll.status).toBe(200);
    expect(await applyAll.json()).toEqual({
      ok: true,
      path: "story/series_backfill.md",
      applied: 2,
      total: 2,
      mode: "overwrite",
    });
    const markdown = await readFile(join(storyDir, "series_backfill.md"), "utf-8");
    expect(markdown.startsWith("# 系列设定回填\n\n来源：《src》（src） · 抽取于 2026-10-16T00:00:00.000Z · 勾选 2 条\n\n")).toBe(true);
    expect(markdown).toContain("## [worldview] 残卷设定\n\n上古残卷共三枚\n\n");

    // apply：merge 模式——既有条目在前，勾选项按 (category, title) 去重后追加
    const applyMerge = await app.request("/api/v1/books/b1/series-backfill/apply", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ itemIds: ["it-1", "it-2"], mode: "merge" }),
    });
    expect(applyMerge.status).toBe(200);
    const mergeBody = await applyMerge.json() as { applied: number; total: number; mode: string };
    expect(mergeBody.mode).toBe("merge");
    expect(mergeBody.applied).toBe(2);
    expect(mergeBody.total).toBe(2);

    // apply：itemIds 全不命中 → 400
    const applyNone = await app.request("/api/v1/books/b1/series-backfill/apply", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ itemIds: ["nope"] }),
    });
    expect(applyNone.status).toBe(400);
  });
});
