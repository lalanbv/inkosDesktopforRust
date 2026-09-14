import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface CodexCard {
  readonly name: string;
  readonly aliases: ReadonlyArray<string>;
  readonly kind: string;
  readonly summary: string;
  readonly facts: ReadonlyArray<string>;
  readonly relationships: ReadonlyArray<{ target: string; note: string }>;
}

/**
 * R10/377 号：实体卡编辑面板（书籍详情挂载）。
 * GET /codex（名册确认自动派生合并）→ 选中卡片编辑 summary/正典事实 →
 * PUT 全量保存（governance 同款全量表单模式）。
 */
export function CodexPanel({ bookId }: { bookId: string }) {
  const [cards, setCards] = useState<ReadonlyArray<CodexCard>>([]);
  const [selected, setSelected] = useState<string>("");
  const [summary, setSummary] = useState("");
  const [facts, setFacts] = useState("");
  const [notice, setNotice] = useState("");

  const load = async () => {
    try {
      const data = await fetchJson<{ cards: ReadonlyArray<CodexCard> }>(
        `/books/${encodeURIComponent(bookId)}/codex`,
      );
      setCards(data.cards);
      const next = data.cards.some((card) => card.name === selected)
        ? selected
        : (data.cards[0]?.name ?? "");
      if (next) setSelected(next);
      // 452 号：自动选中必须同步回填编辑字段（442 号 SeriesCanon 同款静默
      // 清空防御）——字段留空而卡片处于选中态时，保存会以空值覆盖原卡。
      const currentCard = data.cards.find((card) => card.name === next);
      setSummary(currentCard?.summary ?? "");
      setFacts((currentCard?.facts ?? []).join("\n"));
    } catch {
      setCards([]);
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bookId]);

  const current = cards.find((card) => card.name === selected);

  const select = (name: string) => {
    setSelected(name);
    const card = cards.find((item) => item.name === name);
    setSummary(card?.summary ?? "");
    setFacts((card?.facts ?? []).join("\n"));
  };

  const save = async () => {
    if (!current) return;
    const next = cards.map((card) =>
      card.name === current.name
        ? {
            ...card,
            summary: summary.trim(),
            facts: facts.split("\n").map((line) => line.trim()).filter(Boolean),
          }
        : card,
    );
    try {
      await fetchJson(`/books/${encodeURIComponent(bookId)}/codex`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ cards: next }),
      });
      setNotice(tr("已保存", "Saved"));
      await load();
    } catch {
      setNotice(tr("保存失败", "Save failed"));
    }
  };

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("实体卡", "Entity codex")}
      </h3>
      {cards.length === 0 ? (
        <p className="text-xs text-muted-foreground italic">
          {tr(
            "暂无实体——名册确认后会自动派生卡片。",
            "No entities yet — cards are derived automatically once the roster is confirmed.",
          )}
        </p>
      ) : (
        <div className="space-y-2">
          <div className="flex flex-wrap gap-1">
            {cards.map((card) => (
              <button
                key={card.name}
                onClick={() => select(card.name)}
                className={`px-2 py-1 text-[11px] rounded-md border ${
                  selected === card.name
                    ? "border-primary text-primary font-bold"
                    : "border-border text-muted-foreground"
                }`}
              >
                {card.name}
              </button>
            ))}
          </div>
          {current && (
            <div className="space-y-1">
              <textarea
                value={summary}
                onChange={(event) => setSummary(event.target.value)}
                placeholder={tr("卡片摘要：谁/什么（≤300 字）", "Summary: who/what (≤300 chars)")}
                rows={2}
                className="w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background"
              />
              <textarea
                value={facts}
                onChange={(event) => setFacts(event.target.value)}
                placeholder={tr("正典事实（每行一条，写作时注入）", "Canon facts (one per line, injected while writing)")}
                rows={3}
                className="w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background"
              />
              <button
                onClick={save}
                className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground"
              >
                {tr("保存卡片", "Save card")}
              </button>
            </div>
          )}
          {notice && <p className="text-[11px] text-emerald-600">{notice}</p>}
        </div>
      )}
    </div>
  );
}
