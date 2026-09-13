import { z } from "zod";

export const RuntimeStateLanguageSchema = z.enum(["zh", "en"]);
export type RuntimeStateLanguage = z.infer<typeof RuntimeStateLanguageSchema>;

export const StateManifestSchema = z.object({
  schemaVersion: z.literal(2),
  language: RuntimeStateLanguageSchema,
  lastAppliedChapter: z.number().int().min(0),
  projectionVersion: z.number().int().min(1),
  migrationWarnings: z.array(z.string()).default([]),
});

export type StateManifest = z.infer<typeof StateManifestSchema>;

export const HookStatusSchema = z.enum(["open", "progressing", "deferred", "resolved"]);
export type HookStatus = z.infer<typeof HookStatusSchema>;

export const HookPayoffTimingSchema = z.enum([
  "immediate",
  "near-term",
  "mid-arc",
  "slow-burn",
  "endgame",
]);
export type HookPayoffTiming = z.infer<typeof HookPayoffTimingSchema>;

/**
 * R23/393 号：伏笔类型规范分类（≤7 类不做数量战，对标蛙趣 8 类伏笔生命
 * 周期的取精版）。可选字段——存量 hook 无 kind 完全兼容（零迁移）；新
 * hook 由 settler/architect 产出到 `hook-kind.ts` 的别名表归一化。
 */
export const HookKindSchema = z.enum([
  "promise",
  "suspense",
  "crisis",
  "artifact",
  "information",
  "emotion",
  "worldview",
]);
export type HookKind = z.infer<typeof HookKindSchema>;

export const HookRecordSchema = z.object({
  hookId: z.string().min(1),
  startChapter: z.number().int().min(0),
  type: z.string().min(1),
  status: HookStatusSchema,
  /** R23/393 号：规范类型分类（可选；存量无 kind 兼容，零迁移）。 */
  kind: HookKindSchema.optional(),
  lastAdvancedChapter: z.number().int().min(0),
  expectedPayoff: z.string().default(""),
  payoffTiming: HookPayoffTimingSchema.optional(),
  notes: z.string().default(""),
  // Phase 7 — hook causality / promotion metadata.
  // All optional so hooks parsed from pre-Phase-7 markdown still validate
  // and so callers constructing HookRecord inline can omit them.
  dependsOn: z.array(z.string().min(1)).optional(),
  paysOffInArc: z.string().optional(),
  coreHook: z.boolean().optional(),
  halfLifeChapters: z.number().int().positive().optional(),
  advancedCount: z.number().int().min(0).optional(),
  // Phase 7 hotfix 2 — promotion flag. Undefined on legacy 11/12-column
  // ledgers; architect-seed and consolidator-rerun both populate it going
  // forward. Reviewer uses it to gate critical severity for stale hooks.
  promoted: z.boolean().optional(),
});

export type HookRecord = z.infer<typeof HookRecordSchema>;

export const HooksStateSchema = z.object({
  hooks: z.array(HookRecordSchema).default([]),
});

export type HooksState = z.infer<typeof HooksStateSchema>;

export const ChapterSummaryRowSchema = z.object({
  chapter: z.number().int().min(1),
  title: z.string().min(1),
  characters: z.string().default(""),
  events: z.string().default(""),
  stateChanges: z.string().default(""),
  hookActivity: z.string().default(""),
  mood: z.string().default(""),
  chapterType: z.string().default(""),
  // R2/359 号：张力评分（1–10）。由 writer 从 settle 输出的 TENSION_METRICS
  // 节解析后程序化注入——不进 LLM 的 delta JSON 模板，缺省 undefined 不渲染。
  conflictLevel: z.number().int().min(1).max(10).optional(),
  revealLevel: z.number().int().min(1).max(10).optional(),
});

export type ChapterSummaryRow = z.infer<typeof ChapterSummaryRowSchema>;

export const ChapterSummariesStateSchema = z.object({
  rows: z.array(ChapterSummaryRowSchema).default([]),
});

export type ChapterSummariesState = z.infer<typeof ChapterSummariesStateSchema>;

export const CurrentStateFactSchema = z.object({
  subject: z.string().min(1),
  predicate: z.string().min(1),
  object: z.string().min(1),
  validFromChapter: z.number().int().min(0),
  validUntilChapter: z.number().int().min(0).nullable(),
  sourceChapter: z.number().int().min(0),
});

export type CurrentStateFact = z.infer<typeof CurrentStateFactSchema>;

export const CurrentStateStateSchema = z.object({
  chapter: z.number().int().min(0),
  facts: z.array(CurrentStateFactSchema).default([]),
});

export type CurrentStateState = z.infer<typeof CurrentStateStateSchema>;

export const CurrentStatePatchSchema = z.object({
  currentLocation: z.string().optional(),
  protagonistState: z.string().optional(),
  currentGoal: z.string().optional(),
  currentConstraint: z.string().optional(),
  currentAlliances: z.string().optional(),
  currentConflict: z.string().optional(),
});

export type CurrentStatePatch = z.infer<typeof CurrentStatePatchSchema>;

export const HookOpsSchema = z.object({
  upsert: z.array(HookRecordSchema).default([]),
  mention: z.array(z.string().min(1)).default([]),
  resolve: z.array(z.string().min(1)).default([]),
  defer: z.array(z.string().min(1)).default([]),
});

export type HookOps = z.infer<typeof HookOpsSchema>;

export const NewHookCandidateSchema = z.object({
  type: z.string().min(1),
  /** R23/394 号：候选可带规范类型（LLM 词表输出，宿主边界归一化）。 */
  kind: HookKindSchema.optional(),
  expectedPayoff: z.string().default(""),
  payoffTiming: HookPayoffTimingSchema.optional(),
  notes: z.string().default(""),
});

export type NewHookCandidate = z.infer<typeof NewHookCandidateSchema>;

const LooseOpSchema = z.record(z.string(), z.unknown());

export const RuntimeStateDeltaSchema = z.object({
  chapter: z.number().int().min(1),
  currentStatePatch: CurrentStatePatchSchema.optional(),
  hookOps: HookOpsSchema.default({
    upsert: [],
    mention: [],
    resolve: [],
    defer: [],
  }),
  newHookCandidates: z.array(NewHookCandidateSchema).default([]),
  chapterSummary: ChapterSummaryRowSchema.optional(),
  subplotOps: z.array(LooseOpSchema).default([]),
  emotionalArcOps: z.array(LooseOpSchema).default([]),
  characterMatrixOps: z.array(LooseOpSchema).default([]),
  notes: z.array(z.string()).default([]),
});

export type RuntimeStateDelta = z.infer<typeof RuntimeStateDeltaSchema>;
