import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface LensEntry {
  readonly order: number;
  readonly source: string;
  readonly tier: string;
  readonly tierPrecedence: number;
  readonly protected: boolean;
  readonly tokens: number;
  readonly compiled: boolean;
  readonly rank: { readonly recency?: number; readonly frequency?: number; readonly hookBonus?: number } | null;
}

interface ContextLens {
  readonly version: number;
  readonly chapter: number;
  readonly entries: ReadonlyArray<LensEntry>;
  readonly compression: {
    readonly compiledSource: string;
    readonly budgetTokens: number;
    readonly protectedTokens: number;
    readonly compressibleTokens: number;
    readonly preCompressionSources: ReadonlyArray<{ readonly source: string; readonly tokens: number }>;
  } | null;
  readonly notes: ReadonlyArray<string>;
  readonly totals: {
    readonly entries: number;
    readonly protectedEntries: number;
    readonly compiledEntries: number;
    readonly tokens: number;
  };
}

/** 层级中英标签（tier id → 展示名；与 G2 契约七层一致）。 */
function tierLabel(tier: string): string {
  const zh: Record<string, string> = {
    "book-fact": "本书事实",
    "book-planning": "本书规划",
    "book-memory": "本书时序记忆",
    "user-reference": "参考资料",
    deconstruction: "拆书结论",
    "style-asset": "写法资产",
    ephemeral: "临时/未注册",
  };
  const en: Record<string, string> = {
    "book-fact": "Book facts",
    "book-planning": "Book planning",
    "book-memory": "Book memory",
    "user-reference": "User references",
    deconstruction: "Deconstruction",
    "style-asset": "Style assets",
    ephemeral: "Ephemeral",
  };
  return tr(zh[tier] ?? tier, en[tier] ?? tier);
}

/**
 * R21/391 号：Context Lens——上下文装配透明面板（书籍详情挂载，纯读）。
 * 回放每章治理装配：实际进入 prompt 的条目（层级/保护/token/排序特征）
 * + 压缩留痕（被编译的原始来源与压缩前 token）。让作者看见"模型看见了什么"。
 */
export function ContextLensPanel({ bookId }: { bookId: string }) {
  const [chapters, setChapters] = useState<ReadonlyArray<number>>([]);
  const [chapter, setChapter] = useState<number>(0);
  const [lens, setLens] = useState<ContextLens | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      setLens(null);
      setError("");
      try {
        const data = await fetchJson<{ chapters: ReadonlyArray<number> }>(
          `/books/${encodeURIComponent(bookId)}/context-lens`,
        );
        if (cancelled) return;
        setChapters(data.chapters);
        setChapter(data.chapters.length > 0 ? data.chapters[data.chapters.length - 1]! : 0);
      } catch {
        if (!cancelled) setChapters([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bookId]);

  useEffect(() => {
    if (!chapter) return;
    let cancelled = false;
    void (async () => {
      try {
        const data = await fetchJson<ContextLens>(
          `/books/${encodeURIComponent(bookId)}/context-lens/${chapter}`,
        );
        if (!cancelled) {
          setLens(data);
          setError("");
        }
      } catch (cause) {
        if (!cancelled) {
          setLens(null);
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bookId, chapter]);

  if (chapters.length === 0) {
    return (
      <div className="rounded-lg border border-border/60 p-4 space-y-2">
        <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
          {tr("上下文透视", "Context lens")}
        </h3>
        <p className="text-xs text-muted-foreground italic">
          {tr(
            "暂无装配留痕——写一章后可回放该章上下文装配清单。",
            "No assembly traces yet — write a chapter to replay its context assembly.",
          )}
        </p>
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
          {tr("上下文透视", "Context lens")}
        </h3>
        <select
          value={chapter}
          onChange={(event) => setChapter(Number(event.target.value))}
          className="text-xs border border-border rounded-md px-2 py-1 bg-background"
        >
          {chapters.map((value) => (
            <option key={value} value={value}>
              {tr(`第 ${value} 章`, `Chapter ${value}`)}
            </option>
          ))}
        </select>
      </div>
      {error && <p className="text-[11px] text-red-600">{error}</p>}
      {lens && (
        <div className="space-y-2">
          <p className="text-[11px] text-muted-foreground">
            {tr(
              `${lens.totals.entries} 条来源 · 受保护 ${lens.totals.protectedEntries} · 约 ${lens.totals.tokens} tokens`,
              `${lens.totals.entries} sources · ${lens.totals.protectedEntries} protected · ~${lens.totals.tokens} tokens`,
            )}
            {lens.compression
              ? tr(
                  ` · 预算 ${lens.compression.budgetTokens}（已编译压缩）`,
                  ` · budget ${lens.compression.budgetTokens} (compiled)`,
                )
              : tr(" · 预算内未压缩", " · within budget, uncompressed")}
          </p>
          <div className="space-y-1">
            {lens.entries.map((entry) => (
              <div
                key={`${entry.order}-${entry.source}`}
                className="flex items-center gap-2 text-[11px] border border-border/40 rounded-md px-2 py-1"
              >
                <span className="text-muted-foreground w-5 shrink-0 text-right">{entry.order}.</span>
                <span className="font-mono break-all flex-1">{entry.source}</span>
                <span className="text-muted-foreground shrink-0">{tierLabel(entry.tier)}</span>
                {entry.protected && (
                  <span className="shrink-0 px-1 rounded bg-emerald-600/15 text-emerald-700">
                    {tr("保护", "protected")}
                  </span>
                )}
                {entry.compiled && (
                  <span className="shrink-0 px-1 rounded bg-amber-600/15 text-amber-700">
                    {tr("编译产物", "compiled")}
                  </span>
                )}
                <span className="text-muted-foreground shrink-0 w-12 text-right">~{entry.tokens}</span>
              </div>
            ))}
          </div>
          {lens.compression && (
            <div className="text-[11px] border border-border/40 rounded-md px-2 py-1.5 space-y-0.5">
              <p className="font-bold text-amber-700">
                {tr(
                  `压缩留痕：${lens.compression.preCompressionSources.length} 条可压缩来源被编译为单条摘要`,
                  `Compression trail: ${lens.compression.preCompressionSources.length} compressible sources compiled into one summary`,
                )}
              </p>
              {lens.compression.preCompressionSources.map((item) => (
                <p key={item.source} className="text-muted-foreground">
                  {item.source}（~{item.tokens}）
                </p>
              ))}
            </div>
          )}
          {lens.notes.length > 0 && (
            <p className="text-[11px] text-muted-foreground">
              {tr("留痕备注：", "Trace notes: ")}
              {lens.notes.join(tr("；", "; "))}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
