import { useEffect, useMemo, useState } from "react";
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
  buildQuickOpenItems,
  cacheChapters,
  filterQuickOpenItems,
  isChapterCacheFresh,
  readCachedChapters,
} from "@/lib/quick-open";
import type { QuickOpenChapterMeta, QuickOpenFilm, QuickOpenItem, QuickOpenSession } from "@/lib/quick-open";
import type { HashRoute } from "@/hooks/use-hash-route";
import { BookOpen, FileText, Film, MessageSquare } from "lucide-react";

const KIND_ICONS = {
  book: BookOpen,
  chapter: FileText,
  film: Film,
  session: MessageSquare,
} as const;

const KIND_LABELS = {
  book: () => tr("书籍", "Books"),
  chapter: () => tr("章节", "Chapters"),
  film: () => tr("互动影游", "Interactive Films"),
  session: () => tr("会话", "Sessions"),
} as const;

/**
 * Cmd+P 快速打开（P2-5）：与 ⌘K 命令面板分离的另一 CommandDialog 实例
 * （参照 VSCode Cmd+P / Cmd+Shift+P 分离）。索引 = 书 + 各书章节 + 影游 + 会话；
 * 章节按书惰性拉取（/books/:id 一次含全部章节索引，30s 模块缓存）。
 */
export function QuickOpenPalette({ open, onOpenChange, books, sessions, onNavigate }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  books: ReadonlyArray<{ id: string; title: string }>;
  sessions: ReadonlyArray<QuickOpenSession>;
  onNavigate: (route: HashRoute) => void;
}) {
  const [query, setQuery] = useState("");
  const [films, setFilms] = useState<ReadonlyArray<QuickOpenFilm>>([]);
  const [chaptersByBook, setChaptersByBook] = useState<Record<string, ReadonlyArray<QuickOpenChapterMeta>>>({});

  useEffect(() => {
    if (!open) {
      setQuery("");
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
  const grouped = useMemo(() => {
    const groups = new Map<QuickOpenItem["kind"], QuickOpenItem[]>();
    for (const item of filtered) {
      const bucket = groups.get(item.kind);
      if (bucket) bucket.push(item);
      else groups.set(item.kind, [item]);
    }
    return [...groups.entries()];
  }, [filtered]);

  const run = (item: QuickOpenItem) => {
    onOpenChange(false);
    onNavigate(item.route);
  };

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
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
          {filtered.length === 0 ? (
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
