import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface RosterCandidateCard {
  readonly candidate: string;
  readonly action: "new-entity" | "alias" | "typo";
  readonly targetName?: string;
  readonly confidence: "high" | "medium" | "low";
}

interface Payload {
  readonly cards: ReadonlyArray<RosterCandidateCard>;
  readonly currentChapter: number;
}

/**
 * G7b/343 号：新专名确认卡（书籍详情挂载）。
 * 拉取章摘要 characters 与名册的比对结果，逐条三选（新实体/别名/笔误），
 * 确认后写回 story/entity_roster.md 并刷新。无候选时整卡隐藏。
 */
export function RosterCandidatesCard({ bookId }: { bookId: string }) {
  const [cards, setCards] = useState<ReadonlyArray<RosterCandidateCard>>([]);
  const [currentChapter, setCurrentChapter] = useState(0);
  const [loading, setLoading] = useState(true);
  const [busyCandidate, setBusyCandidate] = useState("");

  const load = async () => {
    setLoading(true);
    try {
      const data = await fetchJson<Payload>(
        `/books/${encodeURIComponent(bookId)}/roster-candidates`,
      );
      setCards(data.cards ?? []);
      setCurrentChapter(data.currentChapter ?? 0);
    } catch {
      setCards([]);
    }
    setLoading(false);
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bookId]);

  const confirm = async (card: RosterCandidateCard, action: "new-entity" | "alias" | "typo") => {
    setBusyCandidate(card.candidate);
    try {
      await fetchJson(`/books/${encodeURIComponent(bookId)}/roster/confirm`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          candidate: card.candidate,
          action,
          ...(action !== "new-entity" && card.targetName ? { targetName: card.targetName } : {}),
          chapter: currentChapter > 0 ? currentChapter - 1 : undefined,
        }),
      });
      setCards((previous) => previous.filter((entry) => entry.candidate !== card.candidate));
    } catch {
      // 失败保留卡片，下次刷新重试。
    }
    setBusyCandidate("");
  };

  if (loading) {
    return (
      <div className="text-xs text-muted-foreground">
        {tr("加载名册候选…", "Loading roster candidates…")}
      </div>
    );
  }
  if (cards.length === 0) return null;

  return (
    <div className="rounded-lg border border-amber-500/40 bg-amber-500/5 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("新专名确认卡", "Proper-noun confirmation")
          .replace("卡", "")}
      </h3>
      <ul className="space-y-2">
        {cards.map((card) => (
          <li key={card.candidate} className="flex flex-wrap items-center gap-2 text-xs">
            <span className="font-medium text-foreground">{card.candidate}</span>
            <span className="text-muted-foreground">
              {card.action === "typo" && card.targetName
                ? tr("疑似笔误 →", "typo of") + " " + card.targetName
                : card.action === "alias" && card.targetName
                  ? tr("可能是别名 →", "alias of") + " " + card.targetName
                  : tr("未登记", "unregistered")}
            </span>
            <span className="ml-auto flex items-center gap-1.5">
              <button
                onClick={() => confirm(card, "new-entity")}
                disabled={busyCandidate === card.candidate}
                className="px-2 py-1 rounded-md bg-emerald-500/10 text-emerald-600 hover:bg-emerald-500/20 disabled:opacity-40"
              >
                {tr("新实体", "New entity")}
              </button>
              {card.action !== "new-entity" && card.targetName && (
                <button
                  onClick={() => confirm(card, "alias")}
                  disabled={busyCandidate === card.candidate}
                  className="px-2 py-1 rounded-md bg-sky-500/10 text-sky-600 hover:bg-sky-500/20 disabled:opacity-40"
                >
                  {tr("记为别名", "Alias")}
                </button>
              )}
              <button
                onClick={() => confirm(card, "typo")}
                disabled={busyCandidate === card.candidate}
                className="px-2 py-1 rounded-md bg-muted text-muted-foreground hover:bg-muted/60 disabled:opacity-40"
              >
                {tr("忽略", "Ignore")}
              </button>
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
