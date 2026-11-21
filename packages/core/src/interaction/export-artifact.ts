import type {LengthSpec} from '../models/length-governance.js';
import { mkdir, readFile, readdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { createHash } from "node:crypto";
import { EPub } from "epub-gen-memory";
import JSZip from "jszip";
import {renderChapterDocument} from '../utils/chapter-document.js';
import {readChapterHeading} from '../utils/chapter-splitter.js';
import {buildLengthSpec, chapterLengthDelivery, countChapterLength, defaultChapterLength} from '../utils/length-metrics.js';
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
  readonly format: "txt" | "md" | "epub" | "md-zip";
  readonly contentType: string;
  readonly payload: string | Uint8Array;
}

/** Read once: measurements, exported text and the receipt describe the same source. */
export async function readBookExportSource(state: ExportStateLike, bookId: string) {
  const index = await state.loadChapterIndex(bookId);
  const book = await state.loadBookConfig(bookId);
  if (!index.length) throw new Error("No chapters to export.");
  const chaptersDir = join(state.bookDir(bookId), "chapters");
  const files = buildChapterFileLookup(await readdir(chaptersDir), index);
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

export class ChapterExportSourceError extends Error {
  readonly code = "CHAPTER_EXPORT_SOURCE_MISMATCH";
  constructor(readonly details: {
    readonly missingChapterNumbers: number[];
    readonly duplicateChapterNumbers: number[];
    readonly duplicateIndexNumbers: number[];
    readonly unindexedFiles: string[];
  }) {
    super(`Chapter index and source files differ: ${JSON.stringify(details)}. Inspect these files and reconcile the chapter index through chapter import before exporting.`);
    this.name = "ChapterExportSourceError";
  }
}

function buildChapterFileLookup(files: ReadonlyArray<string>, chapters: ReadonlyArray<{ readonly number: number }>): ReadonlyMap<number, string> {
  const lookup = new Map<number, string>();
  const indexed = new Set<number>();
  const duplicateIndexNumbers = new Set<number>();
  for (const chapter of chapters) {
    if (indexed.has(chapter.number)) duplicateIndexNumbers.add(chapter.number);
    indexed.add(chapter.number);
  }
  const duplicateChapterNumbers = new Set<number>();
  const unindexedFiles: string[] = [];
  for (const file of [...files].sort()) {
    if (!file.endsWith(".md")) continue;
    const match = /^(\d+)_.*\.md$/u.exec(file);
    const chapterNumber = match ? Number(match[1]) : undefined;
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
  if (Object.values(details).some(items => items.length > 0)) throw new ChapterExportSourceError(details);
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
  const sanitizeName = (raw: string): string => raw.replace(/[/\\:*?"<>|]/g, "-");
  const dir = volumeId
    ? (() => {
      const volume = volumes.find((candidate) => candidate.id === volumeId);
      return volume ? `${String(volume.order).padStart(2, "0")}_${sanitizeName(volume.title)}` : "00_ungrouped";
    })()
    : "00_ungrouped";
  return `${dir}/${String(chapterNumber).padStart(4, "0")}_${sanitizeName(chapterTitle)}.md`;
}

export async function buildExportArtifact(
  state: ExportStateLike,
  bookId: string,
  options: {
    readonly format?: "txt" | "md" | "epub" | "md-zip";
    readonly outputPath?: string;
    /** R50/1645 号：章节勾选子集（Scrivener Compile 语义；缺省=全部）。 */
    readonly chapterFilter?: ReadonlyArray<number>;
  },
): Promise<ExportArtifact> {
  const format = options.format ?? "txt";
  const filter = options.chapterFilter;
  const {book, chapters: allChapters, delivery, sourceDigest, totalWords: allWords} = await readBookExportSource(state, bookId);
  const chapters = filter ? allChapters.filter((chapter) => filter.includes(chapter.number)) : allChapters;
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
    readonly format?: "txt" | "md" | "epub" | "md-zip";
    readonly outputPath?: string;
    readonly chapterFilter?: ReadonlyArray<number>;
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
