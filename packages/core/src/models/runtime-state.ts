import { z } from "zod";

export const RuntimeStateLanguageSchema = z.enum(["zh", "en"]);
export type RuntimeStateLanguage = z.infer<typeof RuntimeStateLanguageSchema>;

export const StateManifestSchema = z.object({
  schemaVersion: z.literal(2),
  language: RuntimeStateLanguageSchema,
  lastAppliedChapter: z.number().int().min(0),
  projectionVersion: z.number().int().min(1),
  // migrationWarnings：上游 2.0 从 schema 删除，Rust 移植自 1.x 保留（承载迁移
  // 告警，恒序列化）。TS 以 optional 容错产品引擎写入的 manifest——strict 拒收
  // 会经 readStateJsonTolerant 把整份结构化状态当缺失静默重建（键面漂移族：
  // 考古=24b5c213 移植时 1.x 有 default([])→7e60e13f 删→Rust 对齐对象是消费端）。
  migrationWarnings: z.array(z.string()).optional(),
}).strict();

export type StateManifest = z.infer<typeof StateManifestSchema>;

export const HookStatusSchema = z.enum(["open", "progressing", "deferred", "resolved", "superseded"]);
export type HookStatus = z.infer<typeof HookStatusSchema>;

// 715 号：回收时序枚举（fork 逐字）——hook-lifecycle/hook-policy 消费面
// （resolveHookPayoffTiming/inferHookPayoffTiming/localizeHookPayoffTiming）。
export const HookPayoffTimingSchema = z.enum([
  "immediate",
  "near-term",
  "mid-arc",
  "slow-burn",
  "endgame",
]);
export type HookPayoffTiming = z.infer<typeof HookPayoffTimingSchema>;

export const HookRecordSchema = z.object({
  hookId: z.string().min(1),
  startChapter: z.number().int().min(0),
  type: z.string().min(1),
  status: HookStatusSchema,
  lastAdvancedChapter: z.number().int().min(0),
  // A stored hook may have an unspecified payoff. New candidates require one.
  expectedPayoff: z.string(),
  notes: z.string(),
  dependsOn: z.array(z.string().min(1)).optional(),
  paysOffInArc: z.string().optional(),
}).strict();

export type HookRecord = z.infer<typeof HookRecordSchema>;

export const HooksStateSchema = z.object({
  hooks: z.array(HookRecordSchema),
}).strict();

export type HooksState = z.infer<typeof HooksStateSchema>;

export const ChapterSummaryRowSchema = z.object({
  chapter: z.number().int().min(1),
  title: z.string().min(1),
  characters: z.string(),
  events: z.string(),
  stateChanges: z.string(),
  hookActivity: z.string(),
  mood: z.string(),
  chapterType: z.string(),
  // 796 号：张力评分镜像（R2/359 号；rust ChapterSummaryRow.conflict_level/
  // reveal_level Option+skip 序列化同语义）——缺失不渲染（8 列形态），在场时
  // 渲染 10 列张力行（tension-curve 派生面；旧摘要无此键完全兼容）。
  conflictLevel: z.number().int().min(1).max(10).optional(),
  revealLevel: z.number().int().min(1).max(10).optional(),
}).strict();

export type ChapterSummaryRow = z.infer<typeof ChapterSummaryRowSchema>;

export const ChapterSummariesStateSchema = z.object({
  rows: z.array(ChapterSummaryRowSchema),
}).strict();

export type ChapterSummariesState = z.infer<typeof ChapterSummariesStateSchema>;

export const CurrentStateFactSchema = z.object({
  subject: z.string().min(1),
  predicate: z.string().min(1),
  object: z.string().min(1),
  validFromChapter: z.number().int().min(0),
  validUntilChapter: z.number().int().min(0).nullable(),
  sourceChapter: z.number().int().min(0),
}).strict();

export type CurrentStateFact = z.infer<typeof CurrentStateFactSchema>;

/**
 * 事实在 `chapter` 是否有效：未失效或失效章等于当前章仍有效（`>=`）。
 *
 * R65 第一步：有效期判定单一真源——reducer 的失效写入（`applyFactOps` 的
 * active 判定）、投影过滤（`renderCurrentStateProjection`）与状态校验装配
 * （state-validator）三处共用本判定，语义与 Rust
 * `CurrentStateFact::is_active_at` 逐字对齐。边界向量共享 golden：
 * `__tests__/golden/state-fact-validity.json` 的 `activeAt` 表。
 */
export function isFactActiveAt(
  fact: Pick<CurrentStateFact, "validUntilChapter">,
  chapter: number,
): boolean {
  return fact.validUntilChapter === null || fact.validUntilChapter >= chapter;
}

export const CurrentStateStateSchema = z.object({
  chapter: z.number().int().min(0),
  facts: z.array(CurrentStateFactSchema),
}).strict();

export type CurrentStateState = z.infer<typeof CurrentStateStateSchema>;

export const StateFactInputSchema = z.object({
  subject: z.string().min(1),
  predicate: z.string().min(1),
  object: z.string().min(1),
}).strict();

export type StateFactInput = z.infer<typeof StateFactInputSchema>;

export const StateFactSelectorSchema = z.object({
  subject: z.string().min(1),
  predicate: z.string().min(1),
  object: z.string().min(1).optional(),
}).strict();

export type StateFactSelector = z.infer<typeof StateFactSelectorSchema>;

export const StateFactOpsSchema = z.object({
  upsert: z.array(StateFactInputSchema),
  expire: z.array(StateFactSelectorSchema),
}).strict();

export type StateFactOps = z.infer<typeof StateFactOpsSchema>;

export const HookOpsSchema = z.object({
  upsert: z.array(HookRecordSchema),
  mention: z.array(z.string().min(1)),
  resolve: z.array(z.string().min(1)),
  defer: z.array(z.string().min(1)),
}).strict();

export type HookOps = z.infer<typeof HookOpsSchema>;

export const NewHookCandidateSchema = z.object({
  type: z.string().min(1),
  expectedPayoff: z.string().min(1),
  notes: z.string(),
}).strict();

export type NewHookCandidate = z.infer<typeof NewHookCandidateSchema>;

export const RuntimeStateDeltaSchema = z.object({
  chapter: z.number().int().min(1),
  factOps: StateFactOpsSchema,
  hookOps: HookOpsSchema,
  newHookCandidates: z.array(NewHookCandidateSchema),
  chapterSummary: ChapterSummaryRowSchema.optional(),
}).strict();

export type RuntimeStateDelta = z.infer<typeof RuntimeStateDeltaSchema>;
