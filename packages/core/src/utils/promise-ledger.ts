import type { StoredHook } from "../state/memory-db.js";
import type { HookKind } from "../models/runtime-state.js";

/**
 * 承诺账本运营化（G10/338 号，Phase B 批次一收尾；WNW W7 承诺统一账本思想的采纳）。
 *
 * 伏笔/悬念/感情线/爽点本质都是"对读者的承诺"。hooks 账本已存了承诺的
 * 全生命周期字段（startChapter 开启 / lastAdvancedChapter 推进 / status 兑付 /
 * coreHook 核心承诺 / expectedPayoff 预期兑现），本模块补齐**运营层**：
 *
 * 1. **节奏债告警** `detectPacingDebts`：core_hook（核心承诺）搁置超 N 章告警
 *    ——普通伏笔可以慢，核心承诺欠账必须显眼；
 * 2. **连续弱钩检测** `detectWeakHookRuns`：从章摘要的 hookActivity 列推导
 *    连续无钩动静的章段（≥N 章连续 = 节奏平淡预警）；
 * 3. **承诺时间线** `buildPromiseTimeline`：每条承诺的开启/推进/兑付状态投影。
 *
 * 双端：`engine-rs/src/utils/promise_ledger.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/promise-ledger-vectors.json`（TS 断言
 * `golden-promise-ledger.test.ts`；Rust 差分 `tests/golden_promise_ledger_diff.rs`）。
 */

export const DEFAULT_CORE_HOOK_STALLED_THRESHOLD = 5;
export const DEFAULT_WEAK_HOOK_MIN_RUN = 3;

/** hookActivity 文本强度：strong=有具体钩子动静；weak=提到但不具体；none=无。 */
export type HookActivityStrength = "strong" | "weak" | "none";

const NONE_PATTERNS = /^(无|没有|暂无|none|n\/a|-|—|–)$/i;
const STRONG_PATTERNS =
  /(hook|h\d+|fk\d+|伏笔|钩子|回收|埋|推进|兑现|resolve|plant|advance)/i;
const ID_TOKEN_PATTERN = /[a-z]*\d+[a-z]*/i;

export function hookActivityStrength(text: string): HookActivityStrength {
  const normalized = text.trim();
  if (!normalized || NONE_PATTERNS.test(normalized)) return "none";
  if (STRONG_PATTERNS.test(normalized) || ID_TOKEN_PATTERN.test(normalized)) return "strong";
  return "weak";
}

const RESOLVED_PATTERN = /^(resolved|closed|done|已回收|已解决)$/i;

function isFulfilled(hook: StoredHook): boolean {
  return RESOLVED_PATTERN.test((hook.status ?? "").trim());
}

/** 从 expectedPayoff 文本提取期望兑现章号（"第10章" / 裸数字），提取不到返回 undefined。 */
export function parseExpectedChapter(expectedPayoff: string): number | undefined {
  const trimmed = (expectedPayoff ?? "").trim();
  if (!trimmed) return undefined;
  const zh = /第\s*(\d+)\s*章/.exec(trimmed);
  if (zh) return Number.parseInt(zh[1]!, 10);
  const bare = /(?:^|[^\d])(\d{1,4})(?:[^\d]|$)/.exec(trimmed);
  if (bare) return Number.parseInt(bare[1]!, 10);
  return undefined;
}

export interface PacingDebtAlert {
  readonly hookId: string;
  /** 已搁置章数：currentChapter - max(lastAdvancedChapter, startChapter)。 */
  readonly stalledChapters: number;
  readonly threshold: number;
  readonly expectedPayoff: string;
}

/** 节奏债告警：只盯核心承诺（coreHook）——搁置 ≥ threshold 章即告警（新→旧排序）。 */
export function detectPacingDebts(params: {
  readonly hooks: ReadonlyArray<StoredHook>;
  readonly currentChapter: number;
  readonly threshold?: number;
}): PacingDebtAlert[] {
  const threshold = params.threshold ?? DEFAULT_CORE_HOOK_STALLED_THRESHOLD;
  const alerts: PacingDebtAlert[] = [];
  for (const hook of params.hooks) {
    if (hook.coreHook !== true) continue;
    if (isFulfilled(hook)) continue;
    const anchor = Math.max(hook.lastAdvancedChapter || 0, hook.startChapter || 0);
    const stalledChapters = Math.max(0, params.currentChapter - anchor);
    if (stalledChapters >= threshold) {
      alerts.push({
        hookId: hook.hookId,
        stalledChapters,
        threshold,
        expectedPayoff: hook.expectedPayoff ?? "",
      });
    }
  }
  return alerts.sort((a, b) => b.stalledChapters - a.stalledChapters || a.hookId.localeCompare(b.hookId));
}

export interface WeakHookRun {
  readonly fromChapter: number;
  readonly toChapter: number;
  /** 连续弱章数（weak + none 都计入）。 */
  readonly length: number;
}

export interface WeakHookSummary {
  readonly runs: ReadonlyArray<WeakHookRun>;
  readonly longestRun: number;
}

/** 连续弱钩检测：hookActivity 非 strong 的连续章段（按章号升序扫描），只返回 ≥ minRun 的段。 */
export function detectWeakHookRuns(params: {
  readonly summaries: ReadonlyArray<{ readonly chapter: number; readonly hookActivity: string }>;
  readonly minRunLength?: number;
}): WeakHookSummary {
  const minRunLength = params.minRunLength ?? DEFAULT_WEAK_HOOK_MIN_RUN;
  const ordered = [...params.summaries].sort((a, b) => a.chapter - b.chapter);

  const runs: WeakHookRun[] = [];
  let runStart: number | undefined;
  let runEnd: number | undefined;
  const closeRun = () => {
    if (runStart !== undefined && runEnd !== undefined && runEnd >= runStart) {
      const length = runEnd - runStart + 1;
      if (length >= minRunLength) runs.push({ fromChapter: runStart, toChapter: runEnd, length });
    }
    runStart = undefined;
    runEnd = undefined;
  };
  for (const summary of ordered) {
    if (hookActivityStrength(summary.hookActivity) === "strong") {
      closeRun();
      continue;
    }
    // 章号不连续（缺章）视作段落中断——"连续"以章号严格递增为准。
    if (runStart !== undefined && runEnd !== undefined && summary.chapter !== runEnd + 1) {
      closeRun();
    }
    if (runStart === undefined) runStart = summary.chapter;
    runEnd = summary.chapter;
  }
  closeRun();

  const longestRun = runs.reduce((max, run) => Math.max(max, run.length), 0);
  return { runs, longestRun };
}

export interface PromiseTimelineEntry {
  readonly hookId: string;
  /** 承诺内容（notes 截断 40 字）。 */
  readonly summary: string;
  /** 开启章（startChapter，0 表示未知）。 */
  readonly openedAt: number;
  readonly lastAdvancedAt?: number;
  readonly expectedPayoff: string;
  readonly state: "open" | "advancing" | "fulfilled" | "overdue";
  /** R23/396 号：规范类型分类透传（存量无 kind 不出键）。 */
  readonly kind?: HookKind;
}

const SUMMARY_MAX_CHARS = 40;

/** 承诺时间线：开启/推进/兑付状态投影（未兑付在前，逾期置顶，按 startChapter 升序稳定）。 */
export function buildPromiseTimeline(
  hooks: ReadonlyArray<StoredHook>,
  currentChapter: number,
): ReadonlyArray<PromiseTimelineEntry> {
  const entries: PromiseTimelineEntry[] = hooks.map((hook) => {
    const lastAdvancedAt = hook.lastAdvancedChapter > 0 ? hook.lastAdvancedChapter : undefined;
    let state: PromiseTimelineEntry["state"];
    if (isFulfilled(hook)) {
      state = "fulfilled";
    } else {
      const expected = parseExpectedChapter(hook.expectedPayoff ?? "");
      if (expected !== undefined && currentChapter >= expected) state = "overdue";
      else if (lastAdvancedAt !== undefined) state = "advancing";
      else state = "open";
    }
    const summarySource = (hook.notes ?? "").trim() || (hook.expectedPayoff ?? "").trim();
    const summary = summarySource.length > SUMMARY_MAX_CHARS
      ? `${summarySource.slice(0, SUMMARY_MAX_CHARS - 1)}…`
      : summarySource;
    return {
      hookId: hook.hookId,
      summary,
      openedAt: hook.startChapter,
      lastAdvancedAt,
      expectedPayoff: hook.expectedPayoff ?? "",
      state,
      ...(hook.kind ? { kind: hook.kind } : {}),
    };
  });

  const stateRank: Record<PromiseTimelineEntry["state"], number> = {
    overdue: 0,
    open: 1,
    advancing: 2,
    fulfilled: 3,
  };
  return entries.sort(
    (a, b) =>
      stateRank[a.state] - stateRank[b.state]
      || a.openedAt - b.openedAt
      || a.hookId.localeCompare(b.hookId),
  );
}

// ---------------------------------------------------------------------------
// R3/360 号：数值紧迫度（WNW urgency 映射）+ 目标章窗 + 置信度。
// ---------------------------------------------------------------------------

export type UrgencyLevel = "high" | "medium" | "low";
export type UrgencyConfidence = "explicit" | "inferred" | "unknown";

export interface PromiseUrgency {
  readonly hookId: string;
  /** 0–100：逾期 100 / 剩 1–3 章 80 / 剩 4–10 章 60 / 剩 >10 或推进中无目标 40 / 开启无目标 20 / 已兑付 0。 */
  readonly urgency: number;
  readonly level: UrgencyLevel;
  /** 目标章来源：expectedPayoff 显式提取 / 书末兜底推断 / 无目标。 */
  readonly confidence: UrgencyConfidence;
  readonly targetChapter?: number;
  /** 显式目标 ±2 章缓冲；推断目标 = [当前章, 书末]。 */
  readonly targetWindow?: { readonly start: number; readonly end: number };
}

export const URGENCY_LEVEL_HIGH_THRESHOLD = 80;
export const URGENCY_LEVEL_MEDIUM_THRESHOLD = 40;

/**
 * 单条承诺的数值紧迫度（纯函数，golden 锚定）：
 * - 目标章优先级：expectedPayoff 显式提取（explicit）> targetChapters 书末兜底（inferred）> 无（unknown）；
 * - 分档：remaining ≤0 → 100；1–3 → 80；4–10 → 60；>10 → 40；无目标时
 *   advancing → 40、open → 20；fulfilled → 0；
 * - level：≥80 high / ≥40 medium / 其余 low。
 */
export function resolvePromiseUrgency(params: {
  readonly hook: StoredHook;
  readonly currentChapter: number;
  readonly targetChapters?: number;
}): PromiseUrgency {
  const hook = params.hook;
  if (isFulfilled(hook)) {
    return { hookId: hook.hookId, urgency: 0, level: "low", confidence: "unknown" };
  }

  const explicit = parseExpectedChapter(hook.expectedPayoff ?? "");
  let target: number | undefined;
  let confidence: UrgencyConfidence;
  let targetWindow: PromiseUrgency["targetWindow"];
  if (explicit !== undefined) {
    target = explicit;
    confidence = "explicit";
    targetWindow = { start: Math.max(1, explicit - 2), end: explicit + 2 };
  } else if (typeof params.targetChapters === "number" && params.targetChapters > 0) {
    target = params.targetChapters;
    confidence = "inferred";
    targetWindow = { start: Math.max(1, params.currentChapter), end: params.targetChapters };
  } else {
    confidence = "unknown";
  }

  let urgency: number;
  if (target === undefined) {
    urgency = hook.lastAdvancedChapter > 0 ? 40 : 20;
  } else {
    const remaining = target - params.currentChapter;
    if (remaining <= 0) urgency = 100;
    else if (remaining <= 3) urgency = 80;
    else if (remaining <= 10) urgency = 60;
    else urgency = 40;
  }

  const level: UrgencyLevel = urgency >= URGENCY_LEVEL_HIGH_THRESHOLD
    ? "high"
    : urgency >= URGENCY_LEVEL_MEDIUM_THRESHOLD
      ? "medium"
      : "low";

  return {
    hookId: hook.hookId,
    urgency,
    level,
    confidence,
    ...(target !== undefined ? { targetChapter: target } : {}),
    ...(targetWindow !== undefined ? { targetWindow } : {}),
  };
}
