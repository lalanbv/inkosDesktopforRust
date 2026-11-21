import type { AuditResult } from "../agents/continuity.js";
import type { ChapterMeta } from "../models/chapter.js";
import type { LengthTelemetry, LengthSpec } from "../models/length-governance.js";
import type { WritingLogRow } from "../utils/writing-goal.js";

export interface ChapterPersistenceUsage {
  readonly promptTokens: number;
  readonly completionTokens: number;
  readonly totalTokens: number;
}

export async function persistChapterArtifacts(params: {
  readonly chapterNumber: number;
  readonly chapterTitle: string;
  readonly auditResult: AuditResult;
  readonly finalWordCount: number;
  readonly lengthTelemetry?: LengthTelemetry;
  readonly lengthSpec?: LengthSpec;
  readonly tokenUsage?: ChapterPersistenceUsage;
  readonly loadChapterIndex: () => Promise<ReadonlyArray<ChapterMeta>>;
  readonly saveChapter: (index: ReadonlyArray<ChapterMeta>) => Promise<void>;
  readonly markBookActiveIfNeeded: () => Promise<void>;
  readonly now?: () => string;
  /** R51/1654 号：日产出账本追加（wordCount Δ≠0 时；可选=既有调用方零
   * 破坏，best-effort 失败吞——Rust hooks 默认 no-op 同语义）。 */
  readonly appendWritingLog?: (row: WritingLogRow) => Promise<void>;
}): Promise<{ readonly entry: ChapterMeta }> {
  const existingIndex = await params.loadChapterIndex();
  const now = params.now?.() ?? new Date().toISOString();
  const entry: ChapterMeta = {
    number: params.chapterNumber,
    title: params.chapterTitle,
    wordCount: params.finalWordCount,
    createdAt: now,
    updatedAt: now,
    observations: [...params.auditResult.observations],
    provenance: "generated",
    lengthTelemetry: params.lengthTelemetry,
    lengthSpec: params.lengthSpec,
    tokenUsage: params.tokenUsage,
  };
  const existingIdx = existingIndex.findIndex((e) => e.number === params.chapterNumber);
  const updatedIndex = existingIdx >= 0
    ? existingIndex.map((e, i) => i === existingIdx ? { ...entry, createdAt: e.createdAt } : e)
    : [...existingIndex, entry];
  await params.saveChapter(updatedIndex);
  // R51/1654 号：管线写章 wordCount Δ≠0 记账（索引保存成功后=Rust
  // persist_chapter_artifacts 同序；旧值=既有 index 同号 wordCount，新章
  // 无同号行=before 0；ts 复用注入时刻=同一保存事件；best-effort 失败吞）。
  const existing = existingIndex.find((e) => e.number === params.chapterNumber);
  const before = existing?.wordCount ?? 0;
  if (before !== params.finalWordCount && params.appendWritingLog) {
    await params.appendWritingLog({
      ts: now,
      chapterNum: params.chapterNumber,
      before,
      after: params.finalWordCount,
    }).catch(() => undefined);
  }
  await params.markBookActiveIfNeeded();

  return { entry };
}
