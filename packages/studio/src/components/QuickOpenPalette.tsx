import { useEffect, useRef, useMemo, useState } from "react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { tr } from "@/lib/app-language";
import { fetchJson } from "@/hooks/use-api";
import {
  buildMatchQuickOpenItems,
  buildQuickOpenItems,
  cacheChapters,
  filterQuickOpenItems,
  isChapterCacheFresh,
  QUICK_OPEN_MATCH_DEBOUNCE_MS,
  QUICK_OPEN_MATCH_LIMIT,
  QUICK_OPEN_MATCH_MIN_QUERY,
  readCachedChapters,
} from "@/lib/quick-open";
import type { QuickOpenChapterMeta, QuickOpenFilm, QuickOpenItem, QuickOpenSession } from "@/lib/quick-open";
import type { HashRoute } from "@/hooks/use-hash-route";
import { setChapterFocus } from "@/lib/chapter-focus";
import { BookOpen, FileText, Film, MessageSquare, Search } from "lucide-react";

const KIND_ICONS = {
  book: BookOpen,
  chapter: FileText,
  film: Film,
  session: MessageSquare,
  match: Search,
} as const;

const KIND_LABELS = {
  book: () => tr("书籍", "Books"),
  chapter: () => tr("章节", "Chapters"),
  film: () => tr("互动影游", "Interactive Films"),
  session: () => tr("会话", "Sessions"),
  match: () => tr("正文命中", "Text matches"),
} as const;

/**
 * Cmd+P 快速打开（P2-5）：与 ⌘K 命令面板分离的另一 CommandDialog 实例
 * （参照 VSCode Cmd+P / Cmd+Shift+P 分离）。索引 = 书 + 各书章节 + 影游 + 会话；
 * 章节按书惰性拉取（/books/:id 一次含全部章节索引，30s 模块缓存）。
 */
export function QuickOpenPalette({ open, onOpenChange, books, sessions, activeBookId, onNavigate }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  books: ReadonlyArray<{ id: string; title: string }>;
  sessions: ReadonlyArray<QuickOpenSession>;
  /** R47/1640 号：当前活跃书（正文命中档只搜当前书，防全书并行扫描）。 */
  activeBookId?: string | null;
  onNavigate: (route: HashRoute) => void;
}) {
  // 734 号：弹层焦点归还（732 号模式复用，WCAG 2.4.3）——打开快照触发前
  // 焦点（onOpenChange 回调时机防 autofocus 污染），关闭 restore。
  const lastFocusedRef = useRef<HTMLElement | null>(null);
  const handleOpenChange = (next: boolean) => {
    if (next) {
      lastFocusedRef.current = document.activeElement as HTMLElement | null;
    } else if (lastFocusedRef.current) {
      lastFocusedRef.current.focus?.();
      lastFocusedRef.current = null;
    }
    onOpenChange(next);
  };
  const [query, setQuery] = useState("");
  const [films, setFilms] = useState<ReadonlyArray<QuickOpenFilm>>([]);
  const [chaptersByBook, setChaptersByBook] = useState<Record<string, ReadonlyArray<QuickOpenChapterMeta>>>({});
  // R47/1640 号：正文命中档——activeBookId 存在且查询 ≥2 字符时防抖检索。
  const [matchItems, setMatchItems] = useState<ReadonlyArray<QuickOpenItem>>([]);

  useEffect(() => {
    if (!open) {
      setQuery("");
      setMatchItems([]);
      return;
    }
    let cancelled = false;
    // 影游项目：面板打开时拉一次（列表小，无缓存必要）
    void fetchJson<{ films: ReadonlyArray<QuickOpenFilm> }>("/interactive-films")
      .then((data) => {
        if (!cancelled) setFilms(data.films ?? []);
      })
      .catch(() => {
        if (!cancelled) setFilms([]);
      });
    // 章节索引：每书一次请求，30s 缓存内跳过
    for (const book of books) {
      if (isChapterCacheFresh(book.id, Date.now())) continue;
      void fetchJson<{ chapters: ReadonlyArray<QuickOpenChapterMeta> }>(`/books/${book.id}`)
        .then((data) => {
          if (cancelled) return;
          cacheChapters(book.id, data.chapters ?? [], Date.now());
          setChaptersByBook((prev) => ({ ...prev, [book.id]: data.chapters ?? [] }));
        })
        .catch(() => {
          if (!cancelled) cacheChapters(book.id, [], Date.now());
        });
    }
    return () => {
      cancelled = true;
    };
  }, [open, books]);

  // R47/1640 号：正文命中档（防抖 + latest-wins 竞态守卫；失败静默降级为空档）。
  useEffect(() => {
    const trimmed = query.trim();
    if (!open || !activeBookId || trimmed.length < QUICK_OPEN_MATCH_MIN_QUERY) {
      setMatchItems([]);
      return;
    }
    const timer = window.setTimeout(() => {
      void fetchJson<{ hits: ReadonlyArray<{ chapterNumber?: number; title?: string; snippet?: string }> }>(
        `/books/${encodeURIComponent(activeBookId)}/search?q=${encodeURIComponent(trimmed)}&scope=chapters&limit=${QUICK_OPEN_MATCH_LIMIT}`,
      )
        .then((data) => {
          setMatchItems(buildMatchQuickOpenItems(
            activeBookId,
            (data.hits ?? []).flatMap((hit) => (
              hit.chapterNumber != null
                ? [{ chapterNumber: hit.chapterNumber, title: hit.title ?? `#${hit.chapterNumber}`, snippet: hit.snippet ?? "" }]
                : []
            )),
          ));
        })
        .catch(() => {
          setMatchItems([]);
        });
    }, QUICK_OPEN_MATCH_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [open, activeBookId, query]);

  const items = useMemo(
    () => buildQuickOpenItems({
      books,
      films,
      sessions,
      chaptersByBook: books.length
        ? Object.fromEntries(books.map((book) => [book.id, chaptersByBook[book.id] ?? readCachedChapters(book.id)]))
        : {},
    }),
    [books, films, sessions, chaptersByBook],
  );

  const filtered = useMemo(() => filterQuickOpenItems(items, query), [items, query]);
  // R47/1640 号：正文命中档置顶（搜索直出，跳 filterQuickOpenItems 的标题域）。
  const grouped = useMemo(() => {
    const groups = new Map<QuickOpenItem["kind"], QuickOpenItem[]>();
    for (const item of [...matchItems, ...filtered]) {
      const bucket = groups.get(item.kind);
      if (bucket) bucket.push(item);
      else groups.set(item.kind, [item]);
    }
    return [...groups.entries()];
  }, [matchItems, filtered]);

  const run = (item: QuickOpenItem) => {
    // R47/1640 号：正文命中跳章并滚动到命中行（一次性焦点传递）。
    if (item.kind === "match" && item.route.page === "chapter") {
      setChapterFocus(item.route.bookId, item.route.chapterNumber, query.trim());
    }
    onOpenChange(false);
    onNavigate(item.route);
  };

  return (
    <CommandDialog
      open={open}
      onOpenChange={handleOpenChange}
      title={tr("快速打开", "Quick Open")}
      description={tr("跳转到书、章节、影游或会话…", "Jump to a book, chapter, film or session…")}
    >
      <Command shouldFilter={false}>
        <CommandInput
          value={query}
          onValueChange={setQuery}
          placeholder={tr("跳转到…", "Jump to…")}
        />
        <CommandList>
          {/* R49 后审查修复（1646 号）：正文命中档置顶不受标题索引空态遮蔽
              ——「灵气」在标题无匹配但正文有命中时，CommandEmpty 会独占渲染
              吞掉 match 组（IAB 走查实证）。 */}
          {filtered.length === 0 && matchItems.length === 0 ? (
            <CommandEmpty>{tr("没有匹配的内容", "No matching content")}</CommandEmpty>
          ) : (
            grouped.map(([kind, entries]) => (
              <CommandGroup key={kind} heading={KIND_LABELS[kind]()}>
                {entries.map((item) => {
                  const Icon = KIND_ICONS[item.kind];
                  return (
                    <CommandItem key={item.id} value={item.id} onSelect={() => run(item)}>
                      <Icon size={14} className="shrink-0 text-muted-foreground" />
                      <span className="truncate">{item.label}</span>
                      {item.detail && (
                        <span className="ml-auto truncate text-xs text-muted-foreground/70">{item.detail}</span>
                      )}
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            ))
          )}
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
