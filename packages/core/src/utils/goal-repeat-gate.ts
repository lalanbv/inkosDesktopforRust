/**
 * R28 章节目标防复读门（v5 五轮规划 P1，399 号）。
 *
 * planner 产物校验：memo.goal 与近 N 章摘要（章名+事件）高度重合 → 判定复读。
 * 设计约束（v5 风险表）：
 * - 阈值保守起步（0.8 Dice），只重规划一次，仍犯降级告警不阻断；
 * - 判定口径由共享 golden 锁定（`golden/goal-repeat-vectors.json`，
 *   TS `golden-goal-repeat.test.ts` / Rust `tests/golden_goal_repeat_diff.rs`）；
 * - 双端 1:1：`engine-rs/src/utils/goal_repeat_gate.rs`。归一化只做
 *   ASCII 小写 + 保留「字母/数字/汉字」——禁用 toLowerCase 全量与 locale
 *   排序（367/352 先例：locale 面不可入双端契约）。
 */

/** 相似度阈值（Dice 系数，保守高阈值起步防误杀合法回环章节）。 */
export const GOAL_REPEAT_SIMILARITY_THRESHOLD = 0.8;
/** 对照窗口：近 N 章摘要。 */
export const GOAL_REPEAT_WINDOW = 3;

export interface GoalRepeatVerdict {
  readonly repeat: boolean;
  /** 与最相似对照文本的 Dice 相似度（4 位小数截断，双端口径一致）。 */
  readonly maxSimilarity: number;
  /** 归一化后与某对照文本完全相等（廉价强信号）。 */
  readonly exactMatch: boolean;
}

const isAsciiLower = (c: number): boolean => c >= 0x61 && c <= 0x7a;
const isAsciiUpper = (c: number): boolean => c >= 0x41 && c <= 0x5a;
const isAsciiDigit = (c: number): boolean => c >= 0x30 && c <= 0x39;
const isHan = (c: number): boolean => c >= 0x4e00 && c <= 0x9fff;

/** 保留字母/数字/汉字（\u4e00–\u9fff），其余剔除；ASCII 小写。 */
export function normalizeGoalRepeatText(raw: string): string {
  let out = "";
  for (const ch of raw) {
    const code = ch.codePointAt(0)!;
    if (isAsciiLower(code) || isAsciiDigit(code) || isHan(code)) {
      out += ch;
    } else if (isAsciiUpper(code)) {
      out += String.fromCharCode(code + 0x20);
    }
  }
  return out;
}

function normalizedBigrams(normalized: string): Set<string> {
  const chars = Array.from(normalized);
  if (chars.length < 2) return new Set(chars);
  const set = new Set<string>();
  for (let i = 0; i < chars.length - 1; i += 1) {
    set.add(chars[i]! + chars[i + 1]!);
  }
  return set;
}

/** 字符 bigram 集合 Dice 系数；4 位小数向下截断（floor 口径，双端一致）。 */
export function goalRepeatSimilarity(a: string, b: string): number {
  const na = normalizeGoalRepeatText(a);
  const nb = normalizeGoalRepeatText(b);
  // 退化短串：非空且相等即全同，否则无重叠（空 bigram 集合的 Dice 无定义）。
  if (na.length < 2 || nb.length < 2) {
    return na.length > 0 && na === nb ? 1 : 0;
  }
  const ga = normalizedBigrams(na);
  const gb = normalizedBigrams(nb);
  let overlap = 0;
  for (const gram of ga) {
    if (gb.has(gram)) overlap += 1;
  }
  const dice = (2 * overlap) / (ga.size + gb.size);
  return Math.floor(dice * 10000) / 10000;
}

/**
 * 复读判定：goal 与任一对照文本完全相等（归一化后）或相似度 ≥ 阈值 → repeat。
 * referenceTexts 为空（开书首章）或 goal 归一化后为空 → 恒不判复读。
 */
export function evaluateGoalRepeat(
  goal: string,
  referenceTexts: ReadonlyArray<string>,
  threshold: number = GOAL_REPEAT_SIMILARITY_THRESHOLD,
): GoalRepeatVerdict {
  const normalizedGoal = normalizeGoalRepeatText(goal);
  if (!normalizedGoal || referenceTexts.length === 0) {
    return { repeat: false, maxSimilarity: 0, exactMatch: false };
  }
  let maxSimilarity = 0;
  let exactMatch = false;
  for (const reference of referenceTexts) {
    const normalized = normalizeGoalRepeatText(reference);
    if (!normalized) continue;
    if (normalized === normalizedGoal) exactMatch = true;
    maxSimilarity = Math.max(maxSimilarity, goalRepeatSimilarity(goal, reference));
  }
  return {
    repeat: exactMatch || maxSimilarity >= threshold,
    maxSimilarity,
    exactMatch,
  };
}
