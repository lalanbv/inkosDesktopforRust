import type { ChapterTrace, ContextPackage } from "../models/input-governance.js";
import { estimateTextTokens } from "../llm/provider.js";
import { isProtectedContextSource } from "./context-assembly.js";
import {
  contextSourceTier,
  contextSourceTierDescriptor,
} from "./context-source-tier.js";

/** 层内排序特征透传形状（对齐 Rust ContextSourceRank：缺省维度省略）。 */
export interface ContextLensRank {
  readonly recency?: number;
  readonly frequency?: number;
  readonly hookBonus?: number;
}

/**
 * Context Lens——上下文装配透明投影（R21/391 号，四轮 P0）。
 *
 * 对标 Novelcrafter Codex 视图 / WNW 装配清单的"让作者看见模型看见了什么"：
 * 每章治理装配落盘的两件工件（`chapter-NNNN.context.json` 实际进入 prompt 的
 * 包 + `chapter-NNNN.trace.json` 治理留痕）在这里投影成单一只读视图：
 *
 *   - entries 以 context.json 为准（预算后真况），order=1 起装配序；
 *   - tier/precedence 按来源分类表现算（不信任留痕），protected 独立判定；
 *   - tokens 复用 367 号估算口径（estimateTextTokens），零新增计数器；
 *   - 压缩发生时包里只剩受保护条目 + 单条编译产物，被编译掉的原始来源与
 *     压缩前 token 从 trace.compression.sourceTokens 透传（preCompressionSources）。
 *
 * 纯函数零 IO——写作链不感知本模块；端点读盘后投影，UI 渲染。
 * 双端契约：`engine-rs/src/utils/context_lens.rs` 1:1 镜像；共享向量
 * `golden/context-lens-vectors.json`（TS：`golden-context-lens.test.ts`；
 * Rust 差分：`tests/golden_context_lens_diff.rs`）。
 */

export const CONTEXT_LENS_VERSION = 1;

export interface ContextLensEntry {
  /** 1 起装配序（context.json 数组序，即 G2 优先级契约的最终序）。 */
  readonly order: number;
  readonly source: string;
  readonly tier: ReturnType<typeof contextSourceTier>;
  readonly tierPrecedence: number;
  readonly protected: boolean;
  readonly tokens: number;
  readonly compiled: boolean;
  readonly rank: ContextLensRank | null;
}

export interface ContextLensCompression {
  readonly compiledSource: string;
  readonly budgetTokens: number;
  readonly protectedTokens: number;
  readonly compressibleTokens: number;
  /** 压缩前被编译的原始可压缩来源与逐源 token（trace.compression.sourceTokens 透传）。 */
  readonly preCompressionSources: ReadonlyArray<{ readonly source: string; readonly tokens: number }>;
}

export interface ContextLens {
  readonly version: typeof CONTEXT_LENS_VERSION;
  readonly chapter: number;
  readonly entries: ReadonlyArray<ContextLensEntry>;
  readonly compression: ContextLensCompression | null;
  readonly notes: ReadonlyArray<string>;
  readonly totals: {
    readonly entries: number;
    readonly protectedEntries: number;
    readonly compiledEntries: number;
    readonly tokens: number;
  };
}

/** 上下文装配透明投影：context.json（实况）× trace.json（留痕）→ 单一只读视图。 */
export function buildContextLens(params: {
  readonly contextPackage: ContextPackage;
  readonly trace: ChapterTrace;
}): ContextLens {
  const compression = params.trace.compression ?? null;
  const entries = params.contextPackage.selectedContext.map((entry, index) => {
    const tier = contextSourceTier(entry.source);
    return {
      order: index + 1,
      source: entry.source,
      tier,
      tierPrecedence: contextSourceTierDescriptor(tier).precedence,
      protected: isProtectedContextSource(entry.source),
      tokens: estimateContextLensSourceTokens(entry),
      compiled: compression?.compiledSource === entry.source,
      rank: entry.rank ?? null,
    };
  });
  return {
    version: CONTEXT_LENS_VERSION,
    chapter: params.contextPackage.chapter,
    entries,
    compression: compression
      ? {
          compiledSource: compression.compiledSource,
          budgetTokens: compression.budgetTokens,
          protectedTokens: compression.protectedTokens,
          compressibleTokens: compression.compressibleTokens,
          preCompressionSources: compression.sourceTokens,
        }
      : null,
    notes: params.trace.notes,
    totals: {
      entries: entries.length,
      protectedEntries: entries.filter((entry) => entry.protected).length,
      compiledEntries: entries.filter((entry) => entry.compiled).length,
      tokens: entries.reduce((total, entry) => total + entry.tokens, 0),
    },
  };
}

/** 与 context-assembly 装配留痕同口径（source+reason+excerpt，空段剔除）。 */
function estimateContextLensSourceTokens(entry: ContextPackage["selectedContext"][number]): number {
  return estimateTextTokens([entry.source, entry.reason, entry.excerpt].filter(Boolean).join("\n"));
}
