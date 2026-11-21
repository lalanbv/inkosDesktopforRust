import { useEffect, useMemo, useState } from "react";
// 子路径 import（组件惯例——barrel 会把 node 依赖模块拉进浏览器 bundle）。
import { dailyWordDelta, goalStreak, localDayKey } from "@actalk/inkos-core/utils/writing-goal";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

/**
 * R51/1653 号：写作目标面板（WritingStatsCard 内嵌首档）——日产出账本
 * （.inkos/writing-log.jsonl，编辑保存 Δ≠0 记账）→ 日净增月历热图（三档
 * 着色）+ streak + 日目标。dayKey 全部在**前端本地时区**计算（服务端行面
 * 零时区依赖）；日目标存 localStorage（应用设置面键挂下轮）。
 * 书级 targetWords 进度条与目标编辑入口挂下轮（BookDetail 撞并行会话面）。
 */

interface LogRow {
  readonly bookId: string;
  readonly ts: string;
  readonly chapterNum: number;
  readonly before: number;
  readonly after: number;
}

const GOAL_STORAGE_KEY = "inkos.dailyGoalWords";
const DEFAULT_DAILY_GOAL = 500;

function loadDailyGoal(): number {
  const stored = globalThis.localStorage?.getItem(GOAL_STORAGE_KEY);
  const parsed = stored ? Number.parseInt(stored, 10) : Number.NaN;
  return Number.isInteger(parsed) && parsed > 0 ? parsed : DEFAULT_DAILY_GOAL;
}

export function WritingGoalPanel() {
  const [rows, setRows] = useState<ReadonlyArray<LogRow> | null>(null);
  const [goal, setGoal] = useState(DEFAULT_DAILY_GOAL);

  useEffect(() => {
    setGoal(loadDailyGoal());
    void fetchJson<{ rows: ReadonlyArray<LogRow> }>("/writing-log-rows")
      .then((payload) => setRows(payload.rows ?? []))
      .catch(() => setRows([]));
  }, []);

  const todayKey = localDayKey(new Date());
  const deltas = useMemo(
    () => (rows ? dailyWordDelta(rows, (row) => localDayKey(new Date(row.ts))) : []),
    [rows],
  );
  const streak = useMemo(() => goalStreak(deltas, goal, todayKey), [deltas, goal, todayKey]);

  const month = useMemo(() => {
    // 当月网格：周一为首列；日格三档着色（0/未达/达成）。
    const [year, monthPart] = todayKey.split("-");
    const first = new Date(Number(year), Number(monthPart) - 1, 1);
    const daysInMonth = new Date(Number(year), Number(monthPart), 0).getDate();
    const leadingBlanks = (first.getDay() + 6) % 7;
    const byDay = new Map(deltas.map((entry) => [entry.dayKey, entry.delta]));
    const cells: Array<{ dayKey: string | null; day: number; delta: number | undefined }> = [];
    for (let blank = 0; blank < leadingBlanks; blank += 1) {
      cells.push({ dayKey: null, day: 0, delta: undefined });
    }
    let monthTotal = 0;
    for (let day = 1; day <= daysInMonth; day += 1) {
      const dayKey = [year, monthPart, String(day).padStart(2, "0")].join("-");
      const delta = byDay.get(dayKey);
      if (delta !== undefined && dayKey <= todayKey) monthTotal += delta;
      cells.push({ dayKey, day, delta });
    }
    return { cells, monthTotal };
  }, [deltas, todayKey]);

  const updateGoal = (raw: string) => {
    const parsed = Number.parseInt(raw, 10);
    if (Number.isInteger(parsed) && parsed > 0) {
      setGoal(parsed);
      globalThis.localStorage?.setItem(GOAL_STORAGE_KEY, String(parsed));
    }
  };

  if (!rows) {
    return (
      <div className="pt-2 border-t border-border/60 mt-2" data-goal-panel="loading">
        <p className="text-[11px] text-muted-foreground">{tr("目标账本载入中…", "Loading goal ledger…")}</p>
      </div>
    );
  }

  return (
    <div className="pt-2 border-t border-border/60 mt-2 space-y-2" data-goal-panel="ready">
      <div className="flex flex-wrap items-center gap-3 text-[11px] text-muted-foreground">
        <span>
          🔥 {tr("连续达成", "Streak")}:{" "}
          <span className="font-bold text-foreground" data-goal="streak">{streak}</span>
        </span>
        <span>
          {tr("当月净增", "Month net")}:{" "}
          <span className="font-bold text-foreground" data-goal="month-total">
            {month.monthTotal.toLocaleString()} {tr("字", "words")}
          </span>
        </span>
        <label className="flex items-center gap-1">
          {tr("日目标", "Daily goal")}:
          <input
            type="number"
            min={1}
            value={goal}
            onChange={(event) => updateGoal(event.target.value)}
            className="w-16 rounded border border-border/60 bg-background px-1 py-0.5 text-[11px] text-foreground"
            data-goal="daily-goal-input"
          />
        </label>
      </div>
      <div className="grid grid-cols-7 gap-1" data-goal="heatmap">
        {month.cells.map((cell, index) => (
          <div
            key={cell.dayKey ?? `blank-${index}`}
            title={
              cell.dayKey && cell.delta !== undefined
                ? tr(`${cell.dayKey}: ${cell.delta} 字`, `${cell.dayKey}: ${cell.delta} words`)
                : cell.dayKey ?? ""
            }
            data-goal-cell={cell.dayKey ?? undefined}
            data-goal-delta={cell.delta ?? "none"}
            className={`h-5 rounded text-center text-[9px] leading-5 ${
              cell.dayKey === null
                ? ""
                : cell.delta === undefined || cell.delta === 0
                  ? "bg-muted text-muted-foreground"
                  : cell.delta >= goal
                    ? "bg-primary text-primary-foreground"
                    : "bg-primary/40 text-foreground"
            }`}
          >
            {cell.dayKey ? cell.day : ""}
          </div>
        ))}
      </div>
    </div>
  );
}
