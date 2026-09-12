import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface DirectorSession {
  readonly bookId?: string;
  readonly runMode?: "ready-stop" | "range" | "full-book";
  readonly stage?: string;
  readonly inspiration?: { readonly premise?: string; readonly keywords?: string[] };
  readonly selectedDirection?: { readonly title?: string } | null;
  readonly updatedAt?: string;
}

interface Payload {
  readonly session: DirectorSession | null;
  readonly savedChapters: number;
  readonly resumeAdvice: string;
}

const RUN_MODES: ReadonlyArray<{ value: DirectorSessionRunMode; zh: string; en: string }> = [
  { value: "ready-stop", zh: "就绪即停", en: "Ready-stop" },
  { value: "range", zh: "范围写作", en: "Range" },
  { value: "full-book", zh: "全书", en: "Full book" },
];
type DirectorSessionRunMode = "ready-stop" | "range" | "full-book";

/**
 * G6/353 号：导演驾驶舱/跟进面板（书籍详情挂载，拉模式）。
 * 灵感卡编辑 + 运行模式选择 + 会话保存 + G9 续跑建议注入。
 */
export function DirectorPanel({ bookId }: { bookId: string }) {
  const [premise, setPremise] = useState("");
  const [runMode, setRunMode] = useState<DirectorSessionRunMode>("ready-stop");
  const [stage, setStage] = useState<string>("inspiration");
  const [savedChapters, setSavedChapters] = useState(0);
  const [resumeAdvice, setResumeAdvice] = useState("");
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState("");
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    fetchJson<Payload>(`/books/${encodeURIComponent(bookId)}/director`)
      .then((data) => {
        if (cancelled) return;
        setPremise(data.session?.inspiration?.premise ?? "");
        if (data.session?.runMode) setRunMode(data.session.runMode);
        if (data.session?.stage) setStage(data.session.stage);
        setSavedChapters(data.savedChapters ?? 0);
        setResumeAdvice(data.resumeAdvice ?? "");
      })
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [bookId]);

  const save = async () => {
    setSaving(true);
    try {
      await fetchJson(`/books/${encodeURIComponent(bookId)}/director`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          patch: {
            runMode,
            stage,
            inspiration: { premise, keywords: [] },
          },
        }),
      });
      setNotice(tr("已保存。", "Saved."));
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
    setSaving(false);
  };

  if (!loaded) return null;

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("导演驾驶舱", "Director cockpit")}
      </h3>
      <div className="flex flex-wrap items-center gap-2">
        <input
          value={premise}
          onChange={(event) => setPremise(event.target.value)}
          placeholder={tr("一句话灵感…", "One-line premise…")}
          className="flex-1 min-w-48 text-xs border border-border rounded-md px-2 py-1.5 bg-background"
        />
        <select
          value={runMode}
          onChange={(event) => setRunMode(event.target.value as DirectorSessionRunMode)}
          className="text-xs border border-border rounded-md px-2 py-1.5 bg-background"
        >
          {RUN_MODES.map((mode) => (
            <option key={mode.value} value={mode.value}>
              {tr(mode.zh, mode.en)}
            </option>
          ))}
        </select>
        <button
          onClick={save}
          disabled={saving}
          className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-30"
        >
          {saving ? tr("保存中…", "Saving…") : tr("保存", "Save")}
        </button>
      </div>
      <div className="text-xs text-muted-foreground">
        {tr("阶段", "Stage")}: <span className="text-foreground">{stage}</span>
        {" · "}
        {tr("已存章节", "Saved chapters")}: {savedChapters}
      </div>
      {resumeAdvice && (
        <div className="text-xs text-muted-foreground">
          {tr("续跑建议", "Resume advice")}: {resumeAdvice}
        </div>
      )}
      {notice && <div className="text-xs text-muted-foreground">{notice}</div>}
    </div>
  );
}
