import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 705 号：fork 写面回收第四批端到端——genre 三写面（703 余项行为测试补强）+
 * experience 写/删面（G13 PUT merge/DELETE 单条）+ deconstruct 统一拆书面
 * （G5/351，响应顶层 dossiers/pacing/markdown 包络）。fork 原实现逐字对齐，
 * engine-rs ops_routes.rs 同水位对偶。
 */
describe("write-face recycle routes (705: genre/experience/deconstruct)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "write-face-recycle-"));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const validEntry = {
    id: "learned_1",
    chapter: 0,
    kind: "technique",
    text: "用环境音代替直接写「安静」",
    enabled: true,
    createdAt: "2026-10-15T00:00:00.000Z",
  };

  it("genre create/PUT/copy: frontmatter 落盘 + INVALID_GENRE_ID 守卫", async () => {
    const app = createStudioServer({} as never, root);

    // create：缺 id/name → 400
    const missing = await app.request("/api/v1/genres/create", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: "x" }),
    });
    expect(missing.status).toBe(400);

    // create：路径穿越 id → 400（INVALID_GENRE_ID）
    const traversal = await app.request("/api/v1/genres/create", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: "../evil", name: "恶" }),
    });
    expect(traversal.status).toBe(400);

    // create：合法 → 200 + 项目级 genres/{id}.md（frontmatter 标量走 yamlScalar 引号契约）
    const created = await app.request("/api/v1/genres/create", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: "urban_fantasy", name: "都市奇幻", body: "## 叙事指导\n\n因果链完整。" }),
    });
    expect(created.status).toBe(200);
    expect(await created.json()).toEqual({ ok: true, id: "urban_fantasy" });
    const genreMd = await readFile(join(root, "genres", "urban_fantasy.md"), "utf-8");
    expect(genreMd).toContain('name: "都市奇幻"');
    expect(genreMd).toContain('id: "urban_fantasy"');
    expect(genreMd).toContain("## 叙事指导");

    // PUT：改写 profile → 200 + 文件更新
    const put = await app.request("/api/v1/genres/urban_fantasy", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ profile: { name: "都市奇幻·改", chapterTypes: ["推进章"] }, body: "改后正文" }),
    });
    expect(put.status).toBe(200);
    const putMd = await readFile(join(root, "genres", "urban_fantasy.md"), "utf-8");
    expect(putMd).toContain('name: "都市奇幻·改"');
    expect(putMd).toContain("改后正文");

    // PUT：路径穿越 → 400
    const putBad = await app.request("/api/v1/genres/a%2Fb", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ profile: {}, body: "" }),
    });
    expect(putBad.status).toBe(400);

    // copy：内置题材复制到项目级 → 200 + 文件在位
    const copy = await app.request("/api/v1/genres/cultivation/copy", { method: "POST" });
    expect(copy.status).toBe(200);
    const copyBody = await copy.json() as { ok: boolean; path: string };
    expect(copyBody.ok).toBe(true);
    expect(copyBody.path).toBe("genres/cultivation.md");
    const copied = await readFile(join(root, "genres", "cultivation.md"), "utf-8");
    expect(copied.length).toBeGreaterThan(0);

    // copy：不存在的题材 → 500
    const copyMissing = await app.request("/api/v1/genres/no_such_genre/copy", { method: "POST" });
    expect(copyMissing.status).toBe(500);
  });

  it("experience PUT merge/DELETE: 去重合并 + 单条删除 + 404", async () => {
    const app = createStudioServer({} as never, root);

    // PUT：非数组 entries → 400
    const bad = await app.request("/api/v1/books/b1/experience", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ entries: "nope" }),
    });
    expect(bad.status).toBe(400);

    // PUT：首次沉淀 → 200 {ok, entries}
    const put1 = await app.request("/api/v1/books/b1/experience", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ entries: [validEntry] }),
    });
    expect(put1.status).toBe(200);
    const put1Body = await put1.json() as { ok: boolean; entries: ReadonlyArray<{ id: string }> };
    expect(put1Body.ok).toBe(true);
    expect(put1Body.entries).toHaveLength(1);

    // 落盘形状：{ version: 1, entries }
    const stored = JSON.parse(await readFile(join(root, "books", "b1", "story", "experience.json"), "utf-8")) as {
      version: number;
      entries: ReadonlyArray<unknown>;
    };
    expect(stored.version).toBe(1);
    expect(stored.entries).toHaveLength(1);

    // PUT：同文本再沉淀 → 去重（merge 保序追加语义）
    const put2 = await app.request("/api/v1/books/b1/experience", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ entries: [{ ...validEntry, id: "learned_2" }] }),
    });
    expect(put2.status).toBe(200);
    const put2Body = await put2.json() as { entries: ReadonlyArray<{ id: string }> };
    expect(put2Body.entries).toHaveLength(1);
    expect(put2Body.entries[0]!.id).toBe("learned_1");

    // DELETE：不存在的条目 → 404
    const missing = await app.request("/api/v1/books/b1/experience/no_such_entry", { method: "DELETE" });
    expect(missing.status).toBe(404);

    // DELETE：已存在条目 → 200 + 文件更新
    const removed = await app.request("/api/v1/books/b1/experience/learned_1", { method: "DELETE" });
    expect(removed.status).toBe(200);
    const removedBody = await removed.json() as { ok: boolean; entries: ReadonlyArray<unknown> };
    expect(removedBody.ok).toBe(true);
    expect(removedBody.entries).toHaveLength(0);
    const after = await app.request("/api/v1/books/b1/experience");
    expect((await after.json() as { entries: ReadonlyArray<unknown> }).entries).toHaveLength(0);
  });

  it("deconstruct: 顶层包络（dossiers/pacing/markdown）+ 落盘 + publish 材料池", async () => {
    const app = createStudioServer({} as never, root);

    // 空 chapters → 400
    const empty = await app.request("/api/v1/books/b1/deconstruct", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(empty.status).toBe(400);

    // 合法拆书 → 200 + 顶层 markdown/dossiers/pacing + path
    const posted = await app.request("/api/v1/books/b1/deconstruct", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        name: "拆书·卷一",
        chapters: [
          { chapter: 1, characters: ["林昭", "沈观澜"], events: "林昭发现残卷，沈观澜追查失窃案" },
          { chapter: 2, characters: ["林昭"], events: "林昭修炼突破，残卷异动" },
          { chapter: 3, characters: ["沈观澜", "林昭"], events: "两人相遇，线索交汇" },
        ],
      }),
    });
    expect(posted.status).toBe(200);
    const body = await posted.json() as {
      dossiers: ReadonlyArray<{ name: string }>;
      pacing: { counts: Record<string, number>, strongHookDensity: number };
      markdown: string;
      path: string;
      publishedMaterialId?: string;
    };
    expect(body.markdown).toContain("林昭");
    expect(body.dossiers.length).toBeGreaterThan(0);
    expect(body.dossiers[0]!.name).toBe("林昭");
    expect(body.pacing.counts.untyped).toBe(3);
    expect(body.pacing.strongHookDensity).toBe(0);
    expect(body.path).toBe(".inkos/../books/b1/story/deconstruction/拆书·卷一.md");
    expect(body.publishedMaterialId).toBeUndefined();

    // 落盘：books/b1/story/deconstruction/{name}.md
    const md = await readFile(join(root, "books", "b1", "story", "deconstruction", "拆书·卷一.md"), "utf-8");
    expect(md).toBe(body.markdown);

    // publish：材料池双文件（md + manifest）
    const published = await app.request("/api/v1/books/b1/deconstruct", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        name: "拆书·发布",
        publish: true,
        chapters: [{ chapter: 1, characters: ["林昭"], events: "事件" }],
      }),
    });
    expect(published.status).toBe(200);
    const publishedBody = await published.json() as { publishedMaterialId?: string };
    // 清洗正则 [^\w-] 的 \w 为 ASCII 口径：CJK 字符逐字替换为 "-"（fork 逐字行为；
    // engine-rs 侧 705 号对齐同语义）。
    expect(publishedBody.publishedMaterialId).toBe("decon-b1------");
    const materialMd = await readFile(
      join(root, ".inkos", "materials", "decon-b1------.md"),
      "utf-8",
    );
    expect(materialMd).toContain("林昭");
    const manifest = JSON.parse(
      await readFile(join(root, ".inkos", "materials", "decon-b1------.json"), "utf-8"),
    ) as { source: string; markdownPath: string };
    expect(manifest.source).toBe("deconstruction");
    expect(manifest.markdownPath).toBe(".inkos/materials/decon-b1------.md");
  });
});
