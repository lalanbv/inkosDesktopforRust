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

/** 508 号：方向候选（core DirectionCandidate 同形；置信度 0–1）。 */
interface DirectionCandidate {
  readonly id: string;
  readonly title: string;
  readonly hook: string;
  readonly genre: string;
  readonly synopsis: string;
  readonly differentiator: string;
  readonly confidence: number;
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
 * 灵感卡编辑 + 方向候选生成 + 运行模式选择 + 会话保存 + G9 续跑建议注入。
 */
export function DirectorPanel({ bookId }: { bookId: string }) {
  const [premise, setPremise] = useState("");
  // 478 号：keywords 不在表单展示，但保存 patch 会整体替换 inspiration——
  // 必须加载时回填、保存时原样回传，否则静默清空已存关键词（442 缺陷类）。
  const [keywords, setKeywords] = useState<string[]>([]);
  const [runMode, setRunMode] = useState<DirectorSessionRunMode>("ready-stop");
  const [stage, setStage] = useState<string>("inspiration");
  const [savedChapters, setSavedChapters] = useState(0);
  const [resumeAdvice, setResumeAdvice] = useState("");
  const [selectedTitle, setSelectedTitle] = useState("");
  // 508 号：方向候选生成（354 号端点首次 UI 接线）。
  const [directions, setDirections] = useState<DirectionCandidate[]>([]);
  const [generating, setGenerating] = useState(false);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState("");
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    fetchJson<Payload>(`/books/${encodeURIComponent(bookId)}/director`)
      .then((data) => {
        if (cancelled) return;
        setPremise(data.session?.inspiration?.premise ?? "");
        setKeywords(data.session?.inspiration?.keywords ?? []);
        if (data.session?.runMode) setRunMode(data.session.runMode);
        if (data.session?.stage) setStage(data.session.stage);
        setSavedChapters(data.savedChapters ?? 0);
        setResumeAdvice(data.resumeAdvice ?? "");
        setSelectedTitle(data.session?.selectedDirection?.title ?? "");
      })
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [bookId]);

  // 508 号：方向候选生成——354 号端点首次 UI 接线（premise 必填守卫）。
  const generateDirections = async () => {
    if (!premise.trim()) {
      setNotice(tr("请先填写一句话灵感", "Write a one-line premise first"));
      return;
    }
    setGenerating(true);
    setNotice("");
    try {
      const data = await fetchJson<{ directions: DirectionCandidate[] }>(
        `/api/v1/director/directions`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ inspiration: { premise, keywords }, count: 3 }),
        },
      );
      setDirections(data.directions ?? []);
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
    setGenerating(false);
  };

  // 508 号：选用候选 → 会话 selectedDirection（幂等重放由引擎 PUT 语义承担）。
  const selectDirection = (candidate: DirectionCandidate) => {
    try {
      void fetchJson(`/books/${encodeURIComponent(bookId)}/director`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ patch: { selectedDirection: candidate } }),
      });
      setSelectedTitle(candidate.title);
      setNotice(`已选用方向：${candidate.title}`);
    } catch {
      setNotice("选用失败，请重试");
    }
  };

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
            inspiration: { premise, keywords },
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
        <button
          onClick={generateDirections}
          disabled={generating}
          className="px-3 py-1.5 text-xs rounded-md border border-border hover:bg-muted/30 disabled:opacity-30"
        >
          {tr("生成方向候选", "Generate directions")}
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
      {selectedTitle && (
        <div className="text-xs text-muted-foreground">
          {tr("已选用方向", "Selected direction")}: <span className="text-foreground">{selectedTitle}</span>
        </div>
      )}
      {directions.length > 0 && (
        <ul className="space-y-1">
          {directions.map((candidate) => (
            <li
              key={candidate.id}
              className="text-xs border border-border/40 rounded-md px-2 py-1 space-y-0.5 cursor-pointer hover:bg-muted/30"
              onClick={() => selectDirection(candidate)}
            >
              <div className="font-medium text-foreground">
                {candidate.title}
                <span className="ml-2 text-muted-foreground font-normal">{candidate.genre}</span>
                <span className="ml-2 text-muted-foreground/70">
                  {tr("置信度", "confidence")} {(candidate.confidence * 100).toFixed(0)}%
                </span>
              </div>
              <div className="text-muted-foreground">{candidate.hook}</div>
              <div className="text-muted-foreground/70">{candidate.differentiator}</div>
            </li>
          ))}
        </ul>
      )}
      {notice && <div className="text-xs text-muted-foreground">{notice}</div>}
    </div>
  );
}
