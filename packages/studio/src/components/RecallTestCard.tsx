import { useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface FusedHit {
  readonly id: string;
  readonly score: number;
  readonly sources: ReadonlyArray<"semantic" | "fts5">;
}

interface Payload {
  readonly fts: ReadonlyArray<{ id: string; score: number; source: string; title: string }>;
  readonly fused: ReadonlyArray<FusedHit>;
  readonly mode: "semantic" | "fts5-fallback";
}

/**
 * G1/349 号：召回测试面板（书籍详情挂载）。
 * 输入查询 → POST hybrid-search（FTS5 + 可选向量 RRF 融合）→
 * 展示融合排名与检索模式（semantic / fts5-fallback）。
 */
export function RecallTestCard({ bookId }: { bookId: string }) {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<Payload | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async () => {
    if (!query.trim()) return;
    setBusy(true);
    try {
      const data = await fetchJson<Payload>(`/books/${encodeURIComponent(bookId)}/hybrid-search`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query, k: 10 }),
      });
      setResult(data);
    } catch {
      setResult(null);
    }
    setBusy(false);
  };

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("召回测试", "Recall test")}
      </h3>
      <div className="flex items-center gap-2">
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void run();
          }}
          placeholder={tr("输入测试查询…", "Type a test query…")}
          className="flex-1 text-xs border border-border rounded-md px-2 py-1.5 bg-background"
        />
        <button
          onClick={run}
          disabled={busy || !query.trim()}
          className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-30"
        >
          {busy ? tr("检索中…", "Searching…") : tr("检索", "Search")}
        </button>
      </div>
      {result && (
        <div className="space-y-1">
          <div className="text-[11px] text-muted-foreground">
            {tr("模式", "Mode")}:{" "}
            <span
              className={
                result.mode === "semantic"
                  ? "text-emerald-600 font-bold"
                  : "text-muted-foreground font-bold"
              }
            >
              {result.mode === "semantic" ? tr("语义+词法混合", "semantic + lexical") : tr("词法降级", "lexical fallback")}
            </span>
          </div>
          {result.fused.length === 0 ? (
            <div className="text-xs text-muted-foreground italic">{tr("无命中", "No hits")}</div>
          ) : (
            <ul className="space-y-1">
              {result.fused.slice(0, 10).map((hit) => (
                <li key={hit.id} className="flex items-center gap-2 text-xs">
                  <span className="font-mono text-muted-foreground">{hit.id}</span>
                  <span className="text-muted-foreground/70">
                    {hit.score.toFixed(4)}
                    {hit.sources.map((s) => (s === "semantic" ? " ·语义" : " ·词法")).join("")}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
