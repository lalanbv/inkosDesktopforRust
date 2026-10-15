import { z } from "zod";
import { LengthTelemetrySchema, LengthSpecSchema } from "./length-governance.js";
import { ObservationSchema } from "./observation.js";

// 714 号：审改流域章态（Rust models/chapter.rs ChapterStatus 14 态为真源镜像，
// serde rename 同名串）。可选键——2.0 索引存量文件无此键；approve/reject 写
// 面落盘后须可被本 schema 再读（709 号「strict 裁剪自毁键」教训：写入成功
// ≠ 产物可再消费）。
export const CHAPTER_STATUSES = [
  "card-generated",
  "drafting",
  "drafted",
  "auditing",
  "audit-passed",
  "audit-failed",
  "state-degraded",
  "revising",
  "ready-for-review",
  "approved",
  "rejected",
  "published",
  "imported",
  "needs-revision",
] as const;

export const ChapterMetaSchema = z.object({
  number: z.number().int().min(1),
  title: z.string(),
  status: z.enum(CHAPTER_STATUSES).optional(),
  wordCount: z.number().int(),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime(),
  observations: z.array(ObservationSchema),
  provenance: z.enum(["generated", "imported", "edited"]),
  lengthTelemetry: LengthTelemetrySchema.optional(),
  lengthSpec: LengthSpecSchema.optional(),
  tokenUsage: z.object({
    promptTokens: z.number().int(),
    completionTokens: z.number().int(),
    totalTokens: z.number().int(),
  }).strict().optional(),
}).strict();

export type ChapterMeta = z.infer<typeof ChapterMetaSchema>;
