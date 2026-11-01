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
  // 792 号：observations/provenance 转 optional——Rust ChapterMeta 无此二字段
  //（serde 忽略未知键），索引响应行键对齐（重建行不发）；存量索引两键照常
  // round-trip（读宽容、写保留）。
  observations: z.array(ObservationSchema).optional(),
  // 717 号：审查问题清单（Rust ChapterMeta.audit_issues serde default 空 vec
  // 镜像；book-eval 质量评估消费，存量索引文件无此键完全兼容）。
  auditIssues: z.array(z.string()).optional(),
  // 792 号：length_warnings 镜像（Rust serde default 空 vec 恒序列化——
  // bookDetail 响应行键对齐；存量索引无此键完全兼容）。
  lengthWarnings: z.array(z.string()).optional(),
  provenance: z.enum(["generated", "imported", "edited"]).optional(),
  lengthTelemetry: LengthTelemetrySchema.optional(),
  lengthSpec: LengthSpecSchema.optional(),
  tokenUsage: z.object({
    promptTokens: z.number().int(),
    completionTokens: z.number().int(),
    totalTokens: z.number().int(),
  }).strict().optional(),
}).strict();

export type ChapterMeta = z.infer<typeof ChapterMetaSchema>;
