import { useEffect, useState } from "react";
import type { Theme } from "../hooks/use-theme";
import type { TFunction } from "../hooks/use-i18n";
import { useColors } from "../hooks/use-colors";
import { fetchJson } from "../hooks/use-api";
import { buildBookCreatePrefill } from "../lib/radar-prefill";
import { TrendingUp, Loader2, Target, Clock, ListChecks, Lightbulb, BookPlus } from "lucide-react";

interface Recommendation {
  readonly confidence: number;
  readonly platform: string;
  readonly genre: string;
  readonly concept: string;
  readonly reasoning: string;
  readonly benchmarkTitles: ReadonlyArray<string>;
  // G14a/335 号信号卡：拥挤度 + 差异化机会（旧历史记录可缺省）。
  readonly crowding?: "high" | "medium" | "low";
  readonly differentiation?: string;
}

interface RadarResult {
  readonly marketSummary: string;
  readonly recommendations: ReadonlyArray<Recommendation>;
}

interface RankingEntry {
  readonly title: string;
  readonly author: string;
  readonly category: string;
  readonly extra: string;
}

interface PlatformRankings {
  readonly platform: string;
  readonly entries: ReadonlyArray<RankingEntry>;
}

interface RadarHistoryItem {
  readonly file: string;
  readonly timestamp: string;
  readonly summaryPreview: string;
  readonly result: RadarResult;
}

interface Nav { toDashboard: () => void }

interface RadarViewProps {
  nav: Nav;
  theme: Theme;
  t: TFunction;
  /** R27/402 号：雷达→一键开书——App 层负责预填 chat 输入并跳 book-create。 */
  onCreateBook?: (prefill: string) => void;
}

/** 收集榜单里可选的分类（去重，取前 12 防勾选面板爆炸）。 */
function collectCategories(rankings: ReadonlyArray<PlatformRankings>): string[] {
  const seen = new Set<string>();
  for (const source of rankings) {
    for (const entry of source.entries) {
      const category = entry.category?.trim();
      if (category) seen.add(category);
    }
  }
  return [...seen].slice(0, 12);
}

export function RadarView({ nav, theme, t, onCreateBook }: RadarViewProps) {
  const c = useColors(theme);
  // 预填草稿语言跟界面语言（ChatPage 同款判定：中文 key 命中即 zh）。
  const isZh = t("nav.connected") === "\u5DF2\u8FDE\u63A5";
  const [result, setResult] = useState<RadarResult | null>(null);
  const [history, setHistory] = useState<ReadonlyArray<RadarHistoryItem>>([]);
  const [error, setError] = useState("");
  // G14a/335 号：选后再析——免费扫榜 → 勾选范围 → 分析。
  const [rankings, setRankings] = useState<ReadonlyArray<PlatformRankings>>([]);
  const [rankingsLoading, setRankingsLoading] = useState(false);
  const [selectedPlatforms, setSelectedPlatforms] = useState<ReadonlyArray<string>>([]);
  const [selectedCategories, setSelectedCategories] = useState<ReadonlyArray<string>>([]);
  const [analyzing, setAnalyzing] = useState(false);

  const loadHistory = async () => {
    try {
      const data = await fetchJson<{ items: ReadonlyArray<RadarHistoryItem> }>("/radar/history");
      setHistory(data.items ?? []);
    } catch {
      setHistory([]);
    }
  };

  useEffect(() => {
    void loadHistory();
  }, []);

  const toggle = (
    value: string,
    selected: ReadonlyArray<string>,
    setSelected: (next: ReadonlyArray<string>) => void,
  ) => {
    setSelected(
      selected.includes(value) ? selected.filter((x) => x !== value) : [...selected, value],
    );
  };

  const handleScanRankings = async () => {
    setRankingsLoading(true);
    setError("");
    try {
      const data = await fetchJson<{ rankings: ReadonlyArray<PlatformRankings> }>("/radar/rankings", { method: "POST" });
      setRankings(data.rankings ?? []);
      setResult(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setRankingsLoading(false);
  };

  const handleAnalyze = async () => {
    setAnalyzing(true);
    setError("");
    try {
      const data = await fetchJson<RadarResult>("/radar/analyze", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          selection: { platforms: selectedPlatforms, categories: selectedCategories },
        }),
      });
      setResult(data);
      await loadHistory();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setAnalyzing(false);
  };

  const categories = collectCategories(rankings);
  const selectedCount = selectedPlatforms.length + selectedCategories.length;
  const crowdingStyle = (crowding: NonNullable<Recommendation["crowding"]>): string =>
    crowding === "high"
      ? "bg-red-500/10 text-red-600"
      : crowding === "medium"
        ? "bg-amber-500/10 text-amber-600"
        : "bg-emerald-500/10 text-emerald-600";

  return (
    <div className="space-y-8">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <button onClick={nav.toDashboard} className={c.link}>{t("bread.home")}</button>
        <span className="text-border">/</span>
        <span>{t("nav.radar")}</span>
      </div>

      <div className="flex items-center justify-between">
        <h1 className="font-serif text-3xl flex items-center gap-3">
          <TrendingUp size={28} className="text-primary" />
          {t("radar.title")}
        </h1>
        <div className="flex items-center gap-2">
          <button
            onClick={handleScanRankings}
            disabled={rankingsLoading}
            className={`px-5 py-2.5 text-sm rounded-lg ${c.btnPrimary} disabled:opacity-30 flex items-center gap-2`}
          >
            {rankingsLoading ? <Loader2 size={14} className="animate-spin" /> : <ListChecks size={14} />}
            {rankingsLoading ? t("radar.scanning") : t("radar.rankings")}
          </button>
          <button
            onClick={handleAnalyze}
            disabled={analyzing || rankings.length === 0}
            title={t("radar.rankingsFree")}
            className={`px-5 py-2.5 text-sm rounded-lg border border-border hover:bg-muted/30 disabled:opacity-30 flex items-center gap-2`}
          >
            {analyzing ? <Loader2 size={14} className="animate-spin" /> : <Target size={14} />}
            {analyzing ? t("radar.analyzing") : t("radar.analyzeSelected")}
          </button>
        </div>
      </div>

      {error && (
        <div className="bg-destructive/10 text-destructive px-4 py-3 rounded-lg text-sm">{error}</div>
      )}

      {rankings.length > 0 && (
        <div className={`border ${c.cardStatic} rounded-lg p-5 space-y-4`}>
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground flex items-center gap-2">
              <ListChecks size={14} />
              {t("radar.rankings")}
            </h3>
            <span className="text-xs text-muted-foreground">
              {t("radar.selectedHint").replace("{n}", String(selectedCount))}
            </span>
          </div>

          <div className="flex flex-wrap gap-2">
            {rankings.map((source) => {
              const active = selectedPlatforms.includes(source.platform);
              return (
                <button
                  key={source.platform}
                  onClick={() => toggle(source.platform, selectedPlatforms, setSelectedPlatforms)}
                  className={`px-3 py-1.5 text-xs rounded-full border transition-colors ${
                    active ? "bg-primary text-primary-foreground border-primary" : "border-border hover:bg-muted/30"
                  }`}
                >
                  {t("radar.platform")}: {source.platform}
                </button>
              );
            })}
          </div>

          {categories.length > 0 && (
            <div className="flex flex-wrap gap-2">
              {categories.map((category) => {
                const active = selectedCategories.includes(category);
                return (
                  <button
                    key={category}
                    onClick={() => toggle(category, selectedCategories, setSelectedCategories)}
                    className={`px-3 py-1.5 text-xs rounded-full border transition-colors ${
                      active ? "bg-primary text-primary-foreground border-primary" : "border-border hover:bg-muted/30"
                    }`}
                  >
                    {t("radar.category")}: {category}
                  </button>
                );
              })}
            </div>
          )}

          <div className="space-y-3">
            {rankings.map((source) => (
              <div key={source.platform}>
                <div className="text-xs font-bold text-foreground/80 mb-1">{source.platform}</div>
                <div className="text-xs text-muted-foreground leading-relaxed line-clamp-2">
                  {source.entries.slice(0, 10).map((entry) => entry.title).join(" · ")}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {result && (
        <div className="space-y-6">
          <div className={`border ${c.cardStatic} rounded-lg p-5`}>
            <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground mb-3">{t("radar.summary")}</h3>
            <p className="text-sm leading-relaxed whitespace-pre-wrap">{result.marketSummary}</p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {result.recommendations.map((rec, i) => (
              <div key={i} className={`border ${c.cardStatic} rounded-lg p-5 space-y-3`}>
                <div className="flex items-center justify-between">
                  <span className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
                    {rec.platform} · {rec.genre}
                  </span>
                  <div className="flex items-center gap-1.5">
                    {rec.crowding && (
                      <span
                        title={t("radar.crowding")}
                        className={`text-xs font-bold px-2 py-0.5 rounded-full ${crowdingStyle(rec.crowding)}`}
                      >
                        {t(`radar.crowding.${rec.crowding}`)}
                      </span>
                    )}
                    <span className={`text-xs font-bold px-2 py-0.5 rounded-full ${
                      rec.confidence >= 0.7 ? "bg-emerald-500/10 text-emerald-600" :
                      rec.confidence >= 0.4 ? "bg-amber-500/10 text-amber-600" :
                      "bg-muted text-muted-foreground"
                    }`}>
                      {(rec.confidence * 100).toFixed(0)}%
                    </span>
                  </div>
                </div>
                <p className="text-sm font-semibold">{rec.concept}</p>
                <p className="text-xs text-muted-foreground leading-relaxed">{rec.reasoning}</p>
                {rec.differentiation && (
                  <p className="text-xs leading-relaxed flex items-start gap-1.5 text-primary">
                    <Lightbulb size={13} className="mt-0.5 shrink-0" aria-hidden />
                    <span>{t("radar.differentiation")}：{rec.differentiation}</span>
                  </p>
                )}
                {onCreateBook && (
                  <button
                    type="button"
                    onClick={() => onCreateBook(buildBookCreatePrefill(rec, isZh))}
                    title={t("radar.createBookHint")}
                    className="w-full px-3 py-1.5 text-xs rounded-lg border border-border hover:bg-muted/30 flex items-center justify-center gap-1.5 transition-colors"
                  >
                    <BookPlus size={13} />
                    {t("radar.createBook")}
                  </button>
                )}
                {rec.benchmarkTitles.length > 0 && (
                  <div className="flex gap-2 flex-wrap">
                    {rec.benchmarkTitles.map((bt) => (
                      <span key={bt} className="px-2 py-0.5 text-[10px] bg-secondary rounded">{bt}</span>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {history.length > 0 && (
        <div className={`border ${c.cardStatic} rounded-lg p-5 space-y-3`}>
          <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground flex items-center gap-2">
            <Clock size={14} />
            {t("radar.history")}
          </h3>
          <div className="space-y-2">
            {history.slice(0, 10).map((item) => (
              <button
                key={item.file}
                onClick={() => setResult(item.result)}
                className="w-full rounded-md border border-border/40 px-3 py-2 text-left text-xs hover:bg-muted/30"
              >
                <div className="font-medium text-foreground">{new Date(item.timestamp).toLocaleString()}</div>
                <div className="mt-1 line-clamp-2 text-muted-foreground">{item.summaryPreview || item.file}</div>
              </button>
            ))}
          </div>
        </div>
      )}

      {!result && rankings.length === 0 && !rankingsLoading && !error && (
        <div className={`border border-dashed ${c.cardStatic} rounded-lg p-12 text-center text-muted-foreground text-sm italic`}>
          {t("radar.emptyHint")}
        </div>
      )}
    </div>
  );
}
