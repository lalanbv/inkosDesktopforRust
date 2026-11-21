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
