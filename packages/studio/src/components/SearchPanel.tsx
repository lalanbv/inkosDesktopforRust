import { useEffect, useRef, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { useI18n } from "../hooks/use-i18n";
import { setChapterFocus } from "../lib/chapter-focus";

/** 挂载点 nav 子集（BookDetail 局部 Nav 形态对齐——本面板只需两跳）。 */
interface SearchPanelNav {
  toChapter: (bookId: string, num: number) => void;
  toTruth: (bookId: string) => void;
}

interface SearchHit {
  readonly kind: "chapter" | "entity" | "hook" | "truth";
  readonly id: string;
  readonly title: string;
  readonly source: string;
  readonly score: number;
  readonly snippet: string;
  readonly matchStart: number;
  readonly matchLength: number;
  readonly chapterNumber?: number;
}

type SearchScope = "chapters" | "entity" | "hook" | "truth";

type ScopeKey = "search.scope.chapters" | "search.scope.entity" | "search.scope.hook" | "search.scope.truth";

const SCOPES: ReadonlyArray<{ id: SearchScope; i18nKey: ScopeKey }> = [
  { id: "chapters", i18nKey: "search.scope.chapters" },
  { id: "entity", i18nKey: "search.scope.entity" },
  { id: "hook", i18nKey: "search.scope.hook" },
  { id: "truth", i18nKey: "search.scope.truth" },
];

/** 高亮片段渲染：matchStart/Length 定位 <mark>（省略号随片段携带）。 */
function Snippet({ snippet, matchStart, matchLength }: {
  snippet: string;
  matchStart: number;
  matchLength: number;
}) {
  return (
    <p className="text-xs text-muted-foreground line-clamp-2 break-all">
      {snippet.slice(0, matchStart)}
      <mark className="bg-primary/20 text-foreground rounded-sm px-0.5">
        {snippet.slice(matchStart, matchStart + matchLength)}
      </mark>
      {snippet.slice(matchStart + matchLength)}
    </p>
  );
}

/**
 * R47/1640 号：全局搜索面板（书籍详情挂载）。
 * GET /books/:id/search?q=&scope= 四轴（chapters/entity/hook/truth）；
 * chapters 命中点击跳章并滚动到命中行（chapter-focus 一次性传递），
 * truth 命中跳真相页；输入 250ms 防抖 + latest-wins 竞态守卫。
 */
export function SearchPanel({ bookId, nav }: { bookId: string; nav: SearchPanelNav }) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<SearchScope>("chapters");
  const [hits, setHits] = useState<ReadonlyArray<SearchHit>>([]);
  const [searching, setSearching] = useState(false);
  const [notice, setNotice] = useState("");
  // 请求序号守卫：防抖窗口外的过期响应丢弃（latest-wins）。
  const requestRef = useRef(0);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      requestRef.current += 1;
      setHits([]);
      setSearching(false);
      setNotice("");
      return;
    }
    const requestId = requestRef.current + 1;
    requestRef.current = requestId;
    setSearching(true);
    const timer = window.setTimeout(() => {
      void fetchJson<{ query: string; scope: string; hits: ReadonlyArray<SearchHit> }>(
        `/books/${encodeURIComponent(bookId)}/search?q=${encodeURIComponent(trimmed)}&scope=${scope}&limit=20`,
      )
        .then((data) => {
          if (requestRef.current !== requestId) return;
          setHits(data.hits ?? []);
          setSearching(false);
          setNotice("");
        })
        .catch(() => {
          if (requestRef.current !== requestId) return;
          setHits([]);
          setSearching(false);
          setNotice(t("search.failed"));
        });
    }, 250);
    return () => window.clearTimeout(timer);
    // t 仅文案投影（语言切换面可接受陈旧一帧）——入 deps 会因 hook 每渲染
    // 新引用导致 effect 重触发循环。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bookId, query, scope]);

  const openHit = (hit: SearchHit) => {
    if (hit.kind === "chapter" && hit.chapterNumber != null) {
      setChapterFocus(bookId, hit.chapterNumber, query.trim());
      nav.toChapter(bookId, hit.chapterNumber);
    } else if (hit.kind === "truth") {
      nav.toTruth(bookId);
    }
    // entity/hook 命中：两者都挂书籍详情本页（CodexPanel/伏笔账本），无需跳转。
  };

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {t("search.title")}
      </h3>
      <input
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={t("search.placeholder")}
        className="w-full rounded-lg border border-border/50 bg-background px-3 py-1.5 text-sm focus:outline-none focus:border-primary/40"
      />
      <div className="flex gap-1" role="tablist" aria-label={t("search.scopeAria")}>
        {SCOPES.map((entry) => (
          <button
            key={entry.id}
            role="tab"
            aria-selected={scope === entry.id}
            onClick={() => setScope(entry.id)}
            className={`rounded-full px-2.5 py-0.5 text-xs transition-colors ${
              scope === entry.id
                ? "bg-primary text-primary-foreground"
                : "bg-secondary text-muted-foreground hover:text-foreground"
            }`}
          >
            {t(entry.i18nKey)}
          </button>
        ))}
      </div>
      {searching && query.trim() ? (
        <p className="text-xs text-muted-foreground italic">{t("search.searching")}</p>
      ) : null}
      {!searching && query.trim() && hits.length === 0 && !notice ? (
        <p className="text-xs text-muted-foreground italic">{t("search.noHits")}</p>
      ) : null}
      {notice ? <p className="text-xs text-destructive">{notice}</p> : null}
      <ul className="space-y-1.5">
        {hits.map((hit) => (
          <li key={hit.id}>
            <button
              onClick={() => openHit(hit)}
              className="w-full text-left rounded-md border border-transparent hover:border-border/60 hover:bg-secondary/40 px-2 py-1.5 transition-colors"
            >
              <span className="flex items-center gap-2">
                <span className="text-xs font-bold truncate">{hit.title}</span>
                <span className="text-[10px] uppercase text-muted-foreground/70 shrink-0">
                  {hit.kind === "chapter" && hit.chapterNumber != null ? `#${hit.chapterNumber}` : hit.kind}
                </span>
              </span>
              <Snippet snippet={hit.snippet} matchStart={hit.matchStart} matchLength={hit.matchLength} />
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
