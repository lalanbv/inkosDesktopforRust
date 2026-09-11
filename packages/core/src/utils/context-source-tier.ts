import { z } from "zod";

/**
 * 上下文来源优先级契约（G2，对标 ANWA A1 六层来源分级 + WNW W1 任务书定序）。
 *
 * Selected Context 的组装顺序即模型的"隐性权威排序"：排在前面的来源更容易
 * 赢得注意力。本契约把这件事从"数组拼接的偶然"固化为显式分层：
 *
 *   本书事实(100) > 本书规划(80) > 本书记忆(60) > 参考资料(40)
 *     > 拆书结论(30, 预留 G5) > 写法资产(20, 预留 G4) > 临时/未注册(10)
 *
 * 核心不变量：**参考资料及更低层不得覆盖本书事实与章纲**——参考资料段只
 * 提供素材，与 `reference-context.ts` 写出的逐条 disclaimer、RuleStack 的
 * overrideEdges（仅 L4→L3 允许）共同构成三重守卫。本书记忆层是本项目对
 * ANWA 六层的自有插入（时序事实可压缩、 informs 但不改写规划）。
 *
 * 双端契约：`engine-rs/src/utils/context_source_tier.rs` 为 1:1 镜像，
 * 共享向量见 `src/__tests__/golden/context-priority-vectors.json`
 * （TS 断言：`golden-context-priority.test.ts`；Rust 差分：
 * `engine-rs/tests/golden_context_priority_diff.rs`）。
 */

export const CONTEXT_SOURCE_TIER_IDS = [
  "book-fact",
  "book-planning",
  "book-memory",
  "user-reference",
  "deconstruction",
  "style-asset",
  "ephemeral",
] as const;

export type ContextSourceTierId = (typeof CONTEXT_SOURCE_TIER_IDS)[number];

export const ContextSourceTierIdSchema = z.enum(CONTEXT_SOURCE_TIER_IDS);

const ContextSourceTierDescriptorSchema = z.object({
  id: ContextSourceTierIdSchema,
  /** 数值越大优先级越高；对齐 RuleStack precedence：100=hard_facts(L1)、80=author_intent(L2)。 */
  precedence: z.number().int(),
  /** true = 预算压缩时不可压缩（须与 context-assembly.isProtectedContextSource 一致）。 */
  protected: z.boolean(),
  labelZh: z.string().min(1),
  labelEn: z.string().min(1),
});

export type ContextSourceTierDescriptor = z.infer<typeof ContextSourceTierDescriptorSchema>;

export const CONTEXT_SOURCE_TIERS: readonly ContextSourceTierDescriptor[] = [
  { id: "book-fact", precedence: 100, protected: true, labelZh: "本书事实", labelEn: "Book facts" },
  { id: "book-planning", precedence: 80, protected: true, labelZh: "本书规划", labelEn: "Book planning" },
  { id: "book-memory", precedence: 60, protected: false, labelZh: "本书时序记忆", labelEn: "Book memory" },
  { id: "user-reference", precedence: 40, protected: false, labelZh: "参考资料", labelEn: "User references" },
  { id: "deconstruction", precedence: 30, protected: false, labelZh: "拆书结论", labelEn: "Deconstruction" },
  { id: "style-asset", precedence: 20, protected: false, labelZh: "写法资产", labelEn: "Style assets" },
  { id: "ephemeral", precedence: 10, protected: false, labelZh: "临时/未注册", labelEn: "Ephemeral" },
];

const TIER_BY_ID = new Map(CONTEXT_SOURCE_TIERS.map((tier) => [tier.id, tier]));

export function contextSourceTierDescriptor(id: ContextSourceTierId): ContextSourceTierDescriptor {
  const tier = TIER_BY_ID.get(id);
  if (!tier) throw new Error(`unknown context source tier: ${id}`);
  return tier;
}

export function contextSourceTierPrecedence(id: ContextSourceTierId): number {
  return contextSourceTierDescriptor(id).precedence;
}

/**
 * 与 `isProtectedContextSource` 的契约锚点：protected 层的成员集合。
 * golden 测试锁死两者一致——任何一侧单独漂移都会被测试抓住。
 */
export const PROTECTED_CONTEXT_SOURCE_TIERS: ReadonlySet<ContextSourceTierId> = new Set(
  CONTEXT_SOURCE_TIERS.filter((tier) => tier.protected).map((tier) => tier.id),
);

// ── 来源分类表（组装链 producer 全集，见 composer.collectSelectedContext /
//    reference-context.splitReferenceSections；新增来源必须在此注册层级）──

/** 整文件级事实来源（story_bible/volume_outline 为 Phase 5 旧名，保留兼容）。 */
const EXACT_PLANNING_SOURCES: ReadonlySet<string> = new Set([
  "story/author_intent.md",
  "story/current_focus.md",
  "story/audit_drift.md",
  "story/outline/volume_map.md",
  "story/volume_outline.md",
  "runtime/chapter_memo",
]);

const EXACT_FACT_SOURCES: ReadonlySet<string> = new Set([
  "story/story_bible.md",
  "story/outline/story_frame.md",
  "story/parent_canon.md",
  "story/fanfic_canon.md",
]);

/**
 * ContextPackage 来源 → 层级。总函数：未注册来源一律 ephemeral（排序垫底、
 * 可压缩），绝不臆测其权威性——新来源上线前必须显式注册层级。
 */
export function contextSourceTier(source: string): ContextSourceTierId {
  if (EXACT_PLANNING_SOURCES.has(source)) return "book-planning";
  if (EXACT_FACT_SOURCES.has(source)) return "book-fact";
  if (source.startsWith("story/outline/story_frame.md#")) return "book-fact";
  if (source.startsWith("story/outline/volume_map.md#")) return "book-planning";
  // outlineFallback 的旧版文件名（含 #section 锚点）语义不变：仍是章纲/正典段。
  if (source.startsWith("story/story_bible.md")) return "book-fact";
  if (source.startsWith("story/volume_outline.md")) return "book-planning";
  if (source.startsWith("story/current_state.md")) return "book-fact";
  if (source.startsWith("story/pending_hooks.md#")) return "book-fact";
  if (source.startsWith("runtime/hook_debt#")) return "book-fact";
  if (source.startsWith("story/chapter_summaries.md#")) return "book-memory";
  if (source.startsWith("story/volume_summaries.md#")) return "book-memory";
  if (source === "story/chapters#recent_endings") return "book-memory";
  if (source.startsWith("reference/")) return "user-reference";
  if (source.startsWith("deconstruction/")) return "deconstruction";
  if (source.startsWith("style/")) return "style-asset";
  return "ephemeral";
}

/**
 * 组装固化入口：按层级 precedence 稳定排序（同层保持组装序不变）。
 * composeGovernedChapter 在合并 story 证据与参考资料后调用，使
 * Selected Context 的渲染顺序 == 优先级契约。
 */
export function enforceContextPriorityOrder<T extends { readonly source: string }>(
  entries: readonly T[],
): T[] {
  return entries
    .map((entry, index) => ({ entry, index }))
    .sort((left, right) => {
      const delta = contextSourceTierPrecedence(contextSourceTier(right.entry.source))
        - contextSourceTierPrecedence(contextSourceTier(left.entry.source));
      return delta !== 0 ? delta : left.index - right.index;
    })
    .map(({ entry }) => entry);
}

// ── 机器可读契约（双端 golden 锁形状）──

const ContextSourcePriorityContractSchema = z.object({
  version: z.literal(1),
  rule: z.literal("higherPrecedenceNumberIsHigherPriority"),
  layers: z.array(ContextSourceTierDescriptorSchema),
  overrideRules: z.array(z.string().min(1)).min(1),
  unknownSourceTier: ContextSourceTierIdSchema,
});

export type ContextSourcePriorityContract = z.infer<typeof ContextSourcePriorityContractSchema>;

export const CONTEXT_SOURCE_PRIORITY_CONTRACT: ContextSourcePriorityContract =
  ContextSourcePriorityContractSchema.parse({
    version: 1,
    rule: "higherPrecedenceNumberIsHigherPriority",
    layers: CONTEXT_SOURCE_TIERS,
    overrideRules: [
      // G2 核心断言：参考资料（及更低层）不得覆盖本书事实与章纲段。
      "reference-and-below-cannot-override-fact-or-planning",
      "memory-informs-but-cannot-override-planning",
      "planning-narrows-fact-only-via-explicit-override-edges",
      "unknown-sources-are-ephemeral-and-sort-last",
    ],
    unknownSourceTier: "ephemeral",
  });
