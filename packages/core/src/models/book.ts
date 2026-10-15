import { z } from "zod";

export const PlatformSchema = z.string().trim().min(1);
export type Platform = z.infer<typeof PlatformSchema>;

export const GenreSchema = z.string().min(1);
export type Genre = z.infer<typeof GenreSchema>;

export const BookStatusSchema = z.enum([
  "incubating",
  "outlining",
  "active",
  "paused",
  "completed",
  "dropped",
]);
export type BookStatus = z.infer<typeof BookStatusSchema>;

export const FanficModeSchema = z.string().trim().min(1);
export type FanficMode = z.infer<typeof FanficModeSchema>;

// ── 709 号：写键自由面收敛（Rust BookConfig 为真源，engine-rs models/book.rs）──
// 2.0 strict 化时裁掉的 fork 可选键复位：best-of-n（701）/auto-beats、series-id
// （707）等写面把 writing/governance/seriesId 落进 book.json，strict 根拒收会让
// 随后的 loadBookConfig ZodError（整本书不可加载的潜伏自毁键）。子对象保持
// fork 非严格口径（未知子键容忍，与 serde 缺省容错同水位）。
export const BookWritingSchema = z.object({
  reviewMode: z.enum(["auto", "manual"]).optional(),
  revisionGate: z.enum(["strict", "lenient", "always"]).optional(),
  /** 189 号：write-next 落盘后自动为本章沉淀时间线节拍（默认关）。 */
  autoTimelineBeats: z.boolean().optional(),
  /** R11/378 号：场景节拍驱动写作（planner 产节拍、writer 按拍推进；默认关）。 */
  sceneBeats: z.boolean().optional(),
});
export type BookWriting = z.infer<typeof BookWritingSchema>;

export const BookSeriesSchema = z.object({
  name: z.string().min(1),
  order: z.number().int().min(1),
});
export type BookSeries = z.infer<typeof BookSeriesSchema>;

export const BookGovernanceSchema = z.object({
  policy: z.enum(["completion-first", "quality-first"]).optional(),
  maxConsecutiveDebts: z.number().int().min(1).max(20).optional(),
  /** G11/371 号：多版选优（首版分数低于 minScore 时追加候选重生成）。 */
  bestOfN: z
    .object({
      enabled: z.boolean().optional(),
      candidates: z.number().int().min(2).max(3).optional(),
      minScore: z.number().int().min(0).max(100).optional(),
    })
    .optional(),
});
export type BookGovernance = z.infer<typeof BookGovernanceSchema>;

export const BookConfigSchema = z.object({
  id: z.string().min(1),
  title: z.string().min(1),
  platform: PlatformSchema,
  genre: GenreSchema,
  status: BookStatusSchema,
  targetChapters: z.number().int().min(1),
  chapterWordCount: z.number().int().min(1),
  minChapterLength: z.number().int().min(1).optional(),
  maxChapterLength: z.number().int().min(1).optional(),
  language: z.enum(["zh", "en"]),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime(),
  parentBookId: z.string().optional(),
  fanficMode: FanficModeSchema.optional(),
  /** R20/390 号：系列正典共享键（snake_case slug，指向 .inkos/series/{seriesId}.json）。 */
  seriesId: z.string().optional(),
  /** 180 号 C3-a：系列归属（Rust BookSeries 镜像）。 */
  series: BookSeriesSchema.optional(),
  writing: BookWritingSchema.optional(),
  /** G3/337 号：每书质量治理方案（缺省回落 completion-first/上限 3）。 */
  governance: BookGovernanceSchema.optional(),
}).strict();

export type BookConfig = z.infer<typeof BookConfigSchema>;
