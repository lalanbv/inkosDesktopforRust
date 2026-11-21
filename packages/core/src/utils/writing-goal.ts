/**
 * R51/1653 号：写作目标与日产出账本纯函数（Rust 镜像 =
 * engine-rs/src/utils/writing_goal.rs，golden writing-goal-vectors.json 锁定）。
 *
 * 数据模型（1647 施工图 §2.2/§2.3）：
 * - 账本行 = `{ts, chapterNum, before, after}` 四键（ts=UTC ISO——写入端
 *   不做时区换算；仅元数据，无正文）。
 * - 日净增 = Σ 该日本地日记录 (after-before)（可为负=删稿日）。
 * - **dayKey 全部由消费端计算后入参**（`dayKeyOf`/`todayKey`）——纯函数
 *   不吃时区环境（可测性；时区边界 golden 直喂边界 dayKey 序列；月历
 *   「用户看自己当地日」=消费端本地视图语义）。
 */

export interface WritingLogRow {
  readonly ts: string;
  readonly chapterNum: number;
  readonly before: number;
  readonly after: number;
}

export interface DayDelta {
  readonly dayKey: string;
  readonly delta: number;
}

/** 记录流 + dayKey 映射 → 日净增表（dayKey 升序；无记录日不出现）。 */
export function dailyWordDelta(
  rows: ReadonlyArray<WritingLogRow>,
  dayKeyOf: (row: WritingLogRow) => string,
): ReadonlyArray<DayDelta> {
  const buckets = new Map<string, number>();
  for (const row of rows) {
    const dayKey = dayKeyOf(row);
    buckets.set(dayKey, (buckets.get(dayKey) ?? 0) + (row.after - row.before));
  }
  return [...buckets.entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([dayKey, delta]) => ({dayKey, delta}));
}

/**
 * 连续达成天数（达成=当日净增 ≥ 日目标）。
 * - 端点 = 净增表最大 dayKey；从端点日逐日回溯（**缺日=0=断链**）。
 * - 今日已达成 → 计入（正向激励）；今日未达成 → 跳过不毁（进行中，从
 *   昨日起回溯）；「今日未写不计」= 今日无记录同样跳过不毁。
 * - 非今日的未达标/缺日 → 断链重置（停）。
 */
export function goalStreak(
  deltas: ReadonlyArray<DayDelta>,
  dailyGoalWords: number,
  todayKey: string,
): number {
  if (deltas.length === 0) return 0;
  const byDay = new Map(deltas.map((entry) => [entry.dayKey, entry.delta]));
  let endpoint = deltas[deltas.length - 1]!.dayKey;
  for (const entry of deltas) {
    if (entry.dayKey > endpoint) endpoint = entry.dayKey;
  }
  let streak = 0;
  // 今日未达成（或未写）跳过：进行中不判定，从昨日起回溯。
  if (endpoint === todayKey && (byDay.get(todayKey) ?? 0) < dailyGoalWords) {
    endpoint = shiftDayKey(todayKey, -1);
  }
  let dayKey = endpoint;
  while ((byDay.get(dayKey) ?? 0) >= dailyGoalWords) {
    streak += 1;
    dayKey = shiftDayKey(dayKey, -1);
  }
  return streak;
}

/**
 * 预计完成日（dayKey 算术）：剩余 = ceil((目标-当前)/日节奏) 天，完成日 =
 * todayKey + 剩余天数。
 * - 已达成（当前 ≥ 目标）→ undefined；日节奏 ≤ 0 → undefined（无法预测）。
 */
export function expectedFinish(
  currentWords: number,
  targetWords: number,
  dailyPaceWords: number,
  todayKey: string,
): string | undefined {
  if (currentWords >= targetWords) return undefined;
  if (dailyPaceWords <= 0) return undefined;
  const remaining = Math.ceil((targetWords - currentWords) / dailyPaceWords);
  return shiftDayKey(todayKey, remaining);
}

/** dayKey（YYYY-MM-DD）± 天数（UTC 日算术——dayKey 本身无时区，加法安全）。 */
export function shiftDayKey(dayKey: string, days: number): string {
  const parts = dayKey.split("-");
  if (parts.length !== 3) {
    throw new Error("Invalid dayKey format, expected YYYY-MM-DD");
  }
  const year = Number.parseInt(parts[0]!, 10);
  const month = Number.parseInt(parts[1]!, 10);
  const day = Number.parseInt(parts[2]!, 10);
  const ms = Date.UTC(year, month - 1, day) + days * 86400000;
  return new Date(ms).toISOString().slice(0, 10);
}

/** 本地日键（YYYY-MM-DD，按 Date 的本地分量）——消费端（浏览器/壳内前端）用。 */
export function localDayKey(now: Date): string {
  const year = String(now.getFullYear()).padStart(4, "0");
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return [year, month, day].join("-");
}
