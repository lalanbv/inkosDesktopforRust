import { useEffect, useMemo, useState } from "react";
// 子路径 import（组件惯例——barrel 会把 node 依赖模块拉进浏览器 bundle）。
import { dailyWordDelta, expectedFinish, localDayKey } from "@actalk/inkos-core/utils/writing-goal";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

/**
 * R51/1654 号：书级写作目标卡（BookDetail 设置页挂载，VolumeManageCard 旁）
 * ——targetWords/deadline 编辑（PUT /books/:id，number|null=设置/清除）+
 * 目标进度条 + expectedFinish 预计完成日（pace=近 7 日账本日均净增，
 * dayKey 前端本地计算）。施工图 §2.4#1/#4。
 */

interface ChapterRow {
  readonly number: number;
  readonly wordCount: number;
}

interface BookConfigLike {
  readonly targetWords?: number;
  readonly deadline?: string;
}

interface LogRow {
  readonly bookId: string;
  readonly ts: string;
  readonly chapterNum: number;
  readonly before: number;
  readonly after: number;
}

const PACE_WINDOW_DAYS = 7;

export function BookGoalCard({ bookId, chapters }: { bookId: string; chapters: ReadonlyArray<ChapterRow> }) {
  const [book, setBook] = useState<BookConfigLike | null>(null);
  const [logRows, setLogRows] = useState<ReadonlyArray<LogRow>>([]);
  const [targetInput, setTargetInput] = useState("");
  const [deadlineInput, setDeadlineInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");

  const reload = async () => {
    try {
      const config = await fetchJson<BookConfigLike>(`/books/${encodeURIComponent(bookId)}`);
      setBook(config);
      setTargetInput(config.targetWords ? String(config.targetWords) : "");
      setDeadlineInput(config.deadline ?? "");
    } catch {
      setBook(null);
    }
    try {
      const payload = await fetchJson<{ rows: ReadonlyArray<LogRow> }>("/writing-log-rows");
      setLogRows(payload.rows?.filter((row) => row.bookId === bookId) ?? []);
    } catch {
      setLogRows([]);
    }
  };

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bookId]);

  const totalWords = useMemo(
    () => chapters.reduce((sum, chapter) => sum + (chapter.wordCount || 0), 0),
    [chapters],
  );
  const targetWords = book?.targetWords;
  const todayKey = localDayKey(new Date());
  // pace = 近 7 日（含今日）账本日均净增（>0 才可投影）。
  const pace = useMemo(() => {
    const deltas = dailyWordDelta(logRows, (row) => localDayKey(new Date(row.ts)));
    const recent = deltas.filter((entry) => entry.dayKey >= recentWindowStart(todayKey, PACE_WINDOW_DAYS));
    const net = recent.reduce((sum, entry) => sum + entry.delta, 0);
    return recent.length >= 1 && net > 0 ? net / PACE_WINDOW_DAYS : 0;
  }, [logRows, todayKey]);
  const projected = useMemo(
    () => (targetWords && pace > 0 ? expectedFinish(totalWords, targetWords, Math.round(pace), todayKey) : undefined),
    [targetWords, pace, totalWords, todayKey],
  );

  const save = async () => {
    const trimmed = targetInput.trim();
    const parsed = trimmed ? Number(trimmed) : null;
    if (trimmed && (!Number.isInteger(parsed) || (parsed ?? 0) < 1)) {
      setNotice(tr("目标字数须为正整数。", "Target words must be a positive integer."));
      return;
    }
    setBusy(true);
    setNotice("");
    try {
      await fetchJson(`/books/${encodeURIComponent(bookId)}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          targetWords: parsed,
          deadline: deadlineInput.trim() ? deadlineInput.trim() : null,
        }),
      });
      await reload();
      setNotice(tr("目标已保存。", "Goal saved."));
    } catch {
      setNotice(tr("保存失败", "Save failed"));
    } finally {
      setBusy(false);
    }
  };

  const pct = targetWords ? Math.min(100, Math.round((totalWords / targetWords) * 100)) : 0;

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2" data-goal-card="root">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("写作目标", "Writing goal")}
      </h3>
      {book === null ? (
        <p className="text-xs text-muted-foreground italic">{tr("载入中…", "Loading…")}</p>
      ) : (
        <div className="space-y-2">
          {targetWords ? (
            <div className="space-y-1">
              <div className="flex items-center justify-between text-[11px]">
                <span>
                  {tr("进度", "Progress")}:{" "}
                  <span className="font-bold text-foreground" data-goal-card="progress">
                    {totalWords.toLocaleString()} / {targetWords.toLocaleString()}（{pct}%）
                  </span>
                </span>
                {projected && (
                  <span data-goal-card="expected-finish">
                    {tr("按当前节奏预计", "At current pace")}: <span className="font-bold text-foreground">{projected}</span>
                  </span>
                )}
              </div>
              <div className="h-2 rounded bg-muted overflow-hidden">
                <div className="h-full bg-primary" style={{ width: `${pct}%` }} data-goal-card="bar" />
              </div>
              {book.deadline && (
                <p className="text-[11px] text-muted-foreground" data-goal-card="deadline">
                  {tr("截止日", "Deadline")}: {book.deadline}
                  {projected && projected > book.deadline
                    ? tr("（预计晚于截止）", " (projected to miss deadline)")
                    : projected
                      ? tr("（预计按期完成）", " (on track)")
                      : ""}
                </p>
              )}
              {pace === 0 && (
                <p className="text-[11px] text-muted-foreground italic">
                  {tr("近 7 天无净增——写作后这里会出现预计完成日。", "No net words in the last 7 days — the projected finish date appears after you write.")}
                </p>
              )}
            </div>
          ) : (
            <p className="text-xs text-muted-foreground italic">
              {tr("尚未设置全书目标字数。", "No book-level word target yet.")}
            </p>
          )}
          <div className="flex flex-wrap items-end gap-2">
            <label className="flex flex-col gap-1 text-[11px] text-muted-foreground">
              {tr("目标字数", "Target words")}
              <input
                type="number"
                min={1}
                value={targetInput}
                onChange={(event) => setTargetInput(event.target.value)}
                placeholder="200000"
                className="w-28 rounded border border-border/60 bg-background px-2 py-1 text-xs text-foreground"
                data-goal-card="target-input"
              />
            </label>
            <label className="flex flex-col gap-1 text-[11px] text-muted-foreground">
              {tr("截止日", "Deadline")}
              <input
                type="date"
                value={deadlineInput}
                onChange={(event) => setDeadlineInput(event.target.value)}
                className="rounded border border-border/60 bg-background px-2 py-1 text-xs text-foreground"
                data-goal-card="deadline-input"
              />
            </label>
            <button
              type="button"
              disabled={busy}
              onClick={() => void save()}
              className="rounded bg-primary text-primary-foreground px-3 py-1 text-xs disabled:opacity-50"
              data-goal-card="save"
            >
              {busy ? tr("保存中…", "Saving…") : tr("保存", "Save")}
            </button>
            {notice && (
              <span className="text-[11px] text-muted-foreground" data-goal-card="notice">{notice}</span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/** 近 N 日窗口起点 dayKey（含今日）。 */
function recentWindowStart(todayKey: string, days: number): string {
  const [year, month, day] = todayKey.split("-").map(Number);
  const ms = Date.UTC(year!, month! - 1, day!) - (days - 1) * 86_400_000;
  return new Date(ms).toISOString().slice(0, 10);
}
