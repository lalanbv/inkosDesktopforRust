import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface SeriesCanonEntry {
  readonly name: string;
  readonly aliases: ReadonlyArray<string>;
  readonly kind: string;
  readonly summary: string;
  readonly facts: ReadonlyArray<string>;
  readonly relationships: ReadonlyArray<{ target: string; note: string }>;
}

const SERIES_ID_PATTERN = /^[a-z0-9_]{1,64}$/;

/**
 * R20/390 号：系列正典共享面板（书籍详情挂载）。
 * 书级 seriesId 绑定（PUT /books/:id/series-id）+ 跨书正典条目编辑
 * （GET/PUT /series/:seriesId/canon）——同系列多书消费同一套
 * 角色/设定条目，写作时按场景命中注入「## 系列实体卡」块。
 */
export function SeriesCanonPanel({ bookId }: { bookId: string }) {
  const [seriesId, setSeriesId] = useState<string>("");
  const [seriesIdInput, setSeriesIdInput] = useState<string>("");
  const [entries, setEntries] = useState<ReadonlyArray<SeriesCanonEntry>>([]);
  const [selected, setSelected] = useState<string>("");
  const [aliases, setAliases] = useState("");
  const [summary, setSummary] = useState("");
  const [facts, setFacts] = useState("");
  const [notice, setNotice] = useState("");

  const loadBinding = async () => {
    try {
      const data = await fetchJson<{ seriesId: string | null }>(
        `/books/${encodeURIComponent(bookId)}/series-id`,
      );
      const bound = data.seriesId ?? "";
      setSeriesId(bound);
      setSeriesIdInput(bound);
    } catch {
      setSeriesId("");
      setSeriesIdInput("");
    }
  };

  const loadCanon = async (id: string) => {
    if (!id) {
      setEntries([]);
      setSelected("");
      return;
    }
    try {
      const data = await fetchJson<{ entries: ReadonlyArray<SeriesCanonEntry> }>(
        `/series/${encodeURIComponent(id)}/canon`,
      );
      setEntries(data.entries);
      const next = data.entries.some((entry) => entry.name === selected)
        ? selected
        : (data.entries[0]?.name ?? "");
      setSelected(next);
      // 442 号：自动选中必须同步回填编辑字段——字段留空而条目处于选中态时，
      // 直接「保存条目」会把 aliases/summary/facts 静默清空。
      populateFields(data.entries.find((entry) => entry.name === next));
    } catch {
      setEntries([]);
    }
  };

  useEffect(() => {
    void loadBinding();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bookId]);

  useEffect(() => {
    void loadCanon(seriesId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seriesId]);

  const current = entries.find((entry) => entry.name === selected);

  const populateFields = (entry?: SeriesCanonEntry) => {
    setAliases((entry?.aliases ?? []).join(", "));
    setSummary(entry?.summary ?? "");
    setFacts((entry?.facts ?? []).join("\n"));
  };

  const select = (name: string) => {
    setSelected(name);
    populateFields(entries.find((item) => item.name === name));
  };

  const saveBinding = async () => {
    const next = seriesIdInput.trim();
    if (next && !SERIES_ID_PATTERN.test(next)) {
      setNotice(tr("系列 ID 须为 snake_case（字母/数字/下划线，≤64）", "Series id must be a snake_case slug (≤64)"));
      return;
    }
    try {
      await fetchJson(`/books/${encodeURIComponent(bookId)}/series-id`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ seriesId: next || null }),
      });
      setNotice(tr("已保存系列绑定", "Series binding saved"));
      await loadBinding();
    } catch {
      setNotice(tr("保存失败", "Save failed"));
    }
  };

  const saveEntry = async () => {
    if (!current || !seriesId) return;
    const next = entries.map((entry) =>
      entry.name === current.name
        ? {
            ...entry,
            aliases: aliases.split(/[,，]/).map((alias) => alias.trim()).filter(Boolean),
            summary: summary.trim(),
            facts: facts.split("\n").map((line) => line.trim()).filter(Boolean),
          }
        : entry,
    );
    try {
      await fetchJson(`/series/${encodeURIComponent(seriesId)}/canon`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ entries: next }),
      });
      setNotice(tr("已保存条目", "Entry saved"));
      await loadCanon(seriesId);
    } catch {
      setNotice(tr("保存失败", "Save failed"));
    }
  };

  const addEntry = async () => {
    if (!seriesId) return;
    const name = tr("新条目", "New entry");
    let suffix = 1;
    let candidate = name;
    while (entries.some((entry) => entry.name === candidate)) {
      suffix += 1;
      candidate = `${name}-${suffix}`;
    }
    const next = [
      ...entries,
      { name: candidate, aliases: [], kind: "other", summary: "", facts: [], relationships: [] },
    ];
    try {
      await fetchJson(`/series/${encodeURIComponent(seriesId)}/canon`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ entries: next }),
      });
      await loadCanon(seriesId);
      setSelected(candidate);
      setAliases("");
      setSummary("");
      setFacts("");
    } catch {
      setNotice(tr("新增失败", "Add failed"));
    }
  };

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("系列正典（跨书共享）", "Series canon (shared across books)")}
      </h3>
      <div className="flex gap-1">
        <input
          value={seriesIdInput}
          onChange={(event) => setSeriesIdInput(event.target.value)}
          placeholder={tr("系列 ID（如 wu_dong_qian_kun）", "Series id (e.g. wu_dong_qian_kun)")}
          className="flex-1 text-xs border border-border rounded-md px-2 py-1.5 bg-background"
        />
        <button
          onClick={saveBinding}
          className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground"
        >
          {seriesId ? tr("改绑", "Rebind") : tr("绑定", "Bind")}
        </button>
      </div>
      {seriesId ? (
        entries.length === 0 ? (
          <div className="space-y-2">
            <p className="text-xs text-muted-foreground italic">
              {tr(
                "该系列暂无共享条目——新增后在同系列其他书中自动生效。",
                "No shared entries in this series yet — add one and it applies to every book in the series.",
              )}
            </p>
            <button
              onClick={addEntry}
              className="px-3 py-1.5 text-xs rounded-md border border-border"
            >
              {tr("新增共享条目", "Add shared entry")}
            </button>
          </div>
        ) : (
          <div className="space-y-2">
            <div className="flex flex-wrap gap-1">
              {entries.map((entry) => (
                <button
                  key={entry.name}
                  onClick={() => select(entry.name)}
                  className={`px-2 py-1 text-[11px] rounded-md border ${
                    selected === entry.name
                      ? "border-primary text-primary font-bold"
                      : "border-border text-muted-foreground"
                  }`}
                >
                  {entry.name}
                </button>
              ))}
            </div>
            {current && (
              <div className="space-y-1">
                <input
                  value={aliases}
                  onChange={(event) => setAliases(event.target.value)}
                  placeholder={tr("别名（逗号分隔，场景命中计入）", "Aliases (comma-separated, counted in scene matches)")}
                  className="w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background"
                />
                <textarea
                  value={summary}
                  onChange={(event) => setSummary(event.target.value)}
                  placeholder={tr("条目摘要：谁/什么（≤300 字）", "Summary: who/what (≤300 chars)")}
                  rows={2}
                  className="w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background"
                />
                <textarea
                  value={facts}
                  onChange={(event) => setFacts(event.target.value)}
                  placeholder={tr("跨书正典事实（每行一条，写作时注入）", "Cross-book canon facts (one per line, injected while writing)")}
                  rows={3}
                  className="w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background"
                />
                <button
                  onClick={saveEntry}
                  className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground"
                >
                  {tr("保存条目", "Save entry")}
                </button>
              </div>
            )}
            {notice && <p className="text-[11px] text-emerald-600">{notice}</p>}
          </div>
        )
      ) : (
        notice && <p className="text-[11px] text-emerald-600">{notice}</p>
      )}
    </div>
  );
}
