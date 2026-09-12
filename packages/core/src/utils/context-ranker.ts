/**
 * R6 上下文排序器 + 组装补强（367 号契约层，二轮 P2；355 号 §4 R6）。
 *
 * 1. **层内确定性排序**：G2 层间序（330 号 enforceContextPriorityOrder）之上，
 *    同层内按 `recency×wR + frequency×wF + hookBonus×wH` 降序稳定排序，权重
 *    可配；无特征条目 score=0 保持原序（既有行为逐字节不变）。debug 分随
 *    `rankScore` 附在条目上，调用方写入 context log/trace notes。
 * 2. **openingHint**：上章结尾承接约束块（写作者显式指令 + 结尾摘录）。
 *
 * 双端：`engine-rs/src/utils/context_ranker.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/context-ranker-vectors.json`（TS 断言
 * `golden-context-ranker.test.ts`；Rust 差分 `tests/golden_context_ranker_diff.rs`）。
 */

import { contextSourceTier, contextSourceTierPrecedence } from "./context-source-tier.js";

export interface RankerWeights {
  readonly recency: number;
  readonly frequency: number;
  readonly hookBonus: number;
}

export const DEFAULT_RANKER_WEIGHTS: RankerWeights = {
  recency: 0.5,
  frequency: 0.3,
  hookBonus: 0.2,
};

/** 可排序条目特征（缺省视为 0——无特征条目 score=0，保持组装序）。 */
export interface RankFeatures {
  /** 0–1：越近越高（如 1 − 距当前章章数/窗口）。 */
  readonly recency?: number;
  /** 0–1：出现频率归一。 */
  readonly frequency?: number;
  /** 0/1：含钩子动静加成（hookActivityStrength=strong）。 */
  readonly hookBonus?: number;
}

export interface RankableSourceEntry extends RankFeatures {
  readonly source: string;
}

export interface RankedEntry<T> {
  readonly entry: T;
  /** 调试分（recency×wR + frequency×wF + hookBonus×wH，4 位小数）。 */
  readonly rankScore: number;
}

function clamp01(value: number | undefined): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return 0;
  return Math.min(1, Math.max(0, value));
}

/** 条目得分（4 位小数；缺特征维度计 0）。 */
export function rankScore(entry: RankFeatures, weights: RankerWeights = DEFAULT_RANKER_WEIGHTS): number {
  const raw =
    clamp01(entry.recency) * weights.recency +
    clamp01(entry.frequency) * weights.frequency +
    clamp01(entry.hookBonus) * weights.hookBonus;
  return Math.round(raw * 10000) / 10000;
}

/**
 * 层内排序：score 降序稳定排序（同分保持输入序；禁 localeCompare）。
 * 输入顺序应为该层内的组装序。
 */
export function rankWithinTier<T extends RankFeatures>(
  entries: ReadonlyArray<T>,
  weights: RankerWeights = DEFAULT_RANKER_WEIGHTS,
): Array<RankedEntry<T>> {
  return entries
    .map((entry, index) => ({ entry, index, rankScore: rankScore(entry, weights) }))
    .sort((left, right) => right.rankScore - left.rankScore || left.index - right.index)
    .map(({ entry, rankScore }) => ({ entry, rankScore }));
}

/**
 * 组装排序入口：G2 层间 precedence 之上做层内确定性排序——
 * 按层分组（contextSourceTier）→ 组内 rankWithinTier → 层间按 precedence
 * 升序拼回。同层内无特征条目 score=0 沉底但保持相对序（与 330 号既有
 * 行为兼容：无特征时逐字节等于 enforceContextPriorityOrder 的输出序）。
 */
export function rankEntriesForComposition<T extends RankableSourceEntry>(
  entries: ReadonlyArray<T>,
  weights: RankerWeights = DEFAULT_RANKER_WEIGHTS,
): Array<RankedEntry<T>> {
  const tiers = new Map<string, { precedence: number; items: T[] }>();
  for (const entry of entries) {
    const tier = contextSourceTier(entry.source);
    if (!tiers.has(tier)) {
      tiers.set(tier, { precedence: contextSourceTierPrecedence(tier), items: [] });
    }
    tiers.get(tier)!.items.push(entry);
  }
  const ordered: Array<RankedEntry<T>> = [];
  // 层间 precedence 降序（对齐 330 号：事实 100 在前，ephemeral 10 垫底）。
  for (const tier of [...tiers.values()].sort((a, b) => b.precedence - a.precedence)) {
    ordered.push(...rankWithinTier(tier.items, weights));
  }
  return ordered;
}

// ── openingHint：上章结尾承接约束 ──

const OPENING_HINT_MAX_CHARS = 200;

/**
 * 上章结尾承接约束块：显式指令 + 结尾摘录（≤200 码元）。
 * previousEnding 为空/空白返回 undefined（首章/无上文零打扰）。
 */
export function buildOpeningHint(
  previousEnding: string,
  language: "zh" | "en" = "zh",
  maxChars: number = OPENING_HINT_MAX_CHARS,
): string | undefined {
  const trimmed = previousEnding.trim();
  if (!trimmed) return undefined;
  const clipped =
    trimmed.length > maxChars ? `…${trimmed.slice(trimmed.length - maxChars)}` : trimmed;
  if (language === "en") {
    return [
      "## Opening constraint (carry over the previous ending)",
      "The first paragraphs of this chapter must pick up exactly where the previous chapter ended — same scene, same tension, no reset.",
      `Previous ending:\n${clipped}`,
    ].join("\n");
  }
  return [
    "## 开头承接约束（上章结尾）",
    "本章开头必须真实接住上一章结尾——同一场景、同一张力，不得重置或跳过。",
    `上章结尾：\n${clipped}`,
  ].join("\n");
}
