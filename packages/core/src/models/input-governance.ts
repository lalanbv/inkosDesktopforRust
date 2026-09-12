import { z } from "zod";

/**
 * R1 读者体验合同（357 号）——memo「读者体验合同」节的结构化提取产物。
 *
 * 每个叙事字段限长 200 UTF-16 码元：解析器截断到该值后才入 schema，避免
 * LLM 偶发超长触发解析失败→重试风暴；提示词层面的软约束是每字段 ≤50 字。
 * titleCandidates 为章名候选（≤3 个，各 ≤60 码元），服务"目标 + 追读钩子"。
 */
export const ReaderExperienceSchema = z.object({
  previousHandoff: z.string().min(1).max(200),
  readerQuestion: z.string().min(1).max(200),
  promisePayoff: z.string().min(1).max(200),
  protagonistWant: z.string().min(1).max(200),
  protagonistObstacle: z.string().min(1).max(200),
  sceneTurn: z.string().min(1).max(200),
  endingNetChange: z.string().min(1).max(200),
  titleCandidates: z.array(z.string().min(1).max(60)).max(3).default([]),
});

export type ReaderExperience = z.infer<typeof ReaderExperienceSchema>;

export const ChapterMemoSchema = z.object({
  chapter: z.number().int().min(1),
  goal: z.string().min(1).max(50),
  isGoldenOpening: z.boolean().default(false),
  body: z.string().min(1),
  threadRefs: z.array(z.string()).default([]),
  readerExperience: ReaderExperienceSchema.optional(),
});

export type ChapterMemo = z.infer<typeof ChapterMemoSchema>;

export const ChapterIntentSchema = z.object({
  chapter: z.number().int().min(1),
  goal: z.string().min(1),
  outlineNode: z.string().optional(),
  arcContext: z.string().optional(),
  mustKeep: z.array(z.string()).default([]),
  mustAvoid: z.array(z.string()).default([]),
  styleEmphasis: z.array(z.string()).default([]),
});

export type ChapterIntent = z.infer<typeof ChapterIntentSchema>;

export const ContextSourceSchema = z.object({
  source: z.string().min(1),
  reason: z.string().min(1),
  excerpt: z.string().optional(),
});

export type ContextSource = z.infer<typeof ContextSourceSchema>;

export const ContextPackageSchema = z.object({
  chapter: z.number().int().min(1),
  selectedContext: z.array(ContextSourceSchema).default([]),
});

export type ContextPackage = z.infer<typeof ContextPackageSchema>;

export const RuleLayerScopeSchema = z.enum(["global", "book", "arc", "local"]);
export type RuleLayerScope = z.infer<typeof RuleLayerScopeSchema>;

export const RuleLayerSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  precedence: z.number().int(),
  scope: RuleLayerScopeSchema,
});

export type RuleLayer = z.infer<typeof RuleLayerSchema>;

export const OverrideEdgeSchema = z.object({
  from: z.string().min(1),
  to: z.string().min(1),
  allowed: z.boolean(),
  scope: z.string().min(1),
});

export type OverrideEdge = z.infer<typeof OverrideEdgeSchema>;

export const ActiveOverrideSchema = z.object({
  from: z.string().min(1),
  to: z.string().min(1),
  target: z.string().min(1),
  reason: z.string().min(1),
});

export type ActiveOverride = z.infer<typeof ActiveOverrideSchema>;

export const RuleStackSectionsSchema = z.object({
  hard: z.array(z.string()).default([]),
  soft: z.array(z.string()).default([]),
  diagnostic: z.array(z.string()).default([]),
});

export type RuleStackSections = z.infer<typeof RuleStackSectionsSchema>;

export const RuleStackSchema = z.object({
  layers: z.array(RuleLayerSchema).min(1),
  sections: RuleStackSectionsSchema.default({
    hard: [],
    soft: [],
    diagnostic: [],
  }),
  overrideEdges: z.array(OverrideEdgeSchema).default([]),
  activeOverrides: z.array(ActiveOverrideSchema).default([]),
});

export type RuleStack = z.infer<typeof RuleStackSchema>;

export const ChapterTraceSchema = z.object({
  chapter: z.number().int().min(1),
  plannerInputs: z.array(z.string()),
  composerInputs: z.array(z.string()),
  selectedSources: z.array(z.string()),
  promptPacks: z.array(z.string()).default([]),
  contextTiers: z.object({
    protectedSources: z.array(z.string()).default([]),
    compressibleSources: z.array(z.string()).default([]),
  }).default({
    protectedSources: [],
    compressibleSources: [],
  }),
  tokenBudget: z.object({
    protectedTokens: z.number().int().nonnegative().default(0),
    compressibleTokens: z.number().int().nonnegative().default(0),
    totalSelectedTokens: z.number().int().nonnegative().default(0),
  }).default({
    protectedTokens: 0,
    compressibleTokens: 0,
    totalSelectedTokens: 0,
  }),
  compression: z.object({
    compiledSource: z.string().min(1),
    protectedSources: z.array(z.string()).default([]),
    compressedSources: z.array(z.string()).default([]),
    protectedTokens: z.number().int().nonnegative().default(0),
    compressibleTokens: z.number().int().nonnegative().default(0),
    budgetTokens: z.number().int().nonnegative().default(0),
  }).optional(),
  retrieval: z.object({
    engine: z.literal("sqlite-fts5-bm25"),
    query: z.string(),
    candidates: z.array(z.object({
      id: z.string(),
      kind: z.string(),
      source: z.string(),
      score: z.number(),
    })),
    semanticSelectedIds: z.array(z.string()).optional(),
  }).optional(),
  notes: z.array(z.string()).default([]),
});

export type ChapterTrace = z.infer<typeof ChapterTraceSchema>;
