import { z } from "zod";

const ProtagonistSchema = z.object({
  name: z.string(),
  personalityLock: z.array(z.string()),
  behavioralConstraints: z.array(z.string()),
}).strict().optional();

const GenreLockSchema = z.object({
  primary: z.string(),
  forbidden: z.array(z.string()),
}).strict().optional();

export const BookRulesSchema = z.object({
  version: z.literal("2"),
  protagonist: ProtagonistSchema,
  genreLock: GenreLockSchema,
  narrativePerson: z.string().trim().min(1).optional(),
  prohibitions: z.array(z.string()),
  // R64/1700 号：双栈键面收口——以下五节镜像 Rust BookRules serde 键面（
  // serde default 恒序列化，strict 根此前拒收=任何 Rust 写回的规则文件 TS
  // parse 崩，1429/1666 同族）；形态对齐 serde：列表/可选/untagged 联合。
  chapterTypesOverride: z.array(z.string()).optional(),
  fatigueWordsOverride: z.array(z.string()).optional(),
  sensitiveWords: z.array(z.string()).optional(),
  additionalAuditDimensions: z.array(z.union([z.number(), z.string()])).optional(),
  eraConstraints: z
    .object({
      enabled: z.boolean().optional(),
      period: z.string().optional(),
      region: z.string().optional(),
    })
    .strict()
    .optional(),
  numericalSystemOverrides: z
    .object({
      hardCap: z.union([z.number(), z.string()]).optional(),
      resourceTypes: z.array(z.string()).optional(),
    })
    .strict()
    .optional(),
  enableFullCastTracking: z.boolean(),
  fanficMode: z.string().trim().min(1).optional(),
  allowedDeviations: z.array(z.string()),
}).strict();

export type BookRules = z.infer<typeof BookRulesSchema>;

export interface ParsedBookRules {
  readonly rules: BookRules;
  readonly body: string;
}
