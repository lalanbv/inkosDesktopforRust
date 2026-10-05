import { z } from "zod";
import { PlayModeSchema, SessionKindSchema, type PlayMode, type SessionKind } from "./session.js";
export type { SessionKind };
export type { PlayMode };

export const TranscriptRoleSchema = z.enum(["user", "assistant", "toolResult", "system"]);
export type TranscriptRole = z.infer<typeof TranscriptRoleSchema>;

const BaseEventSchema = z.object({
  version: z.literal(1),
  sessionId: z.string().min(1),
  seq: z.number().int().nonnegative(),
  timestamp: z.number().int().nonnegative(),
  /**
   * R36 会话树化（636 号）：本事件在链上的父事件 seq。缺省（键不存在）=
   * legacy 线性链语义——父即 seq 序前一个事件，旧文件零改写照常解析；
   * null = 链根（含 reset-leaf 后新链首事件）。append 助手统一戳记，
   * 写入方不感知。 */
  parentSeq: z.number().int().nonnegative().nullable().optional(),
});

export const SessionCreatedEventSchema = BaseEventSchema.extend({
  type: z.literal("session_created"),
  bookId: z.string().nullable(),
  sessionKind: SessionKindSchema.optional(),
  playMode: PlayModeSchema.optional(),
  title: z.string().nullable().default(null),
  createdAt: z.number().int().nonnegative(),
  updatedAt: z.number().int().nonnegative(),
});

export const SessionMetadataUpdatedEventSchema = BaseEventSchema.extend({
  type: z.literal("session_metadata_updated"),
  bookId: z.string().nullable().optional(),
  sessionKind: SessionKindSchema.optional(),
  playMode: PlayModeSchema.optional(),
  title: z.string().nullable().optional(),
  updatedAt: z.number().int().nonnegative(),
});

export const RequestStartedEventSchema = BaseEventSchema.extend({
  type: z.literal("request_started"),
  requestId: z.string().min(1),
  sessionKind: SessionKindSchema.optional(),
  input: z.string(),
});

export const RequestCommittedEventSchema = BaseEventSchema.extend({
  type: z.literal("request_committed"),
  requestId: z.string().min(1),
});

export const RequestFailedEventSchema = BaseEventSchema.extend({
  type: z.literal("request_failed"),
  requestId: z.string().min(1),
  error: z.string(),
});

export const MessageEventSchema = BaseEventSchema.extend({
  type: z.literal("message"),
  requestId: z.string().min(1),
  uuid: z.string().min(1),
  parentUuid: z.string().min(1).nullable(),
  role: TranscriptRoleSchema,
  piTurnIndex: z.number().int().nonnegative().optional(),
  toolCallId: z.string().min(1).optional(),
  sourceToolAssistantUuid: z.string().min(1).optional(),
  legacyDisplay: z.object({
    thinking: z.string().optional(),
    toolExecutions: z.array(z.unknown()).optional(),
  }).optional(),
  message: z.unknown(),
});

/**
 * R32a 会话压缩条目（553 号）：恢复窗口从 `firstKeptUuid` 起，其前的对话以
 * `summary`（LLM 生成，迭代链式时已并入上一条摘要）替代。`firstKeptUuid`
 * 为 null 表示无保留段（全部对话被摘要，极端防线）。上游语义对应
 * pi-agent-core session `CompactionEntry`（firstKeptEntryId 等价物——R36
 * 树化后压缩沿 active 链解释：恢复时取 root→head 路径上最近一条 compaction，
 * message uuid 即定位；分支各自的压缩边界互不污染）。
 */
export const CompactionEventSchema = BaseEventSchema.extend({
  type: z.literal("compaction"),
  requestId: z.string().min(1),
  summary: z.string().min(1),
  firstKeptUuid: z.string().min(1).nullable(),
  tokensBefore: z.number().int().nonnegative(),
  trigger: z.enum(["threshold", "overflow"]),
});

/**
 * R36 会话树化（636 号）：分支指针移动事件（对齐 Pi branch 的 leaf 语义）。
 * `toSeq` = 新 head（null 等价 resetLeaf——head 置空，后续写入开新链根）；
 * `fromSeq` = 移动前 head（审计/桥接用）。branch_moved 自身不入任何对话链，
 * replay 时只改写 head 不延伸链。版本字面量保持 1：加法式扩展零版本增量。
 */
export const BranchMovedEventSchema = BaseEventSchema.extend({
  type: z.literal("branch_moved"),
  fromSeq: z.number().int().nonnegative().nullable(),
  toSeq: z.number().int().nonnegative().nullable(),
});

export const TranscriptEventSchema = z.discriminatedUnion("type", [
  SessionCreatedEventSchema,
  SessionMetadataUpdatedEventSchema,
  RequestStartedEventSchema,
  RequestCommittedEventSchema,
  RequestFailedEventSchema,
  MessageEventSchema,
  CompactionEventSchema,
  BranchMovedEventSchema,
]);

export type SessionCreatedEvent = z.infer<typeof SessionCreatedEventSchema>;
export type SessionMetadataUpdatedEvent = z.infer<typeof SessionMetadataUpdatedEventSchema>;
export type RequestStartedEvent = z.infer<typeof RequestStartedEventSchema>;
export type RequestCommittedEvent = z.infer<typeof RequestCommittedEventSchema>;
export type RequestFailedEvent = z.infer<typeof RequestFailedEventSchema>;
export type MessageEvent = z.infer<typeof MessageEventSchema>;
export type CompactionEvent = z.infer<typeof CompactionEventSchema>;
export type BranchMovedEvent = z.infer<typeof BranchMovedEventSchema>;
export type TranscriptEvent = z.infer<typeof TranscriptEventSchema>;
