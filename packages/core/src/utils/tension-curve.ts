/**
 * R2 张力曲线（358 号，二轮 P0）——settle 产物「TENSION_METRICS」节解析 +
 * 章级张力点列聚合 + 启发式读感告警。
 *
 * 分数由 settler 管道产出（355 号 §5：曲线是展示层，允许 ±1 抖动）；缺失分数
 * 的章自动跳过，告警只读点列不读正文。阈值默认值锚定在
 * `__tests__/golden/tension-curve-vectors.json`，双端共享差分。
 */

export interface TensionMetrics {
  readonly conflictLevel: number;
  readonly revealLevel: number;
}

/** 曲线输入行：`StoredSummary` 的张力子集（结构兼容，可直接传入）。 */
export interface TensionRow {
  readonly chapter: number;
  readonly conflictLevel?: number;
  readonly revealLevel?: number;
}

export interface TensionPoint {
  readonly chapter: number;
  readonly conflictLevel: number;
  readonly revealLevel: number;
}

export interface TensionCurve {
  readonly points: ReadonlyArray<TensionPoint>;
  readonly scoredChapters: number;
  readonly unscoredChapters: number;
}

export type TensionWarningKind = "flat-middle" | "climax-crowding" | "weak-hook-streak";

export interface TensionWarning {
  readonly kind: TensionWarningKind;
  readonly chapters: ReadonlyArray<number>;
  readonly severity: "warning";
  readonly description: string;
  readonly suggestion: string;
}

export interface TensionWarningOptions {
  /** 中段平坦判定的连续章数下限。 */
  readonly flatWindow?: number;
  /** 平坦 = 窗口内 conflictLevel 极差 ≤ 该值。 */
  readonly flatMaxRange?: number;
  /** 中段起点比例（× 全曲最大章号）。 */
  readonly middleStart?: number;
  /** 中段终点比例。 */
  readonly middleEnd?: number;
  /** 高潮揭示的 revealLevel 下限。 */
  readonly climaxMinLevel?: number;
  /** 尾段占比（× 全曲最大章号，向前取）。 */
  readonly climaxWindow?: number;
  /** 尾段高点数下限。 */
  readonly climaxMinCount?: number;
  /** 弱钩的 revealLevel 上限。 */
  readonly weakLevel?: number;
  /** 弱钩连击的连续章数下限。 */
  readonly weakWindow?: number;
}

export const TENSION_WARNING_DEFAULTS: Required<TensionWarningOptions> = {
  flatWindow: 4,
  flatMaxRange: 1,
  middleStart: 0.3,
  middleEnd: 0.7,
  climaxMinLevel: 8,
  climaxWindow: 0.25,
  climaxMinCount: 3,
  weakLevel: 3,
  weakWindow: 3,
};

const TENSION_METRICS_RE = /=== TENSION_METRICS ===\s*([\s\S]*?)(?==== [A-Z_]+ ===|$)/;

/**
 * 从 settler 输出解析张力评分节。JSON 形态（可带 ``` 围栏）优先，`key: value`
 * 行式回退（兼容中文冒号）；任一字段缺失/非法 → undefined（整节放弃，不产半截分）。
 * 数值 round 后 clamp 到 1–10。
 */
export function parseTensionMetrics(content: string): TensionMetrics | undefined {
  const section = content.match(TENSION_METRICS_RE)?.[1]?.trim();
  if (!section) return undefined;
  const fromJson = parseMetricsFromJson(section);
  const fromLines = parseMetricsFromKeyLines(section);
  const conflictLevel = fromJson?.conflictLevel ?? fromLines?.conflictLevel;
  const revealLevel = fromJson?.revealLevel ?? fromLines?.revealLevel;
  if (conflictLevel === undefined || revealLevel === undefined) return undefined;
  return {
    conflictLevel: clampLevel(conflictLevel),
    revealLevel: clampLevel(revealLevel),
  };
}

function parseMetricsFromJson(section: string): Partial<TensionMetrics> | undefined {
  const jsonMatch = section.match(/```(?:json)?\s*([\s\S]*?)\s*```/i) ?? [undefined, section];
  const raw = (jsonMatch[1] ?? section).trim();
  if (!raw.startsWith("{")) return undefined;
  try {
    const parsed: unknown = JSON.parse(raw.replace(/,\s*([}\]])/g, "$1"));
    if (typeof parsed !== "object" || parsed === null) return undefined;
    const record = parsed as Record<string, unknown>;
    return {
      conflictLevel: asFiniteNumber(record.conflictLevel),
      revealLevel: asFiniteNumber(record.revealLevel),
    };
  } catch {
    return undefined;
  }
}

function parseMetricsFromKeyLines(section: string): Partial<TensionMetrics> | undefined {
  const conflict = section.match(/^\s*conflictLevel\s*[:：]\s*(\d+(?:\.\d+)?)/im);
  const reveal = section.match(/^\s*revealLevel\s*[:：]\s*(\d+(?:\.\d+)?)/im);
  if (!conflict && !reveal) return undefined;
  return {
    conflictLevel: conflict ? Number(conflict[1]) : undefined,
    revealLevel: reveal ? Number(reveal[1]) : undefined,
  };
}

function asFiniteNumber(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() !== "" && Number.isFinite(Number(value))) {
    return Number(value);
  }
  return undefined;
}

function clampLevel(value: number): number {
  return Math.min(10, Math.max(1, Math.round(value)));
}

/** 聚合章级点列：仅收双分齐全的章，按章号升序（稳定排序，禁 localeCompare）。 */
export function buildTensionCurve(rows: ReadonlyArray<TensionRow>): TensionCurve {
  const points = rows
    .filter((row) =>
      typeof row.conflictLevel === "number" && typeof row.revealLevel === "number"
    )
    .map((row) => ({
      chapter: row.chapter,
      conflictLevel: row.conflictLevel as number,
      revealLevel: row.revealLevel as number,
    }))
    .sort((a, b) => a.chapter - b.chapter);
  return {
    points,
    scoredChapters: points.length,
    unscoredChapters: rows.length - points.length,
  };
}

/**
 * 启发式读感告警（三条规则，默认阈值见 `TENSION_WARNING_DEFAULTS`）：
 * - flat-middle：中段窗口 conflictLevel 极差 ≤ flatMaxRange 的最长连续段 ≥ flatWindow；
 * - climax-crowding：尾段（最大章号 ×(1−climaxWindow) 之后）revealLevel 高点数
 *   ≥ climaxMinCount 且严格大于尾段之外的高点数；
 * - weak-hook-streak：连续 weakWindow 个点 revealLevel ≤ weakLevel（取最长段）。
 * 点列为空直接返回空数组。
 */
export function detectTensionWarnings(
  points: ReadonlyArray<TensionPoint>,
  language: "zh" | "en" = "zh",
  options: TensionWarningOptions = {},
): TensionWarning[] {
  if (points.length === 0) return [];
  const opts = { ...TENSION_WARNING_DEFAULTS, ...options };
  const sorted = [...points].sort((a, b) => a.chapter - b.chapter);
  const total = sorted[sorted.length - 1]!.chapter;
  const middleLo = Math.ceil(total * opts.middleStart);
  const middleHi = Math.floor(total * opts.middleEnd);
  const tailLo = Math.floor(total * (1 - opts.climaxWindow)) + 1;
  const isEn = language === "en";
  const warnings: TensionWarning[] = [];

  const middlePoints = sorted.filter((p) => p.chapter >= middleLo && p.chapter <= middleHi);
  const flatRun = longestRangeWindow(middlePoints, opts.flatMaxRange);
  if (flatRun.length >= opts.flatWindow) {
    warnings.push({
      kind: "flat-middle",
      chapters: flatRun.map((p) => p.chapter),
      severity: "warning",
      description: isEn
        ? `Middle stretch (ch. ${flatRun[0]!.chapter}–${flatRun[flatRun.length - 1]!.chapter}) stays flat for ${flatRun.length} chapters (conflict range ≤ ${opts.flatMaxRange})`
        : `中段（第${flatRun[0]!.chapter}–${flatRun[flatRun.length - 1]!.chapter}章）连续${flatRun.length}章冲突强度平坦（极差≤${opts.flatMaxRange}）`,
      suggestion: isEn
        ? "Introduce an escalation or reversal mid-book so tension breathes."
        : "在中段插入一次冲突升级或反转，让张力有起伏。",
    });
  }

  const tailHighs = sorted.filter((p) =>
    p.revealLevel >= opts.climaxMinLevel && p.chapter >= tailLo
  );
  const earlyHighCount = sorted.filter((p) =>
    p.revealLevel >= opts.climaxMinLevel && p.chapter < tailLo
  ).length;
  if (tailHighs.length >= opts.climaxMinCount && tailHighs.length > earlyHighCount) {
    warnings.push({
      kind: "climax-crowding",
      chapters: tailHighs.map((p) => p.chapter),
      severity: "warning",
      description: isEn
        ? `Climactic reveals crowd the final stretch (${tailHighs.length} from ch. ${tailLo} onward vs ${earlyHighCount} earlier)`
        : `高潮揭示堆积尾段（第${tailLo}章起${tailHighs.length}处，早段仅${earlyHighCount}处）`,
      suggestion: isEn
        ? "Move some climactic reveals earlier to avoid a crowded ending."
        : "把部分高潮揭示前移到中段，避免结尾拥挤。",
    });
  }

  const weakRun = longestPredicateRun(sorted, (p) => p.revealLevel <= opts.weakLevel);
  if (weakRun.length >= opts.weakWindow) {
    warnings.push({
      kind: "weak-hook-streak",
      chapters: weakRun.map((p) => p.chapter),
      severity: "warning",
      description: isEn
        ? `Chapters ${weakRun[0]!.chapter}–${weakRun[weakRun.length - 1]!.chapter} end weak for ${weakRun.length} straight chapters (reveal level ≤ ${opts.weakLevel})`
        : `第${weakRun[0]!.chapter}–${weakRun[weakRun.length - 1]!.chapter}章连续${weakRun.length}章章末偏弱（揭示强度≤${opts.weakLevel}）`,
      suggestion: isEn
        ? "Add an open question or new hook to recent chapter endings."
        : "给最近章节的结尾加一个未解悬念或新钩子。",
    });
  }

  return warnings;
}

/** 曲线 + 告警一括聚合（端点/面板共用入口）。 */
export function analyzeTensionCurve(
  rows: ReadonlyArray<TensionRow>,
  language: "zh" | "en" = "zh",
  options: TensionWarningOptions = {},
): { curve: TensionCurve; warnings: TensionWarning[] } {
  const curve = buildTensionCurve(rows);
  return { curve, warnings: detectTensionWarnings(curve.points, language, options) };
}

/** 极差窗口：找最长连续段使段内 conflictLevel 极差 ≤ maxRange（双指针）。 */
function longestRangeWindow(
  points: ReadonlyArray<TensionPoint>,
  maxRange: number,
): TensionPoint[] {
  let bestStart = 0;
  let bestEnd = -1;
  let start = 0;
  for (let end = 0; end < points.length; end++) {
    while (start < end && windowRange(points, start, end) > maxRange) {
      start++;
    }
    if (end - start > bestEnd - bestStart) {
      bestStart = start;
      bestEnd = end;
    }
  }
  return points.slice(bestStart, bestEnd + 1);
}

function windowRange(
  points: ReadonlyArray<TensionPoint>,
  start: number,
  end: number,
): number {
  let min = Number.POSITIVE_INFINITY;
  let max = Number.NEGATIVE_INFINITY;
  for (let i = start; i <= end; i++) {
    const level = points[i]!.conflictLevel;
    if (level < min) min = level;
    if (level > max) max = level;
  }
  return max - min;
}

/** 全点满足谓词的最长连续段。 */
function longestPredicateRun(
  points: ReadonlyArray<TensionPoint>,
  predicate: (point: TensionPoint) => boolean,
): TensionPoint[] {
  let bestStart = 0;
  let bestEnd = -1;
  let start = -1;
  for (let i = 0; i <= points.length; i++) {
    if (i < points.length && predicate(points[i]!)) {
      if (start < 0) start = i;
    } else if (start >= 0) {
      if (i - 1 - start > bestEnd - bestStart) {
        bestStart = start;
        bestEnd = i - 1;
      }
      start = -1;
    }
  }
  return points.slice(bestStart, bestEnd + 1);
}
