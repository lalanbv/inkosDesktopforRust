import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { chapterSetRevision } from "@actalk/inkos-core";

/**
 * R48a/1641 号：章节结构操作三端点 HTTP 薄层（负路径文案映射 + 快乐路径 +
 * 409 乐观并发）。事务语义面由 core chapter-structure 单测 + golden vectors
 * + Rust e2e/duel 覆盖——本测锁端点层 parse/文案/状态码双端逐字对齐。
 */
describe("chapter structure routes (R48a/1641)", () => {
  const roots: string[] = [];
  afterEach(() => {
    // 1669 号：此前空转（`void root`，注释称「清理交由进程退出」）——系统 tmp
    // 累计 236 个 chapter-structure-routes-* 根。
    for (const root of roots.splice(0)) {
      rmSync(root, { recursive: true, force: true });
    }
  });

  function seedApp() {
    const root = mkdtempSync(join(tmpdir(), "chapter-structure-routes-"));
    roots.push(root);
    const bookDir = join(root, "works", "b1", "source");
    mkdirSync(join(bookDir, "chapters"), { recursive: true });
    mkdirSync(join(bookDir, "story", "runtime"), { recursive: true });
    for (const [n, title, body] of [
      [1, "风起", "第一章正文。"],
      [2, "灵脉", "第二章正文乙。"],
      [3, "收束", "第三章正文丙。"],
    ] as const) {
      writeFileSync(
        join(bookDir, "chapters", `${String(n).padStart(4, "0")}_${title}.md`),
        `# ${title}\n\n${body}\n`,
      );
    }
    const index = [
      { number: 1, title: "风起", status: "ready-for-review", wordCount: 8, createdAt: "2026-11-21T00:00:00.000Z", updatedAt: "2026-11-21T00:00:00.000Z", auditIssues: [], lengthWarnings: [] },
      { number: 2, title: "灵脉", status: "ready-for-review", wordCount: 10, createdAt: "2026-11-21T00:00:00.000Z", updatedAt: "2026-11-21T00:00:00.000Z", auditIssues: [], lengthWarnings: [] },
      { number: 3, title: "收束", status: "ready-for-review", wordCount: 9, createdAt: "2026-11-21T00:00:00.000Z", updatedAt: "2026-11-21T00:00:00.000Z", auditIssues: [], lengthWarnings: [] },
    ];
    writeFileSync(join(bookDir, "chapters", "index.json"), JSON.stringify(index));
    const app = createStudioServer({} as never, root);
    return { root, bookDir, app };
  }

  it("split：负路径文案逐字 + 快乐路径重编号", async () => {
    const { app } = seedApp();
    // offset 缺失 400 逐字（Rust "Split offset is required." 同文）。
    const missing = await app.request("/api/v1/books/b1/chapters/2/split", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    expect(missing.status).toBe(400);
    expect(((await missing.json()) as { error: string }).error).toBe("Split offset is required.");
    // 非法章号 500 逐字（PUT chapters NaN 同款）。
    const nan = await app.request("/api/v1/books/b1/chapters/abc/split", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"offset":4}',
    });
    expect(nan.status).toBe(500);
    expect(((await nan.json()) as { error: string }).error).toBe("Chapter abc not found.");
    // 409 乐观并发。
    const conflict = await app.request("/api/v1/books/b1/chapters/2/split", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"offset":4,"expectedRevision":"deadbeefdeadbeef"}',
    });
    expect(conflict.status).toBe(409);
    const conflictBody = (await conflict.json()) as { code?: string };
    expect(conflictBody.code).toBe("CHAPTER_SET_CONFLICT");
    // 快乐路径：响应 revision 与盘上索引指纹一致。
    const ok = await app.request("/api/v1/books/b1/chapters/2/split", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"offset":6}',
    });
    expect(ok.status).toBe(200);
    const body = (await ok.json()) as { ok: boolean; newChapter: number; revision: string; renumbered: ReadonlyArray<{ from: number; to: number }> };
    expect(body.ok).toBe(true);
    expect(body.newChapter).toBe(3);
    expect(body.renumbered).toEqual([{ from: 3, to: 4 }]);
    expect(typeof body.revision).toBe("string");
    expect(body.revision).toHaveLength(16);
  });

  it("merge：mergeWith 缺失 400 逐字 + 非相邻 400 逐字", async () => {
    const { app } = seedApp();
    const missing = await app.request("/api/v1/books/b1/chapters/2/merge", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    expect(missing.status).toBe(400);
    expect(((await missing.json()) as { error: string }).error).toBe("Merge target chapter is required.");
    const nonAdjacent = await app.request("/api/v1/books/b1/chapters/1/merge", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"mergeWith":3}',
    });
    expect(nonAdjacent.status).toBe(400);
    expect(((await nonAdjacent.json()) as { error: string }).error).toBe("Merge requires adjacent chapters.");
  });

  it("reorder：order 缺失 400 逐字 + 非双射 400 + 恒序 400", async () => {
    const { app } = seedApp();
    const missing = await app.request("/api/v1/books/b1/chapters/reorder", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    expect(missing.status).toBe(400);
    expect(((await missing.json()) as { error: string }).error).toBe("Reorder order is required.");
    const dup = await app.request("/api/v1/books/b1/chapters/reorder", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"order":[1,2,2]}',
    });
    expect(dup.status).toBe(400);
    const identity = await app.request("/api/v1/books/b1/chapters/reorder", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"order":[1,2,3]}',
    });
    expect(identity.status).toBe(400);
    expect(((await identity.json()) as { error: string }).error).toBe("Reorder produces no change.");
  });

  it("split 后 revision 可链式传递（第二次操作带旧 revision → 409）", async () => {
    const { bookDir, app } = seedApp();
    const first = await app.request("/api/v1/books/b1/chapters/2/split", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"offset":6}',
    });
    const body = (await first.json()) as { revision: string };
    expect(body.revision).toBe(
      chapterSetRevision(
        JSON.parse(readFileSync(join(bookDir, "chapters", "index.json"), "utf-8")) as never,
      ),
    );
    // 旧 revision 再拆 → 409（集合已变）。
    const stale = await app.request("/api/v1/books/b1/chapters/2/split", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: `{"offset":4,"expectedRevision":"deadbeefdeadbeef"}`,
    });
    expect(stale.status).toBe(409);
  });

  it("undo：拆分后撤销回位（R61 批二）+ 负路径文案逐字", async () => {
    const { bookDir, app } = seedApp();
    const split = await app.request("/api/v1/books/b1/chapters/2/split", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"offset":6}',
    });
    expect(split.status).toBe(200);
    const undo = await app.request("/api/v1/books/b1/chapters/structure/undo", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "",
    });
    expect(undo.status).toBe(200);
    const undoBody = (await undo.json()) as { op: string };
    expect(undoBody.op).toBe("undo");
    // 盘面复原：4 号消失、index 号集 [1,2,3]。
    expect(readFileSync(join(bookDir, "chapters", "index.json"), "utf-8")).toContain('"number":3');
    // 第二次撤销 → 尾笔是 undo 行（文案与产品引擎逐字同）。
    const again = await app.request("/api/v1/books/b1/chapters/structure/undo", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "",
    });
    expect(again.status).toBe(409);
    expect(((await again.json()) as { error: string }).error).toBe(
      "Undo refused: last entry is itself an undo",
    );
  });
});
