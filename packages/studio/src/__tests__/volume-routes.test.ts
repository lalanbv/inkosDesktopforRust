import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { chapterSetRevision } from "@actalk/inkos-core";

/**
 * R49/1644 号：卷/幕元数据三端点 HTTP 薄层——GET/PUT volumes（全量事务：
 * 新建/改名/重排/删卷合一，删卷自动迁移章归默认卷）+ PUT chapters/:num/volume
 * 行级归属。纯函数面由 golden vectors 双端锁（三组验收）。
 */
describe("volume routes (R49/1644)", () => {
  const roots: string[] = [];
  afterEach(() => {
    // 1669 号：此前空转（`void root`）——系统 tmp 累计 290 个 volume-routes-* 根。
    for (const root of roots.splice(0)) {
      rmSync(root, { recursive: true, force: true });
    }
  });

  function seedApp() {
    const root = mkdtempSync(join(tmpdir(), "volume-routes-"));
    roots.push(root);
    const bookDir = join(root, "works", "b1", "source");
    mkdirSync(join(bookDir, "chapters"), { recursive: true });
    writeFileSync(join(bookDir, "chapters", "0001_风起.md"), "# 第一章\n\n正文一。\n");
    writeFileSync(join(bookDir, "chapters", "0002_灵脉.md"), "# 第二章\n\n正文二。\n");
    const index = [
      { number: 1, title: "风起", status: "ready-for-review", wordCount: 6, createdAt: "2026-11-21T00:00:00.000Z", updatedAt: "2026-11-21T00:00:00.000Z", auditIssues: [], lengthWarnings: [] },
      { number: 2, title: "灵脉", status: "ready-for-review", wordCount: 6, createdAt: "2026-11-21T00:00:00.000Z", updatedAt: "2026-11-21T00:00:00.000Z", auditIssues: [], lengthWarnings: [] },
    ];
    writeFileSync(join(bookDir, "chapters", "index.json"), JSON.stringify(index));
    const bookJson = {
      id: "b1", title: "卷书", platform: "other", genre: "xuanhuan", status: "active",
      targetChapters: 8, chapterWordCount: 1600, language: "zh",
      createdAt: "2026-11-21T00:00:00.000Z", updatedAt: "2026-11-21T00:00:00.000Z",
    };
    writeFileSync(join(bookDir, "book.json"), JSON.stringify(bookJson));
    const app = createStudioServer({} as never, root);
    return { root, bookDir, app };
  }

  it("GET 缺省卷空数组（存量书零迁移）→ PUT 新建两卷 → GET 回读有序", async () => {
    const { app } = seedApp();
    const empty = await app.request("/api/v1/books/b1/volumes");
    expect(empty.status).toBe(200);
    expect(((await empty.json()) as { volumes: unknown[] }).volumes).toEqual([]);

    const put = await app.request("/api/v1/books/b1/volumes", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        volumes: [
          { id: "vol-2", title: "第二卷", order: 2 },
          { id: "vol-1", title: "第一卷", order: 1 },
        ],
      }),
    });
    expect(put.status).toBe(200);
    const body = (await put.json()) as { ok: boolean; volumes: Array<{ id: string; order: number }> };
    expect(body.ok).toBe(true);
    // 响应按 order 升序（sortVolumes 投影）。
    expect(body.volumes.map((volume) => volume.id)).toEqual(["vol-1", "vol-2"]);

    const reget = await app.request("/api/v1/books/b1/volumes");
    expect(((await reget.json()) as { volumes: Array<{ id: string }> }).volumes.map((v) => v.id)).toEqual([
      "vol-1", "vol-2",
    ]);
  });

  it("PUT volumes 负路径：缺列表 400 逐字、重复 id 400 逐字（双端同文）", async () => {
    const { app } = seedApp();
    const missing = await app.request("/api/v1/books/b1/volumes", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    expect(missing.status).toBe(400);
    expect(((await missing.json()) as { error: string }).error).toBe("Volumes list is required.");
    const dup = await app.request("/api/v1/books/b1/volumes", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ volumes: [{ id: "a", title: "甲", order: 1 }, { id: "a", title: "乙", order: 2 }] }),
    });
    expect(dup.status).toBe(400);
    expect(((await dup.json()) as { error: string }).error).toBe("Volume ids must be unique.");
  });

  it("删卷迁移：卷内章 volumeId 清除（归默认卷）", async () => {
    const { bookDir, app } = seedApp();
    // 建卷 + 归属章 1。
    await app.request("/api/v1/books/b1/volumes", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ volumes: [{ id: "vol-1", title: "第一卷", order: 1 }] }),
    });
    const assign = await app.request("/api/v1/books/b1/chapters/1/volume", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ volumeId: "vol-1" }),
    });
    expect(assign.status).toBe(200);
    const indexAfterAssign = JSON.parse(
      readFileSync(join(bookDir, "chapters", "index.json"), "utf-8"),
    ) as Array<{ number: number; volumeId?: string }>;
    expect(indexAfterAssign.find((row) => row.number === 1)?.volumeId).toBe("vol-1");

    // 删卷（PUT 不含 vol-1）→ 章 1 volumeId 键清除。
    const putEmpty = await app.request("/api/v1/books/b1/volumes", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ volumes: [] }),
    });
    expect(putEmpty.status).toBe(200);
    const index = JSON.parse(
      readFileSync(join(bookDir, "chapters", "index.json"), "utf-8"),
    ) as Array<{ number: number; volumeId?: string }>;
    expect(index.find((row) => row.number === 1)?.volumeId).toBeUndefined();
  });

  it("PUT chapters/:num/volume 负路径：缺 volumeId 400 逐字、未知卷 400 逐字、章缺失 404", async () => {
    const { app } = seedApp();
    const missing = await app.request("/api/v1/books/b1/chapters/1/volume", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    expect(missing.status).toBe(400);
    expect(((await missing.json()) as { error: string }).error).toBe("Volume id is required.");
    const unknownPut = await app.request("/api/v1/books/b1/chapters/1/volume", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: '{"volumeId":"ghost"}',
    });
    expect(unknownPut.status).toBe(400);
    expect(((await unknownPut.json()) as { error: string }).error).toBe("Volume ghost not found");
    // 卷校验先于章校验（双端同序）——先建卷再测章缺失 404。
    await app.request("/api/v1/books/b1/volumes", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ volumes: [{ id: "vol-1", title: "第一卷", order: 1 }] }),
    });
    const noChapter = await app.request("/api/v1/books/b1/chapters/9/volume", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ volumeId: "vol-1" }),
    });
    expect(noChapter.status).toBe(404);
    expect(((await noChapter.json()) as { error: string }).error).toBe("Chapter 9 not found");
  });

  it("归属更新不改章集合 revision 语义（指纹面只含 number/wordCount/updatedAt）", async () => {
    const { bookDir, app } = seedApp();
    await app.request("/api/v1/books/b1/volumes", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ volumes: [{ id: "vol-1", title: "第一卷", order: 1 }] }),
    });
    const before = chapterSetRevision(
      JSON.parse(readFileSync(join(bookDir, "chapters", "index.json"), "utf-8")) as never,
    );
    await app.request("/api/v1/books/b1/chapters/1/volume", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: '{"volumeId":"vol-1"}',
    });
    const after = chapterSetRevision(
      JSON.parse(readFileSync(join(bookDir, "chapters", "index.json"), "utf-8")) as never,
    );
    // 卷归属不在指纹面（仅结构操作乐观并发面）——语义备案。
    expect(before).toBe(after);
  });
});
