import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 717 号：写作质量面四路由回收端到端——detect/:chapter + detect-all（ai-tells
 * 结构规则零 LLM）/ eval（book-eval 启发式，auditPassRate status 基计算）/
 * draft（fire-and-forget LLM 管线，激活经 runWithAgentContext 注入 production
 * 绑定）。fork 逐字 + 两点 2.0 适配（LengthSpec bounds 化告警、落盘段
 * saveChapter 四参一体）。Rust 对偶 ai_tells.rs / book_eval.rs（contract-diff
 * 双端 body 同构已证——fixture 同文阈值无漂移）。
 */
describe("writing quality recycle routes (717)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "writing-quality-recycle-"));
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

  const chapter = (num: number, title: string, body: string) =>
    `# 第${num}章 ${title}\n\n${body}`;

  const seedBook = async (bookId: string, chapters: ReadonlyArray<{ num: number; title: string; body: string; status?: string }>) => {
    const sourceDir = join(root, "works", bookId, "source");
    const dir = join(sourceDir, "chapters");
    await mkdir(dir, { recursive: true });
    const index = chapters.map(({ num, title, body, status }) => ({
      number: num,
      title,
      ...(status ? { status } : {}),
      wordCount: body.replace(/\s+/g, "").length,
      createdAt: "2026-10-16T00:00:00.000Z",
      updatedAt: "2026-10-16T00:00:00.000Z",
      observations: [],
      provenance: "generated" as const,
    }));
    await writeFile(join(dir, "index.json"), JSON.stringify(index, null, 2), "utf-8");
    for (const { num, title, body } of chapters) {
      await writeFile(join(dir, `${String(num).padStart(4, "0")}_${title}.md`), chapter(num, title, body), "utf-8");
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
        createdAt: "2026-10-16T00:00:00.000Z",
        updatedAt: "2026-10-16T00:00:00.000Z",
      }),
      "utf-8",
    );
  };

  const aiHeavyBody = "然而，他并未察觉。与此同时，城市的另一端，另一场阴谋正在酝酿。然而，命运的齿轮已经开始转动。与此同时，暗流涌动。然而，一切都太迟了。与此同时，钟声响起。然而，无人听见。与此同时，夜色渐深。";

  it("detect/:chapter + detect-all: 结构规则命中 + 404/全量扫描", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1", [
      { num: 1, title: "转折连发", body: aiHeavyBody },
      { num: 2, title: "干净章", body: "他推开柴门，院里的老槐树落了一地黄叶。灶上的粥还温着，猫蜷在门槛边打盹。" },
    ]);

    // 单章：AI 腔样本命中结构规则（转折公式/段落形态）。
    const hit = await app.request("/api/v1/books/b1/detect/1", { method: "POST" });
    expect(hit.status).toBe(200);
    const hitBody = (await hit.json()) as { chapterNumber: number; issues: ReadonlyArray<{ category: string }> };
    expect(hitBody.chapterNumber).toBe(1);
    expect(hitBody.issues.length).toBeGreaterThan(0);

    // 未命中章 404。
    const missing = await app.request("/api/v1/books/b1/detect/9", { method: "POST" });
    expect(missing.status).toBe(404);

    // 全量：两章齐扫（含干净章）。
    const all = await app.request("/api/v1/books/b1/detect-all", { method: "POST" });
    expect(all.status).toBe(200);
    const allBody = (await all.json()) as { bookId: string; results: ReadonlyArray<{ chapterNumber: number; issues: ReadonlyArray<unknown> }> };
    expect(allBody.bookId).toBe("b1");
    expect(allBody.results.map((r) => r.chapterNumber)).toEqual([1, 2]);
    expect(allBody.results[1]?.issues.length).toBe(0);

    // 坏 id 400。
    const evil = await app.request("/api/v1/books/a%2Fb%2F..%2Fevil/detect-all", { method: "POST" });
    expect(evil.status).toBe(400);
  });

  it("eval: auditPassRate status 基计算 + qualityScore 加权 + 章节过滤", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1", [
      { num: 1, title: "已过审", body: "他推开柴门，院里的老槐树落了一地黄叶。", status: "approved" },
      { num: 2, title: "草稿中", body: "铜镜里的世界倒悬着。", status: "drafted" },
    ]);

    const res = await app.request("/api/v1/books/b1/eval");
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      bookId: string;
      totalChapters: number;
      auditPassRate: number;
      qualityScore: number;
      chapters: ReadonlyArray<{ number: number; status: string }>;
    };
    expect(body.bookId).toBe("b1");
    expect(body.totalChapters).toBe(2);
    // status 基：audited = 非草稿态 = 仅第 1 章（approved）→ 通过率 100。
    expect(body.auditPassRate).toBe(100);
    expect(body.qualityScore).toBeGreaterThan(0);
    expect(body.chapters.map((c) => c.status)).toEqual(["approved", "drafted"]);

    // chapters 过滤参数（fork 契约：逗号分隔章号子集）。
    const filtered = await app.request("/api/v1/books/b1/eval?chapters=2");
    const filteredBody = (await filtered.json()) as { totalChapters: number; auditPassRate: number };
    expect(filteredBody.totalChapters).toBe(1);
    // 仅草稿章 → audited 空 → 缺省满分（fork 语义）。
    expect(filteredBody.auditPassRate).toBe(100);
  });

  it("draft: fire-and-forget 200 drafting + 坏 id 400（LLM 结果走 SSE 广播）", async () => {
    const app = createStudioServer({} as never, root);
    await seedBook("b1", [
      { num: 1, title: "镜中醒来", body: "苏檀睁开眼，头顶是一面巨大的铜镜。" },
    ]);

    const res = await app.request("/api/v1/books/b1/draft", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ wordCount: 800 }),
    });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ status: "drafting", bookId: "b1" });

    const evil = await app.request("/api/v1/books/a%2Fb%2F..%2Fevil/draft", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(evil.status).toBe(400);

    // fire-and-forget 后台链与 afterEach 清理竞态——等 LLM 连接失败路径收口
    // （draft:error 广播完成后落盘不再推进）。
    await new Promise((resolve) => setTimeout(resolve, 2500));
  });
});
