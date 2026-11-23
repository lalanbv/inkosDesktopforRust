import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { syncWorkSourceArtifacts } from "@actalk/inkos-core";
import { createStudioServer } from "../api/server.js";

/**
 * R50/1647 号：导出双端点 HTTP 薄层——GET /export approvedOnly 回填
 * （fb8c8e3e 状态机替换时 TS 侧遗失，Rust 47 号移植体一直保留=同端点
 * 行为漂移）+ export-save chapters 子集/正典扩展名/空子集报错。
 * 事务与打包语义面由 core work-export flow + Rust e2e books50 覆盖。
 */
describe("export routes (R50/1647)", () => {
  const roots: string[] = [];
  afterEach(() => {
    for (const root of roots.splice(0)) {
      // tmpdir 清理交由进程退出（createStudioServer 无常驻句柄面）。
      void root;
    }
  });

  function seedApp() {
    const root = mkdtempSync(join(tmpdir(), "export-routes-"));
    roots.push(root);
    const workDir = join(root, "works", "b1");
    const bookDir = join(workDir, "source");
    mkdirSync(join(bookDir, "chapters"), { recursive: true });
    // export-save 走 executeExplicitCapabilityTool → loadWorkManifest 读取。
    mkdirSync(join(root, ".inkos"), { recursive: true });
    writeFileSync(
      join(workDir, "work.json"),
      JSON.stringify({
        version: 2, id: "b1", title: "导出测试书", profileId: "longform-novel",
        language: "zh", status: "draft", lineage: [], artifacts: [], metadata: {},
        createdAt: "2026-11-22T00:00:00.000Z", updatedAt: "2026-11-22T00:00:00.000Z",
      }),
    );
    writeFileSync(
      join(bookDir, "book.json"),
      JSON.stringify({
        id: "b1", title: "导出测试书", platform: "other", genre: "xuanhuan",
        status: "active", targetChapters: 8, chapterWordCount: 1600, language: "zh",
        createdAt: "2026-11-22T00:00:00.000Z", updatedAt: "2026-11-22T00:00:00.000Z",
      }),
    );
    const rows = [
      { number: 1, title: "风起", status: "approved", body: "林动睁开双眼。" },
      { number: 2, title: "云涌", status: "ready-for-review", body: "正文二乙。" },
      { number: 3, title: "收束", status: "approved", body: "正文三丙。" },
    ] as const;
    for (const row of rows) {
      writeFileSync(
        join(bookDir, "chapters", `${String(row.number).padStart(4, "0")}_${row.title}.md`),
        `# ${row.title}\n\n${row.body}\n`,
      );
    }
    writeFileSync(
      join(bookDir, "chapters", "index.json"),
      JSON.stringify(
        rows.map(({ number, title, status }) => ({
          number, title, status, wordCount: 8,
          createdAt: "2026-11-22T00:00:00.000Z", updatedAt: "2026-11-22T00:00:00.000Z",
          auditIssues: [], lengthWarnings: [],
        })),
      ),
    );
    const app = createStudioServer({} as never, root);
    return { root, bookDir, app };
  }

  /** export-save 走 capability 运行时的交付证据链——导出源须已注册为
   * work artifacts（生产路径=写作链落盘时注册；测试夹具用 source-sync
   * 等价注册，与 work-export flow 测试同惯用形）。 */
  async function seedRegisteredApp() {
    const seeded = seedApp();
    await syncWorkSourceArtifacts({ projectRoot: seeded.root, workId: "b1", accept: true });
    return seeded;
  }

  it("GET /export approvedOnly=true → 仅 approved 章（遗失能力回填）", async () => {
    const { app } = seedApp();
    const response = await app.request("/api/v1/books/b1/export?format=md&approvedOnly=true");
    expect(response.status).toBe(200);
    const text = await response.text();
    expect(text).toContain("林动睁开双眼");
    expect(text).toContain("正文三丙");
    expect(text).not.toContain("正文二乙");
  });

  it("GET /export chapters 勾选子集 → 子集正文（既有 1645 面回归）", async () => {
    const { app } = seedApp();
    const response = await app.request("/api/v1/books/b1/export?format=md&chapters=2");
    expect(response.status).toBe(200);
    const text = await response.text();
    expect(text).toContain("正文二乙");
    expect(text).not.toContain("林动睁开双眼");
  });

  it("GET /export 空子集 → 500 Export failed（Rust 同文案对齐）", async () => {
    const { app } = seedApp();
    const response = await app.request("/api/v1/books/b1/export?format=md&chapters=99");
    expect(response.status).toBe(500);
    expect(((await response.json()) as { error: string }).error).toBe("Export failed");
  });

  it("GET/PUT /project/writing-goal 日目标键（缺省 500 → 设 800 → 回读，R51/1654）", async () => {
    const { root, app } = seedApp();
    // project config 文件（loadRawConfig 读 inkos.json——空对象起）。
    writeFileSync(join(root, "inkos.json"), "{}");
    // 缺省 500（project config 无 writing 段）。
    const initial = await app.request("/api/v1/project/writing-goal");
    expect(initial.status).toBe(200);
    expect(((await initial.json()) as { dailyGoalWords: number }).dailyGoalWords).toBe(500);
    // PUT 正整数 → 落盘 writing.dailyGoalWords。
    const saved = await app.request("/api/v1/project/writing-goal", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: '{"dailyGoalWords": 800}',
    });
    expect(saved.status).toBe(200);
    expect(((await saved.json()) as { dailyGoalWords: number }).dailyGoalWords).toBe(800);
    // 回读 + 盘面（root 的 project config 文件含键）。
    const reread = await app.request("/api/v1/project/writing-goal");
    expect(((await reread.json()) as { dailyGoalWords: number }).dailyGoalWords).toBe(800);
    const configText = JSON.stringify(JSON.parse(readFileSync(join(root, "inkos.json"), "utf-8") as unknown as string));
    expect(configText).toContain("dailyGoalWords");
    // 非正整数 → 500 拒。
    const invalid = await app.request("/api/v1/project/writing-goal", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: '{"dailyGoalWords": -3}',
    });
    expect(invalid.status).toBe(500);
  });

  it("prompt-packs 端点族全链（Node 回退腿，R54/1663 回座）", async () => {
    const { app } = seedApp();
    const list = await app.request("/api/v1/prompt-packs");
    expect(list.status).toBe(200);
    const listBody = (await list.json()) as { packs: ReadonlyArray<{ id: string }>; prompts: ReadonlyArray<{ id: string; packId: string; source: string; overridden: boolean }> };
    expect(listBody.packs.map((pack) => pack.id)).toEqual(["longform", "play", "interactive-film"]);
    expect(listBody.prompts).toHaveLength(12);
    const writer = listBody.prompts.find((entry) => entry.id === "longform.writer")!;
    expect(writer.source).toBe("builtin");
    expect(writer.overridden).toBe(false);
    const put = await app.request("/api/v1/prompt-packs/LONGFORM.Writer", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: "# 回退腿覆盖" }),
    });
    expect(put.status).toBe(200);
    const putBody = (await put.json()) as { prompt: { source: string; overridden: boolean; content: string }; regressionHint: string };
    expect(putBody.prompt.source).toBe("project");
    expect(putBody.prompt.content).toBe("# 回退腿覆盖");
    expect(putBody.regressionHint).toBe("prompt-changed");
    const versions = await app.request("/api/v1/prompt-packs/longform.writer/versions");
    const versionsBody = (await versions.json()) as { versions: ReadonlyArray<{ ts: string; action: string; content: string }> };
    expect(versionsBody.versions).toHaveLength(1);
    expect(versionsBody.versions[0]!.action).toBe("put");
    const restore = await app.request("/api/v1/prompt-packs/longform.writer/versions/restore", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ts: versionsBody.versions[0]!.ts }),
    });
    expect(restore.status).toBe(200);
    const after = (await (await app.request("/api/v1/prompt-packs/longform.writer/versions")).json()) as { versions: ReadonlyArray<{ action: string }> };
    expect(after.versions.map((row) => row.action)).toEqual(["restore", "put"]);
    const del = await app.request("/api/v1/prompt-packs/longform.writer", { method: "DELETE" });
    expect(del.status).toBe(200);
    const delBody = (await del.json()) as { prompt: { source: string; overridden: boolean } };
    expect(delBody.prompt.source).toBe("builtin");
    expect(delBody.prompt.overridden).toBe(false);
    const unknown = await app.request("/api/v1/prompt-packs/ghost.prompt", { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ content: "x" }) });
    expect(unknown.status).toBe(404);
  });

  it("GET /usage/budget-status——services 预算+账本投影（R55/1661）", async () => {
    const { root, app } = seedApp();
    writeFileSync(
      join(root, "inkos.json"),
      JSON.stringify({ llm: { services: { deepseek: { baseUrl: "https://api.deepseek.com", dailyTokenBudget: 1000 } } } }),
    );
    const today = new Date().toISOString().slice(0, 10);
    const row = (service: string, total: number) =>
      JSON.stringify({ ts: `${today}T01:00:00.000Z`, serviceKey: service, model: "m", promptTokens: Math.floor(total / 2), completionTokens: total - Math.floor(total / 2), totalTokens: total });
    mkdirSync(join(root, ".inkos"), { recursive: true });
    writeFileSync(join(root, ".inkos", "usage-log.jsonl"), [row("api.deepseek.com", 850), row("other.example", 999)].join("\n") + "\n");
    const response = await app.request("/api/v1/usage/budget-status");
    expect(response.status).toBe(200);
    const body = (await response.json()) as { statuses: Array<{ serviceKey: string; daily: number; ratio: number; level: string; confirmRequired: boolean }>; todayKey: string };
    expect(body.statuses).toHaveLength(1);
    expect(body.statuses[0]).toMatchObject({ serviceKey: "deepseek", daily: 850, ratio: 0.85, level: "warn", confirmRequired: false });
    expect(body.todayKey).toBe(today);
  });

  it("PUT 章保存记账 → GET /writing-log-rows 行面（R51/1653）", async () => {
    const { app } = seedApp();
    // 章 1 正文改长（wordCount 变化 → 账本一行）。
    const edit = await app.request("/api/v1/books/b1/chapters/1", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: "# 风起\n\n林动睁开双眼，看见了一片崭新的天地与远方的山脉。" }),
    });
    expect(edit.status).toBe(200);
    const response = await app.request("/api/v1/writing-log-rows");
    expect(response.status).toBe(200);
    const body = (await response.json()) as {
      rows: Array<{ bookId: string; ts: string; chapterNum: number; before: number; after: number }>;
    };
    expect(body.rows).toHaveLength(1);
    expect(body.rows[0]).toMatchObject({ bookId: "b1", chapterNum: 1, before: 8 });
    expect(body.rows[0]!.after).toBeGreaterThan(body.rows[0]!.before);
    expect(typeof body.rows[0]!.ts).toBe("string");
  });

  it("GET /export format=docx → OOXML 4-part（R50/1652 全量面）", async () => {
    const { app } = seedApp();
    const response = await app.request("/api/v1/books/b1/export?format=docx");
    expect(response.status).toBe(200);
    expect(response.headers.get("Content-Type")).toBe(
      "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    );
    expect(response.headers.get("Content-Disposition")).toContain('filename="b1.docx"');
    const bytes = Buffer.from(await response.arrayBuffer());
    // zip 魔数（jszip STORE）。
    expect([...bytes.subarray(0, 4)]).toEqual([0x50, 0x4b, 0x03, 0x04]);
    // jszip 是 core 依赖（studio 无权直引）——条目名/正文按字节断言；内容级
    // 解包由 core golden-chapter-docx 测试（jszip 全量解析）覆盖。
    for (const entry of ["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/styles.xml"]) {
      expect(bytes.includes(Buffer.from(entry, "utf-8"))).toBe(true);
    }
    expect(bytes.includes(Buffer.from("导出测试书", "utf-8"))).toBe(true);
    expect(bytes.includes(Buffer.from("林动睁开双眼。", "utf-8"))).toBe(true);
  });

  it("GET /export format=docx chapters 子集 → 子集正文（R50/1652 子集面）", async () => {
    const { app } = seedApp();
    const response = await app.request("/api/v1/books/b1/export?format=docx&chapters=2");
    expect(response.status).toBe(200);
    const bytes = Buffer.from(await response.arrayBuffer());
    expect(bytes.includes(Buffer.from("正文二乙。", "utf-8"))).toBe(true);
    expect(bytes.includes(Buffer.from("林动睁开双眼", "utf-8"))).toBe(false);
  });

  it("POST export-save md-zip chapters 子集 → 落盘 b1.zip 正典扩展名 + 子集条目", async () => {
    const { bookDir, app } = await seedRegisteredApp();
    const response = await app.request("/api/v1/books/b1/export-save", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"format":"md-zip","chapters":[1,3]}',
    });
    expect(response.status).toBe(200);
    const body = (await response.json()) as { ok: boolean; chapters: number; path: string; format: string };
    expect(body.ok).toBe(true);
    expect(body.format).toBe("md-zip");
    // 此前 body 的 chapters 被静默忽略=全量 3 章（契约谎言）。
    expect(body.chapters).toBe(2);
    // 落盘名=正典扩展名 b1.zip（此前 fmt 原样拼出 b1.md-zip）。
    expect(body.path.endsWith("b1.zip")).toBe(true);
    expect(body.path.endsWith("md-zip")).toBe(false);
    // jszip 是 core 依赖（studio 无权直引）——条目名以 UTF-8 明文出现在
    // local file header 与 central directory，按字节断言；内容级解包由
    // core work-export flow 测试（jszip 全量解析）覆盖。
    const zipBytes = readFileSync(join(bookDir, "b1.zip"));
    expect(zipBytes.includes(Buffer.from("00_ungrouped/0001_风起.md", "utf-8"))).toBe(true);
    expect(zipBytes.includes(Buffer.from("00_ungrouped/0003_收束.md", "utf-8"))).toBe(true);
    expect(zipBytes.includes(Buffer.from("manifest.json", "utf-8"))).toBe(true);
    expect(zipBytes.includes(Buffer.from("0002_云涌", "utf-8"))).toBe(false);
  });
});
