import type { HashRoute } from "@/hooks/use-hash-route";
import { tr } from "./app-language.js";

/**
 * Cmd+P 快速打开（P2-5）：内容层索引与过滤的纯函数。
 * 数据聚合在组件层（App 已有 /books；面板打开时补拉影游与各书章节索引并缓存）。
 */

export type QuickOpenKind = "book" | "chapter" | "film" | "session" | "match";

export interface QuickOpenItem {
  /** 稳定 id：`book:{bookId}` / `chapter:{bookId}:{n}` / `film:{projectId}` / `session:{sessionId}` / `match:{bookId}:{n}`。 */
  id: string;
  kind: QuickOpenKind;
  label: string;
  /** 辅助行：章节条目显示所属书名等。 */
  detail?: string;
  /** 额外参与过滤的文本（章节号等）。 */
  searchable?: string;
  route: HashRoute;
}

export interface QuickOpenBook {
  readonly id: string;
  readonly title: string;
}

export interface QuickOpenFilm {
  readonly projectId: string;
  readonly title: string;
}

export interface QuickOpenSession {
  readonly sessionId: string;
  readonly title: string;
  /** null = 项目级会话（跳 chat 页）。 */
  readonly bookId: string | null;
}

export interface QuickOpenChapterMeta {
  readonly number: number;
  readonly title?: string;
}

export interface QuickOpenInput {
  readonly books: ReadonlyArray<QuickOpenBook>;
  readonly films: ReadonlyArray<QuickOpenFilm>;
  readonly sessions: ReadonlyArray<QuickOpenSession>;
  /** bookId → 章节索引（面板打开时按需拉取的部分视图）。 */
  readonly chaptersByBook: Readonly<Record<string, ReadonlyArray<QuickOpenChapterMeta>>>;
}

/** 大书（500+ 章）场景下的渲染截断；过滤仍全集参与，只是列表截断。 */
export const QUICK_OPEN_RENDER_LIMIT = 50;

export function buildQuickOpenItems(input: QuickOpenInput): QuickOpenItem[] {
  const books: QuickOpenItem[] = input.books.map((book) => ({
    id: `book:${book.id}`,
    kind: "book",
    label: book.title,
    route: { page: "book", bookId: book.id },
  }));

  const chapters: QuickOpenItem[] = input.books.flatMap((book) =>
    (input.chaptersByBook[book.id] ?? []).map((chapter) => ({
      id: `chapter:${book.id}:${chapter.number}`,
      kind: "chapter" as const,
      label: chapter.title?.trim() ? chapter.title : tr(`第 ${chapter.number} 章`, `Chapter ${chapter.number}`),
      detail: book.title,
      searchable: String(chapter.number),
      route: { page: "chapter", bookId: book.id, chapterNumber: chapter.number },
    })),
  );

  const films: QuickOpenItem[] = input.films.map((film) => ({
    id: `film:${film.projectId}`,
    kind: "film",
    label: film.title,
    route: { page: "film-studio", projectId: film.projectId },
  }));

  const sessions: QuickOpenItem[] = input.sessions.map((session) => ({
    id: `session:${session.sessionId}`,
    kind: "session",
    label: session.title,
    route: session.bookId
      ? { page: "book", bookId: session.bookId }
      : { page: "chat" },
  }));

  return [...books, ...chapters, ...films, ...sessions];
}

/**
 * R47/1640 号：⌘P 正文命中档条目（纯函数）。
 * 输入 = /books/:id/search?scope=chapters 的命中投影；detail 承载高亮片段
 * （filterQuickOpenItems 参与 AND 过滤——命中片段含查询词，天然通过）。
 */
export interface QuickOpenSearchHit {
  readonly chapterNumber: number;
  readonly title: string;
  readonly snippet: string;
}

export function buildMatchQuickOpenItems(
  bookId: string,
  hits: ReadonlyArray<QuickOpenSearchHit>,
): QuickOpenItem[] {
  return hits.map((hit) => ({
    id: `match:${bookId}:${hit.chapterNumber}`,
    kind: "match" as const,
    label: hit.title,
    detail: hit.snippet,
    searchable: String(hit.chapterNumber),
    route: { page: "chapter", bookId, chapterNumber: hit.chapterNumber },
  }));
}

/** 正文命中档请求参数（组件层防抖消费；≥2 字符才触发全文检索）。 */
export const QUICK_OPEN_MATCH_MIN_QUERY = 2;
export const QUICK_OPEN_MATCH_LIMIT = 8;
export const QUICK_OPEN_MATCH_DEBOUNCE_MS = 250;

/**
 * 过滤（纯函数）：空查询返回前 RENDER_LIMIT 条；非空按空格分词 AND 匹配
 * label/detail/searchable（不区分大小写）——「山河 3」= 书名含"山河"且含"3"。
 */
export function filterQuickOpenItems(
  items: ReadonlyArray<QuickOpenItem>,
  query: string,
): QuickOpenItem[] {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) {
    return items.slice(0, QUICK_OPEN_RENDER_LIMIT);
  }
  return items
    .filter((item) => {
      const haystack = [item.label, item.detail ?? "", item.searchable ?? ""]
        .join("\n")
        .toLowerCase();
      return terms.every((term) => haystack.includes(term));
    })
    .slice(0, QUICK_OPEN_RENDER_LIMIT);
}

/** 章节索引的模块级缓存：面板打开时按书拉取，30s 内复用（写入新章后重开面板即刷新）。 */
const CHAPTER_CACHE_TTL_MS = 30_000;
const chapterCache = new Map<string, { fetchedAt: number; chapters: ReadonlyArray<QuickOpenChapterMeta> }>();

export function isChapterCacheFresh(bookId: string, now: number): boolean {
  const entry = chapterCache.get(bookId);
  return Boolean(entry && now - entry.fetchedAt < CHAPTER_CACHE_TTL_MS);
}

export function cacheChapters(bookId: string, chapters: ReadonlyArray<QuickOpenChapterMeta>, now: number): void {
  chapterCache.set(bookId, { fetchedAt: now, chapters });
}

export function readCachedChapters(bookId: string): ReadonlyArray<QuickOpenChapterMeta> {
  return chapterCache.get(bookId)?.chapters ?? [];
}
