import type {LengthSpec} from '../models/length-governance.js';
import { mkdir, readFile, readdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { createHash } from "node:crypto";
import { EPub } from "epub-gen-memory";
import JSZip from "jszip";
import {renderChapterDocument} from '../utils/chapter-document.js';
import {readChapterHeading} from '../utils/chapter-splitter.js';
import {buildLengthSpec, chapterLengthDelivery, countChapterLength, defaultChapterLength} from '../utils/length-metrics.js';
import { chapterFileNumber, compareByCodePoint } from "../utils/chapter-files.js";
import { writeTextAtomic } from "../utils/atomic-write.js";

export interface ExportStateLike {
  readonly bookDir: (bookId: string) => string;
  readonly loadBookConfig: (bookId: string) => Promise<{
    readonly title: string; readonly language?: string; readonly chapterWordCount?: number;
    readonly minChapterLength?: number; readonly maxChapterLength?: number;
    /** R49/1644 号：卷有序列表（md-zip 打包按卷目录）。 */
    readonly writing?: { readonly volumes?: ReadonlyArray<{ readonly id: string; readonly title: string; readonly order: number }> };
  }>;
  readonly loadChapterIndex: (bookId: string) => Promise<ReadonlyArray<{
    readonly number: number;
    readonly title?: string;
    readonly wordCount: number;
    readonly lengthSpec?: LengthSpec;
    /** R49/1644 号：卷归属（md-zip 打包按卷分组目录）。 */
    readonly volumeId?: string;
    /** R50/1647 号：approvedOnly 过滤依据（fb8c8e3e 状态机替换时遗失的
     * 能力回填——Rust build_export_artifact 移植体一直保留）。 */
    readonly status?: string;
  }>>;
}

export interface ExportArtifact {
  readonly kind: "book_exported";
  readonly workId: string;
  readonly sourceDigest: string;
  readonly exportSourcePaths: readonly string[];
  readonly delivery: Awaited<ReturnType<typeof readBookExportSource>>["delivery"];
  readonly outputPath: string;
  readonly fileName: string;
  readonly chaptersExported: number;
  readonly totalWords: number;
  readonly format: "txt" | "md" | "epub" | "md-zip" | "docx";
  readonly contentType: string;
  readonly payload: string | Uint8Array;
}

/** Read once: measurements, exported text and the receipt describe the same source. */
export async function readBookExportSource(
  state: ExportStateLike,
  bookId: string,
  // R50/1647 号：导出章选择器（读盘前于索引层生效——镜像 Rust
  // build_export_artifact 先过滤后读盘；过滤后空集=「No chapters to export.」
  // 双端同文案，此前 TS 空子集静默产出空工件=漂移）。
  select?: (chapter: { readonly number: number; readonly status?: string }) => boolean,
) {
  const allIndex = await state.loadChapterIndex(bookId);
  const index = select ? allIndex.filter(select) : allIndex;
  const book = await state.loadBookConfig(bookId);
  if (!allIndex.length || !index.length) throw new Error("No chapters to export.");
  const chaptersDir = join(state.bookDir(bookId), "chapters");
  // 源一致性校验面对全量索引——部分导出不豁免未导出章的对账（409 语义
  // 与既有行为一致；此前首版把过滤后索引喂进校验=子集外章误报 unindexed）。
  const files = buildChapterFileLookup(await readdir(chaptersDir), allIndex);
  const chapters = await Promise.all(index.map(async chapter => {
    const file = files.get(chapter.number)!;
    const raw = await readFile(join(chaptersDir, file), "utf-8");
    const heading = readChapterHeading(raw.trimStart().split(/\r?\n/u)[0] ?? "");
    const language = book.language === "en" || book.language === undefined && heading?.language === "en" ? "en" : "zh";
    const title = chapter.title ?? heading?.title ?? file.replace(/^\d+_/u, "").replace(/\.md$/u, "");
    const markdown = renderChapterDocument(chapter.number, title, raw, language);
    const spec = chapter.lengthSpec ?? buildLengthSpec(book.chapterWordCount ?? defaultChapterLength(language), language, book);
    const count = countChapterLength(markdown, spec.countingMode);
    return {
      number: chapter.number,
      title,
      volumeId: chapter.volumeId ?? undefined,
      file,
      markdown,
      count,
      delivery: chapterLengthDelivery(count, spec),
    };
  }));
  const checks = chapters.flatMap(chapter => chapter.delivery ? [{chapterNumber: chapter.number, ...chapter.delivery}] : []);
  const delivery = {
    status: checks.some(check => check.status === "needs_revision") ? "needs_revision" as const
      : checks.length ? "checks_passed" as const : "unverified" as const,
    chapters: checks,
  };
  const sourceDigest = createHash("sha256").update(JSON.stringify({title: book.title, language: book.language, chapters})).digest("hex");
  return {book, chapters, delivery, sourceDigest, totalWords: chapters.reduce((sum, chapter) => sum + chapter.count, 0)};
}

export interface ExportSourceMismatchDetails {
  readonly missingChapterNumbers: number[];
  readonly duplicateChapterNumbers: number[];
  readonly duplicateIndexNumbers: number[];
  readonly unindexedFiles: string[];
}

export class ChapterExportSourceError extends Error {
  readonly code = "CHAPTER_EXPORT_SOURCE_MISMATCH";
  constructor(readonly details: ExportSourceMismatchDetails) {
    super(`Chapter index and source files differ: ${JSON.stringify(details)}. Inspect these files and reconcile the chapter index through chapter import before exporting.`);
    this.name = "ChapterExportSourceError";
  }
}

/**
 * 1681 号：源一致性校验抽成纯函数——共享 golden 向量（export-source-mismatch-vectors.json，
 * engine-rs tests/golden_export_source_diff.rs 读同一文件）需要一个不依赖 StateManager 夹具的
 * 直接入口。四类细节任一非空即拒（拒绝而非静默部分导出：缺章/错章的导出物是数据完整性事故，
 * 同章双形态的择优是需要用户裁决的状态——两腿同此口径，Rust 腿本轮起对齐）。
 */
export function checkChapterExportSource(
  files: ReadonlyArray<string>,
  indexedNumbers: ReadonlyArray<number>,
): ExportSourceMismatchDetails | undefined {
  const lookup = new Map<number, string>();
  const indexed = new Set<number>();
  const duplicateIndexNumbers = new Set<number>();
  for (const number of indexedNumbers) {
    if (indexed.has(number)) duplicateIndexNumbers.add(number);
    indexed.add(number);
  }
  const duplicateChapterNumbers = new Set<number>();
  const unindexedFiles: string[] = [];
  // 1681 号：排序从裸 sort（UTF-16 码元序）改码点序——与 Rust `&str` 字节序恒等
  //（compareByCodePoint，854 号立的尺子），增补平面字符（emoji 文件名）双端消息逐字同序。
  for (const file of [...files].sort(compareByCodePoint)) {
    if (!file.endsWith(".md")) continue;
    // 1678 号：章文件的认定与索引重建同一集合（chapterFileNumber——正典 `NNNN_标题.md` /
    // legacy `NNNN-标题.md` / 裸 `NNNN.md`）。此前只认正典形态：旧命名的章被报成「缺文件 /
    // 未入索引」，这样的书在 Node 腿上导不出。校验本身不变——真缺文件、未入索引的章、
    // 同章两个文件，照样拒绝。
    const chapterNumber = chapterFileNumber(file);
    if (chapterNumber === undefined || !indexed.has(chapterNumber)) {
      unindexedFiles.push(file);
      continue;
    }
    if (lookup.has(chapterNumber)) duplicateChapterNumbers.add(chapterNumber);
    lookup.set(chapterNumber, file);
  }
  const details = {
    missingChapterNumbers: [...indexed].filter(number => !lookup.has(number)).sort((a,b) => a-b),
    duplicateChapterNumbers: [...duplicateChapterNumbers].sort((a,b) => a-b),
    duplicateIndexNumbers: [...duplicateIndexNumbers].sort((a,b) => a-b),
    unindexedFiles,
  };
  if (Object.values(details).some(items => items.length > 0)) return details;
  return undefined;
}

function buildChapterFileLookup(files: ReadonlyArray<string>, chapters: ReadonlyArray<{ readonly number: number }>): ReadonlyMap<number, string> {
  const details = checkChapterExportSource(files, chapters.map(chapter => chapter.number));
  if (details) throw new ChapterExportSourceError(details);
  // 校验通过后重建 lookup：四类细节全空 ⇒ 无缺章、无双形态、无孤儿，逐章必有唯一文件。
  const lookup = new Map<number, string>();
  for (const file of [...files].sort(compareByCodePoint)) {
    if (!file.endsWith(".md")) continue;
    const chapterNumber = chapterFileNumber(file);
    if (chapterNumber === undefined) continue;
    lookup.set(chapterNumber, file);
  }
  return lookup;
}

function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

/** 214 号：导出供共享 golden 差分（engine tests/golden_export_diff.rs 同向量）。 */
export function markdownToSimpleHtml(markdown: string): { title: string; html: string } {
  // 214 号：\s 含换行——「# + 纯空白行」会把下一行正文误当标题（golden 差分
  // 抓到）；改行内空白 [ \t]，且 trim 后为空回退 Untitled（对齐 Rust）。
  const matched = markdown.match(/^#[ \t]+(.+)/m)?.[1]?.trim();
  const title = matched ? matched : "Untitled Chapter";
  const html = markdown
    .split("\n")
    .filter((line) => !line.startsWith("#"))
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => `<p>${escapeHtml(line)}</p>`)
    .join("\n");
  return { title, html };
}

/** R50/1645 号：md-zip 条目路径构造（纯函数，双端 golden 锁定）。
 * 卷目录 = `{order:02}_{title}`（路径分隔符 sanitize），未分组 =
 * `00_ungrouped`；章文件 = `{NNNN}_{title}.md`。Rust 镜像 =
 * export_artifact.rs `md_zip_entry_path`。 */
export function mdZipEntryPath(
  volumeId: string | undefined,
  volumes: ReadonlyArray<{ readonly id: string; readonly title: string; readonly order: number }>,
  chapterNumber: number,
  chapterTitle: string,
): string {
  // R62/1690 号性质测试抓出：章/卷标题含连续点（如 ".."）会落进 zip 段——
  // 解包器 zip-slip 家族。非法字符替换之外折叠连续点为 "-"（单点保留），
  // 并剥离段尾点（"标题." + ".md" 相连同样成 ".."）。
  const sanitizeName = (raw: string): string =>
    raw.replace(/[/\\:*?"<>|]/g, "-").replace(/\.{2,}/g, "-").replace(/\.+$/u, "");
  const dir = volumeId
    ? (() => {
      const volume = volumes.find((candidate) => candidate.id === volumeId);
      return volume ? `${String(volume.order).padStart(2, "0")}_${sanitizeName(volume.title)}` : "00_ungrouped";
    })()
    : "00_ungrouped";
  return `${dir}/${String(chapterNumber).padStart(4, "0")}_${sanitizeName(chapterTitle)}.md`;
}

// ── R50/1652 号：docx 导出纯函数族（Rust 镜像 = export_artifact.rs docx_*
// 系列，golden chapter-docx-vectors.json 锁定）。最小 4-part OOXML
// （ECMA-376）：[Content_Types].xml / _rels/.rels / word/document.xml /
// word/styles.xml——属性序固定、无缩进无注释、UTF-8 无 BOM = duel 条目级
// 逐位可比。零新依赖（jszip STORE，与 md-zip 链同款）。 ─────────────────

/** XML 五实体转义（& 最先——防二次转义）。 */
export function docxEscapeXml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

/** 章正文切段：非 `#` 行 trim 后非空各一段——与 EPUB markdownToSimpleHtml
 * 行语义一致（同书双格式分段一致；章标题由索引 title 单独成 Heading2 段，
 * 正文中的标题行滤除）。 */
export function docxBodyParagraphs(markdown: string): ReadonlyArray<string> {
  return markdown
    .split("\n")
    .filter((line) => !line.startsWith("#"))
    .map((line) => line.trim())
    .filter(Boolean);
}

const DOCX_EAST_ASIA_FONT = "SimSun";

/** (styleId, text) → `<w:p>`：pPr 样式引用 + run 级 rPr eastAsia（中文字族
 * 声明不依赖 styles 继承链）+ xml:space="preserve"。空文本 = 无 run 空段。 */
export function docxParagraphXml(styleId: string, text: string): string {
  if (!text) {
    return `<w:p><w:pPr><w:pStyle w:val="${styleId}"/></w:pPr></w:p>`;
  }
  return `<w:p><w:pPr><w:pStyle w:val="${styleId}"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="${DOCX_EAST_ASIA_FONT}"/></w:rPr><w:t xml:space="preserve">${docxEscapeXml(text)}</w:t></w:r></w:p>`;
}

export interface DocxVolume {
  readonly id: string;
  readonly title: string;
  readonly order: number;
}

export interface DocxChapterInput {
  readonly number: number;
  readonly title: string;
  readonly volumeId?: string;
  readonly paragraphs: ReadonlyArray<string>;
}

/** 章序 = (卷 order（未分组/未知卷 = 0 前置，与 md-zip `00_ungrouped` 桶序
 * 一致）, 章号) 稳定排序；卷桶首章前发 Heading1（卷名）。未分组章直排不造
 * 卷标题——docx 无目录树语义，直排更符 Word 阅读习惯（与 md-zip 的
 * `00_ungrouped` 目录形态有意不同）。sectPr = A4 + 1 英寸页边距（Word
 * 惯例 twips 值）。 */
export function docxDocumentXml(
  bookTitle: string,
  volumes: ReadonlyArray<DocxVolume>,
  chapters: ReadonlyArray<DocxChapterInput>,
): string {
  const volumeOf = (volumeId: string | undefined) =>
    volumeId ? volumes.find((candidate) => candidate.id === volumeId) : undefined;
  const ordered = chapters
    .map(chapter => ({chapter, volume: volumeOf(chapter.volumeId)}))
    .sort((a, b) => {
      const orderA = a.volume?.order ?? 0;
      const orderB = b.volume?.order ?? 0;
      return orderA !== orderB ? orderA - orderB : a.chapter.number - b.chapter.number;
    });
  const body: string[] = [docxParagraphXml("Title", bookTitle)];
  let prevBucketId: string | null | undefined;
  for (const {chapter, volume} of ordered) {
    const bucketId = volume?.id ?? null;
    if (volume && bucketId !== prevBucketId) {
      body.push(docxParagraphXml("Heading1", volume.title));
    }
    prevBucketId = bucketId;
    body.push(docxParagraphXml("Heading2", chapter.title));
    for (const paragraph of chapter.paragraphs) {
      body.push(docxParagraphXml("Normal", paragraph));
    }
  }
  const sectPr = `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>`;
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>${body.join("")}${sectPr}</w:body></w:document>`;
}

/** 三常量 part（双端字面一致，duel sha256 对拍锁定；document.xml 由
 * docxDocumentXml 产出，条目序 = [Content_Types].xml → _rels/.rels →
 * word/document.xml → word/styles.xml）。 */
export const DOCX_CONTENT_TYPES_XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>';

export const DOCX_RELS_XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>';

export const DOCX_STYLES_XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="SimSun"/><w:sz w:val="24"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:b/><w:sz w:val="56"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="36"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="30"/></w:rPr></w:style></w:styles>';

export async function buildExportArtifact(
  state: ExportStateLike,
  bookId: string,
  options: {
    readonly format?: "txt" | "md" | "epub" | "md-zip" | "docx";
    readonly outputPath?: string;
    /** R50/1645 号：章节勾选子集（Scrivener Compile 语义；缺省=全部）。 */
    readonly chapterFilter?: ReadonlyArray<number>;
    /** R50/1647 号：仅导出 approved 章（fb8c8e3e 遗失回填）。与 Rust 同
     * 优先级：approvedOnly 置位时 chapterFilter 被忽略（互斥非交集）。 */
    readonly approvedOnly?: boolean;
  },
): Promise<ExportArtifact> {
  const format = options.format ?? "txt";
  const filter = options.chapterFilter;
  const {book, chapters, delivery, sourceDigest} = await readBookExportSource(
    state,
    bookId,
    (entry) =>
      options.approvedOnly
        ? entry.status === "approved"
        : filter
          ? filter.includes(entry.number)
          : true,
  );
  const totalWords = chapters.reduce((sum, chapter) => sum + chapter.count, 0);

  const bookDir = state.bookDir(bookId);
  const outputPath = options.outputPath ?? join(bookDir, "exports", `${bookId}.${format}`);
  const receipt = {kind: "book_exported" as const, workId: bookId, sourceDigest, delivery,
    exportSourcePaths: chapters.map(chapter=>`source/chapters/${chapter.file}`)};

  if (format === "epub") {
    const epubChapters: Array<{ title: string; content: string }> = [];
    for (const chapter of chapters) {
      const { title, html } = markdownToSimpleHtml(chapter.markdown);
      epubChapters.push({ title, content: html });
    }
    const epubInstance = new EPub(
      { title: book.title, lang: book.language === "en" ? "en" : "zh-CN" },
      epubChapters,
    );
    return {
      ...receipt,
      outputPath,
      fileName: `${bookId}.epub`,
      chaptersExported: chapters.length,
      totalWords,
      format,
      contentType: "application/epub+zip",
      payload: await epubInstance.genEpub(),
    };
  }

  if (format === "docx") {
    // R50/1652 号：最小 4-part OOXML（条目序固定，STORE 零压缩=双端同为
    // method-0，duel 解包逐条目可比）。Rust 镜像 = export_artifact.rs
    // ExportFormat::Docx 分支（StoredZipWriter）。
    const zip = new JSZip();
    zip.file("[Content_Types].xml", DOCX_CONTENT_TYPES_XML);
    zip.file("_rels/.rels", DOCX_RELS_XML);
    const documentXml = docxDocumentXml(
      book.title,
      book.writing?.volumes ?? [],
      chapters.map(chapter => ({
        number: chapter.number,
        title: chapter.title ?? "",
        volumeId: chapter.volumeId,
        paragraphs: docxBodyParagraphs(chapter.markdown),
      })),
    );
    zip.file("word/document.xml", documentXml);
    zip.file("word/styles.xml", DOCX_STYLES_XML);
    return {
      ...receipt,
      outputPath,
      fileName: `${bookId}.docx`,
      chaptersExported: chapters.length,
      totalWords,
      format,
      contentType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
      payload: await zip.generateAsync({ type: "uint8array", compression: "STORE" }),
    };
  }

  if (format === "md-zip") {
    // R50/1645 号：整书打包（Rust ExportFormat::MdZip 镜像）——按卷目录
    // （{order:02}_{title}，未分组 00_ungrouped）+ 元数据 manifest.json。
    const zip = new JSZip();
    const volumes = book.writing?.volumes ?? [];
    const manifestChapters: Array<Record<string, unknown>> = [];
    for (const chapter of chapters) {
      const path = mdZipEntryPath(chapter.volumeId, volumes, chapter.number, chapter.title ?? "");
      zip.file(path, chapter.markdown);
      manifestChapters.push({
        number: chapter.number,
        title: chapter.title ?? "",
        volumeId: chapter.volumeId ?? null,
        path,
        wordCount: chapter.count,
      });
    }
    const manifest = {
      book: { id: bookId, title: book.title, language: book.language ?? null },
      volumes,
      chapters: manifestChapters,
    };
    zip.file("manifest.json", JSON.stringify(manifest, null, 2));
    return {
      ...receipt,
      outputPath,
      fileName: `${bookId}.zip`,
      chaptersExported: chapters.length,
      totalWords,
      format,
      contentType: "application/zip",
      payload: await zip.generateAsync({ type: "uint8array" }),
    };
  }

  const parts: string[] = [];
  parts.push(format === "md" ? `# ${book.title}` : book.title);
  for (const chapter of chapters) {
    parts.push(chapter.markdown);
  }

  return {
    ...receipt,
    outputPath,
    fileName: `${bookId}.${format}`,
    chaptersExported: chapters.length,
    totalWords,
    format,
    contentType: format === "md" ? "text/markdown; charset=utf-8" : "text/plain; charset=utf-8",
    payload: parts.join("\n\n"),
  };
}

export async function writeExportArtifact(
  state: ExportStateLike,
  bookId: string,
  options: {
    readonly format?: "txt" | "md" | "epub" | "md-zip" | "docx";
    readonly outputPath?: string;
    readonly chapterFilter?: ReadonlyArray<number>;
    /** R50/1647 号：approvedOnly 透传（互斥优先级同 buildExportArtifact）。 */
    readonly approvedOnly?: boolean;
  },
): Promise<Omit<ExportArtifact, "payload" | "contentType" | "fileName">> {
  const artifact = await buildExportArtifact(state, bookId, options);
  await mkdir(dirname(artifact.outputPath), { recursive: true });
  await writeTextAtomic(artifact.outputPath, artifact.payload);
  return {
    kind: artifact.kind,
    workId: artifact.workId,
    sourceDigest: artifact.sourceDigest,
    exportSourcePaths: artifact.exportSourcePaths,
    delivery: artifact.delivery,
    outputPath: artifact.outputPath,
    chaptersExported: artifact.chaptersExported,
    totalWords: artifact.totalWords,
    format: artifact.format,
  };
}
