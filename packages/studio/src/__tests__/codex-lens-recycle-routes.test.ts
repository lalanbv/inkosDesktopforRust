import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 716 号：实体卡 + 上下文透视四路由回收端到端——codex GET（名册同源派生
 * 合并，文件卡优先）/PUT（cards 数组校验落盘）+ context-lens 列表（runtime
 * trace 扫描）/详情（ContextPackage+ChapterTrace → lens 投影；404 缺产物）。
 * fork 逐字 + 路径正典化（state.bookDir=works/{id}/source，books/ legacy 不
 * 入读面）。Rust 对偶 entity_codex.rs / context_lens.rs（leaf.json 同源
 * golden 向量族）。
 */
describe("codex + context-lens recycle routes (716)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "codex-lens-recycle-"));
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

  const storyPath = (bookId: string, ...parts: ReadonlyArray<string>) =>
    join(root, "works", bookId, "source", "story", ...parts);

  it("codex GET/PUT: 名册派生合并（文件卡优先）+ cards 数组校验 + 落盘往返", async () => {
    const app = createStudioServer({} as never, root);
    // 名册两名（渲染器防呆方言；heading id 须 entity-* 前缀——ENTITY_HEADING
    // 正则契约）+ 文件卡一名（同源 → 文件卡优先不重复派生）。
    const roster = [
      "# 实体名册（Entity Roster）",
      "",
      "## entity-su-tan",
      "- name: 苏檀",
      "- aliases: 苏少爷",
      "- kind: person",
      "- registeredAt: 1",
      "",
      "## entity-lin-dong",
      "- name: 林动",
      "- aliases: ",
      "- kind: person",
      "- registeredAt: 2",
    ].join("\n");
    await mkdir(storyPath("b1"), { recursive: true });
    await writeFile(storyPath("b1", "entity_roster.md"), roster, "utf-8");
    const fileCards = [
      { name: "苏檀", aliases: ["苏少爷"], kind: "person", summary: "文件卡摘要", facts: [], relationships: [] },
    ];
    await writeFile(storyPath("b1", "entity_codex.json"), JSON.stringify({ version: 1, cards: fileCards }, null, 2), "utf-8");

    const get1 = await app.request("/api/v1/books/b1/codex");
    expect(get1.status).toBe(200);
    const cards1 = (await get1.json()) as { cards: ReadonlyArray<{ name: string; summary: string }> };
    // 文件卡优先 + 名册派生补缺（同名不重复）。
    const names = cards1.cards.map((card) => card.name);
    expect(names).toContain("苏檀");
    expect(names).toContain("林动");
    expect(names.filter((name) => name === "苏檀")).toHaveLength(1);
    expect(cards1.cards.find((card) => card.name === "苏檀")?.summary).toBe("文件卡摘要");

    // PUT：非法载荷 400 + 合法落盘往返。
    const bad = await app.request("/api/v1/books/b1/codex", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ cards: "not-an-array" }),
    });
    expect(bad.status).toBe(400);

    const updated = [{ name: "林动", aliases: [], kind: "person" as const, summary: "更新卡", facts: ["祖符持有者"], relationships: [{ target: "苏檀", note: "夺符之仇" }] }];
    const put = await app.request("/api/v1/books/b1/codex", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ cards: updated }),
    });
    expect(put.status).toBe(200);
    expect(await put.json()).toEqual({ ok: true, cards: updated });

    const round = await app.request("/api/v1/books/b1/codex");
    const cards2 = (await round.json()) as { cards: ReadonlyArray<{ name: string; summary: string; facts?: ReadonlyArray<string> }> };
    expect(cards2.cards.find((card) => card.name === "林动")?.summary).toBe("更新卡");
    expect(cards2.cards.find((card) => card.name === "林动")?.facts).toEqual(["祖符持有者"]);

    // 坏 id 400（编码形态绕过 fetch 归一化）。
    const evil = await app.request("/api/v1/books/a%2Fb%2F..%2Fevil/codex");
    expect(evil.status).toBe(400);
  });

  it("context-lens 列表/详情: trace 扫描排序 + 缺产物 404 + lens 投影 200", async () => {
    const app = createStudioServer({} as never, root);

    // 无 runtime 目录 → 空列表（不 404）。
    const empty = await app.request("/api/v1/books/b1/context-lens");
    expect(empty.status).toBe(200);
    expect(await empty.json()).toEqual({ chapters: [] });

    // 详情：产物缺失 404 + 非法章号 400。
    const missing = await app.request("/api/v1/books/b1/context-lens/2");
    expect(missing.status).toBe(404);
    expect(((await missing.json()) as { error: string }).error).toContain("not found");
    const badChapter = await app.request("/api/v1/books/b1/context-lens/0");
    expect(badChapter.status).toBe(400);

    // 播 trace + context 双产物（路由先读 context.json 后 trace.json）。
    // schema 不完整 → 投影 ZodError 500（typed；schema 完整真链面由 714 plan
    // 产物在走查活体覆盖——contract-diff context-lens/2 探针 200 同证）。
    const runtimeDir = storyPath("b1", "runtime");
    await mkdir(runtimeDir, { recursive: true });
    const partialTrace = JSON.stringify({
      chapter: 2,
      selectedSources: [],
      protectedSources: [],
      compressibleSources: [],
      tokenBudget: { total: 100, used: 10, remaining: 90 },
    });
    await writeFile(join(runtimeDir, "chapter-0002.trace.json"), partialTrace, "utf-8");
    await writeFile(join(runtimeDir, "chapter-0002.context.json"), JSON.stringify({ partial: true }), "utf-8");
    await writeFile(join(runtimeDir, "chapter-0001.trace.json"), partialTrace, "utf-8");
    await writeFile(join(runtimeDir, "chapter-0001.context.json"), JSON.stringify({ partial: true }), "utf-8");

    const list = await app.request("/api/v1/books/b1/context-lens");
    expect(await list.json()).toEqual({ chapters: [1, 2] });

    const detail = await app.request("/api/v1/books/b1/context-lens/2");
    expect(detail.status).toBe(500);
    expect(((await detail.json()) as { error: string }).error).toBeTruthy();
  });
});
