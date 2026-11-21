//! R50/1652 号：docx 导出 golden 断言（双端一致性的 TS 锚面）——四纯函数
//! 组（escape/bodyParagraphs/paragraph/document）+ 三常量 part sha256 +
//! 全链产物（条目序、document.xml 结构、textutil 开档活体）。Rust 侧 =
//! engine-rs/tests/golden_chapter_docx_diff.rs 读同一向量文件差分。
import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { describe, expect, it } from "vitest";
import JSZip from "jszip";
import {
  DOCX_CONTENT_TYPES_XML,
  DOCX_RELS_XML,
  DOCX_STYLES_XML,
  buildExportArtifact,
  docxBodyParagraphs,
  docxDocumentXml,
  docxEscapeXml,
  docxParagraphXml,
} from "../interaction/export-artifact.js";
import { StateManager } from "../state/manager.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/chapter-docx-vectors.json"), "utf-8"),
) as {
  staticParts: Record<string, string>;
  /** 同 fixture 全链 word/document.xml sha256——Rust lib docx 测试同
   * fixture 断言同值（双端活体逐位对拍锚；fixture=docxFixture 四章两卷）。 */
  fullChainDocumentDigest: string;
  escape: Array<{ name: string; input: string; expected: string }>;
  bodyParagraphs: Array<{ name: string; input: string; expected: string[] }>;
  paragraph: Array<{ name: string; styleId: string; text: string; expected: string }>;
  document: Array<{
    name: string;
    bookTitle: string;
    volumes: Array<{ id: string; title: string; order: number }>;
    chapters: Array<{ number: number; title: string; volumeId?: string; paragraphs: string[] }>;
    expected: string;
  }>;
};

describe("docx golden（R50/1652 号）", () => {
  it.each(vectors.escape.map((v) => [v.name, v] as const))("escape: %s", (_name, vector) => {
    expect(docxEscapeXml(vector.input)).toBe(vector.expected);
  });

  it.each(vectors.bodyParagraphs.map((v) => [v.name, v] as const))("bodyParagraphs: %s", (_name, vector) => {
    expect([...docxBodyParagraphs(vector.input)]).toEqual(vector.expected);
  });

  it.each(vectors.paragraph.map((v) => [v.name, v] as const))("paragraph: %s", (_name, vector) => {
    expect(docxParagraphXml(vector.styleId, vector.text)).toBe(vector.expected);
  });

  it.each(vectors.document.map((v) => [v.name, v] as const))("document: %s", (_name, vector) => {
    expect(docxDocumentXml(vector.bookTitle, vector.volumes, vector.chapters)).toBe(vector.expected);
  });

  it("三常量 part sha256 对拍（Rust 字面镜像漂移守卫）", () => {
    expect(sha256(DOCX_CONTENT_TYPES_XML)).toBe(vectors.staticParts["[Content_Types].xml"]);
    expect(sha256(DOCX_RELS_XML)).toBe(vectors.staticParts["_rels/.rels"]);
    expect(sha256(DOCX_STYLES_XML)).toBe(vectors.staticParts["word/styles.xml"]);
  });

  it("golden 非空防空转", () => {
    for (const group of ["escape", "bodyParagraphs", "paragraph", "document"] as const) {
      expect(vectors[group].length).toBeGreaterThan(0);
    }
  });
});

function sha256(text: string): string {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

// ── 全链：StateManager 真实盘面 → docx 产物（条目序 + 结构 + 开档）──────

async function docxFixture(root: string): Promise<StateManager> {
  const state = new StateManager(root);
  const now = new Date().toISOString();
  await state.saveBookConfig("b1", {
    id: "b1", title: "风起云涌", genre: "general", platform: "other", status: "active",
    targetChapters: 4, chapterWordCount: 1000, language: "zh", createdAt: now, updatedAt: now,
    writing: { volumes: [
      { id: "v1", title: "第一卷 风起", order: 1 },
      { id: "v2", title: "第二卷 云涌", order: 2 },
    ] },
  } as Parameters<StateManager["saveBookConfig"]>[1]);
  const chapter = (number: number, volumeId?: string) => ({
    number, title: `第${number}章`, volumeId, wordCount: 5,
    provenance: "imported" as const, observations: [], createdAt: now, updatedAt: now,
  });
  await state.saveChapterIndex("b1", [chapter(1, "v1"), chapter(2, "v1"), chapter(3, "v2"), chapter(4)]);
  const chaptersDir = join(state.bookDir("b1"), "chapters");
  await mkdir(chaptersDir, { recursive: true });
  await writeFile(join(chaptersDir, "0001_第1章.md"), "# 第1章\n\n风起青萍。");
  await writeFile(join(chaptersDir, "0002_第2章.md"), "# 第2章\n\n青萍之末。");
  await writeFile(join(chaptersDir, "0003_第3章.md"), "# 第3章\n\n云涌微澜。");
  await writeFile(join(chaptersDir, "0004_第4章.md"), "# 第4章\n\n未分组直排。");
  return state;
}

describe("docx 全链（R50/1652 号）", () => {
  it("条目序固定 + document.xml 结构（卷 Heading1/章 Heading2/未分组直排）", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-docx-"));
    try {
      const state = await docxFixture(root);
      const artifact = await buildExportArtifact(state, "b1", { format: "docx" });
      expect(artifact.fileName).toBe("b1.docx");
      expect(artifact.contentType).toBe("application/vnd.openxmlformats-officedocument.wordprocessingml.document");
      expect(artifact.chaptersExported).toBe(4);
      const loaded = await JSZip.loadAsync(artifact.payload as Uint8Array);
      // jszip 载入补 word/ 目录条目——只断言文件条目序（=生成序）。
      expect(Object.values(loaded.files).filter((entry) => !entry.dir).map((entry) => entry.name))
        .toEqual(["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/styles.xml"]);
      const documentXml = await loaded.file("word/document.xml")!.async("string");
      // 标题/卷/章/正文结构（序 = 未分组(0) 前置 → 卷1 → 卷2）。
      const heading1s = [...documentXml.matchAll(/<w:pStyle w:val="Heading1"\/>.*?<w:t xml:space="preserve">([^<]*)<\/w:t>/g)].map((m) => m[1]);
      expect(heading1s).toEqual(["第一卷 风起", "第二卷 云涌"]);
      const order = [
        documentXml.indexOf("第4章</w:t>"),
        documentXml.indexOf("第一卷 风起</w:t>"),
        documentXml.indexOf("第1章</w:t>"),
        documentXml.indexOf("风起青萍。</w:t>"),
        documentXml.indexOf("第二卷 云涌</w:t>"),
        documentXml.indexOf("云涌微澜。</w:t>"),
      ];
      expect(order.every((pos, index) => index === 0 || pos > order[index - 1]!)).toBe(true);
      expect(documentXml).toContain('w:val="Title"');
      expect(documentXml.endsWith("</w:sectPr></w:body></w:document>")).toBe(true);
      // 常量 part 逐字节 = 生产常量。
      expect(await loaded.file("[Content_Types].xml")!.async("string")).toBe(DOCX_CONTENT_TYPES_XML);
      expect(await loaded.file("_rels/.rels")!.async("string")).toBe(DOCX_RELS_XML);
      expect(await loaded.file("word/styles.xml")!.async("string")).toBe(DOCX_STYLES_XML);
      // 双端活体逐位对拍锚：word/document.xml sha256（Rust docx_fixture 同
      // 数据断言同值；renderChapterDocument 重写标题行但正文段两端口径一致）。
      const digest = sha256(await loaded.file("word/document.xml")!.async("string"));
      if (vectors.fullChainDocumentDigest === "PENDING") {
        throw new Error(`digest 未登记，生产值=${digest}`);
      }
      expect(digest).toBe(vectors.fullChainDocumentDigest);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("chapterFilter 子集：仅导出勾选章", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-docx-subset-"));
    try {
      const state = await docxFixture(root);
      const artifact = await buildExportArtifact(state, "b1", { format: "docx", chapterFilter: [2] });
      expect(artifact.chaptersExported).toBe(1);
      const loaded = await JSZip.loadAsync(artifact.payload as Uint8Array);
      const documentXml = await loaded.file("word/document.xml")!.async("string");
      expect(documentXml).toContain("第2章</w:t>");
      expect(documentXml).toContain("青萍之末。</w:t>");
      expect(documentXml).not.toContain("第1章</w:t>");
      expect(documentXml).not.toContain("云涌微澜。</w:t>");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  // textutil 活体开档（macOS 本机自动可读性用例——施工图 §1.5；Word/WPS/Pages
  // 三端人工走查备案变更记录）。非 darwin 平台跳过。
  (process.platform === "darwin" ? it : it.skip)("textutil 可开档并提取正文", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-docx-textutil-"));
    try {
      const state = await docxFixture(root);
      const artifact = await buildExportArtifact(state, "b1", { format: "docx" });
      const docxPath = join(root, "b1.docx");
      await writeFile(docxPath, artifact.payload as Uint8Array);
      const { stdout } = await promisify(execFile)("textutil", ["-convert", "txt", "-stdout", docxPath]);
      expect(stdout).toContain("风起云涌");
      expect(stdout).toContain("风起青萍。");
      expect(stdout).toContain("第一卷 风起");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});
