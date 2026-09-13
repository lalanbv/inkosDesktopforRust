/**
 * Temporal memory database for InkOS truth files.
 *
 * Uses Node.js built-in SQLite (node:sqlite, Node 22+).
 * Stores facts with temporal validity (valid_from/valid_until chapter numbers),
 * enabling precise queries like "what did character X know in chapter 5?"
 *
 * Backward compatible: existing markdown truth files are still the primary
 * persistence layer. MemoryDB is an acceleration index built alongside them.
 */

import { createRequire } from "node:module";
import { join } from "node:path";

const require = createRequire(import.meta.url);

const FACT_SELECT_COLUMNS = `
  id,
  subject,
  predicate,
  object,
  valid_from_chapter AS validFromChapter,
  valid_until_chapter AS validUntilChapter,
  source_chapter AS sourceChapter
`;

export interface Fact {
  readonly id?: number;
  readonly subject: string;
  readonly predicate: string;
  readonly object: string;
  readonly validFromChapter: number;
  readonly validUntilChapter: number | null;
  readonly sourceChapter: number;
}

export interface StoredSummary {
  readonly chapter: number;
  readonly title: string;
  readonly characters: string;
  readonly events: string;
  readonly stateChanges: string;
  readonly hookActivity: string;
  readonly mood: string;
  readonly chapterType: string;
  /** R2/358 号：张力评分（1–10，settle TENSION_METRICS 节产出）。缺分章为 undefined；不入 sqlite（真相源在 chapter_summaries.md）。 */
  readonly conflictLevel?: number;
  readonly revealLevel?: number;
}

/** G3/337 号：质量债务账本一行（对齐 ANWA A3 质量债务；状态机见 quality-governance.ts）。 */
export interface StoredQualityDebt {
  readonly debtId: string;
  readonly bookId: string;
  readonly chapter: number;
  readonly issueCategory: string;
  readonly severity: "critical" | "warning" | "info";
  readonly status: "open" | "deferred" | "resolved";
  readonly createdAt: string;
  readonly followUpNote?: string;
}

/** R3/361 号：章节审查指标一行（对应 `review_metrics` 表；contentHash 幂等重放）。 */
export interface StoredReviewMetric {
  readonly chapter: number;
  readonly overallScore?: number;
  readonly passed: boolean;
  readonly criticalCount: number;
  readonly warningCount: number;
  readonly infoCount: number;
  readonly contentHash: string;
  readonly recordedAt: string;
}

/** G1/349 号：语义检索 chunk 向量一行（vector 为 JSON 数组文本，双端序列化一致）。 */
export interface StoredChunkVector {
  readonly chunkId: string;
  readonly source: string;
  readonly fingerprint: string;
  readonly dim: number;
  readonly vector: ReadonlyArray<number>;
  readonly createdAt: string;
}

export interface StoredHook {
  readonly hookId: string;
  readonly startChapter: number;
  readonly type: string;
  readonly status: string;
  /** R23/394 号：规范类型分类（markdown 台账第 14 列往返；sqlite 投影不落）。 */
  readonly kind?: import("../models/runtime-state.js").HookKind;
  readonly lastAdvancedChapter: number;
  readonly expectedPayoff: string;
  readonly payoffTiming?: string;
  readonly notes: string;
  // Phase 7 — hook causality / promotion metadata.
  readonly dependsOn?: ReadonlyArray<string>;
  readonly paysOffInArc?: string;
  readonly coreHook?: boolean;
  readonly halfLifeChapters?: number;
  readonly advancedCount?: number;
  // Phase 7 hotfix 2 — whether the seed has been promoted into the live ledger
  // (architect-time structural rules + consolidator-time advanced_count rule).
  // Reviewer uses this to gate critical-severity escalation.
  readonly promoted?: boolean;
}

export class MemoryDB {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private db: any;

  constructor(bookDir: string) {
    // node:sqlite requires Node 22+; require() via createRequire for ESM compat
    const { DatabaseSync } = require("node:sqlite");
    const dbPath = join(bookDir, "story", "memory.db");
    this.db = new DatabaseSync(dbPath);
    this.db.exec("PRAGMA journal_mode = WAL");
    this.migrate();
  }

  private migrate(): void {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS facts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        subject TEXT NOT NULL,
        predicate TEXT NOT NULL,
        object TEXT NOT NULL,
        valid_from_chapter INTEGER NOT NULL,
        valid_until_chapter INTEGER,
        source_chapter INTEGER NOT NULL,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
      );

      CREATE TABLE IF NOT EXISTS chapter_summaries (
        chapter INTEGER PRIMARY KEY,
        title TEXT NOT NULL,
        characters TEXT NOT NULL DEFAULT '',
        events TEXT NOT NULL DEFAULT '',
        state_changes TEXT NOT NULL DEFAULT '',
        hook_activity TEXT NOT NULL DEFAULT '',
        mood TEXT NOT NULL DEFAULT '',
        chapter_type TEXT NOT NULL DEFAULT ''
      );

      CREATE TABLE IF NOT EXISTS hooks (
        hook_id TEXT PRIMARY KEY,
        start_chapter INTEGER NOT NULL DEFAULT 0,
        type TEXT NOT NULL DEFAULT '',
        status TEXT NOT NULL DEFAULT 'open',
        last_advanced_chapter INTEGER NOT NULL DEFAULT 0,
        expected_payoff TEXT NOT NULL DEFAULT '',
        payoff_timing TEXT NOT NULL DEFAULT '',
        notes TEXT NOT NULL DEFAULT ''
      );

      CREATE INDEX IF NOT EXISTS idx_facts_subject ON facts(subject);
      CREATE INDEX IF NOT EXISTS idx_facts_valid ON facts(valid_from_chapter, valid_until_chapter);
      CREATE INDEX IF NOT EXISTS idx_facts_source ON facts(source_chapter);
      CREATE INDEX IF NOT EXISTS idx_hooks_status ON hooks(status);
      CREATE INDEX IF NOT EXISTS idx_hooks_last_advanced ON hooks(last_advanced_chapter);
      CREATE TABLE IF NOT EXISTS quality_debts (
        debt_id TEXT PRIMARY KEY,
        book_id TEXT NOT NULL DEFAULT '',
        chapter INTEGER NOT NULL DEFAULT 0,
        issue_category TEXT NOT NULL DEFAULT '',
        severity TEXT NOT NULL DEFAULT 'warning',
        status TEXT NOT NULL DEFAULT 'open',
        created_at TEXT NOT NULL DEFAULT '',
        follow_up_note TEXT NOT NULL DEFAULT ''
      );
      CREATE INDEX IF NOT EXISTS idx_debts_status ON quality_debts(status);
      CREATE INDEX IF NOT EXISTS idx_debts_book ON quality_debts(book_id, chapter);
      CREATE TABLE IF NOT EXISTS review_metrics (
        chapter INTEGER PRIMARY KEY,
        overall_score INTEGER,
        passed INTEGER NOT NULL DEFAULT 1,
        critical_count INTEGER NOT NULL DEFAULT 0,
        warning_count INTEGER NOT NULL DEFAULT 0,
        info_count INTEGER NOT NULL DEFAULT 0,
        content_hash TEXT NOT NULL DEFAULT '',
        recorded_at TEXT NOT NULL DEFAULT ''
      );
      CREATE TABLE IF NOT EXISTS retrieval_chunks (
        chunk_id TEXT PRIMARY KEY,
        source TEXT NOT NULL DEFAULT '',
        fingerprint TEXT NOT NULL DEFAULT '',
        dim INTEGER NOT NULL DEFAULT 0,
        vector TEXT NOT NULL DEFAULT '[]',
        created_at TEXT NOT NULL DEFAULT ''
      );
      CREATE INDEX IF NOT EXISTS idx_chunks_source ON retrieval_chunks(source);
    `);

    this.ensureColumn("hooks", "payoff_timing", "TEXT NOT NULL DEFAULT ''");
  }

  private ensureColumn(table: string, column: string, definition: string): void {
    try {
      this.db.exec(`ALTER TABLE ${table} ADD COLUMN ${column} ${definition}`);
    } catch {
      // Column already exists on existing databases.
    }
  }

  // ---------------------------------------------------------------------------
  // Facts (temporal)
  // ---------------------------------------------------------------------------

  /** Add a new fact. */
  addFact(fact: Omit<Fact, "id">): number {
    const stmt = this.db.prepare(
      `INSERT INTO facts (subject, predicate, object, valid_from_chapter, valid_until_chapter, source_chapter)
       VALUES (?, ?, ?, ?, ?, ?)`,
    );
    const result = stmt.run(
      fact.subject, fact.predicate, fact.object,
      fact.validFromChapter, fact.validUntilChapter ?? null, fact.sourceChapter,
    );
    return Number(result.lastInsertRowid);
  }

  /** Invalidate a fact (set valid_until). */
  invalidateFact(id: number, untilChapter: number): void {
    this.db.prepare(
      "UPDATE facts SET valid_until_chapter = ? WHERE id = ?",
    ).run(untilChapter, id);
  }

  /** Get all currently valid facts (valid_until is null). */
  getCurrentFacts(): ReadonlyArray<Fact> {
    return this.db.prepare(
      `SELECT ${FACT_SELECT_COLUMNS}
       FROM facts
       WHERE valid_until_chapter IS NULL
       ORDER BY subject, predicate`,
    ).all() as unknown as Fact[];
  }

  /** Get facts about a specific subject that are valid at a given chapter. */
  getFactsAt(subject: string, chapter: number): ReadonlyArray<Fact> {
    return this.db.prepare(
      `SELECT ${FACT_SELECT_COLUMNS}
       FROM facts
       WHERE subject = ? AND valid_from_chapter <= ?
       AND (valid_until_chapter IS NULL OR valid_until_chapter > ?)
       ORDER BY predicate`,
    ).all(subject, chapter, chapter) as unknown as Fact[];
  }

  /** Get all facts about a subject (including historical). */
  getFactHistory(subject: string): ReadonlyArray<Fact> {
    return this.db.prepare(
      `SELECT ${FACT_SELECT_COLUMNS}
       FROM facts
       WHERE subject = ?
       ORDER BY valid_from_chapter`,
    ).all(subject) as unknown as Fact[];
  }

  /** Search facts by predicate (e.g., all "location" facts). */
  getFactsByPredicate(predicate: string): ReadonlyArray<Fact> {
    return this.db.prepare(
      `SELECT ${FACT_SELECT_COLUMNS}
       FROM facts
       WHERE predicate = ? AND valid_until_chapter IS NULL
       ORDER BY subject`,
    ).all(predicate) as unknown as Fact[];
  }

  /** Get facts relevant to a set of character names. */
  getFactsForCharacters(names: ReadonlyArray<string>): ReadonlyArray<Fact> {
    if (names.length === 0) return [];
    const placeholders = names.map(() => "?").join(",");
    return this.db.prepare(
      `SELECT ${FACT_SELECT_COLUMNS}
       FROM facts
       WHERE subject IN (${placeholders}) AND valid_until_chapter IS NULL
       ORDER BY subject, predicate`,
    ).all(...names) as unknown as Fact[];
  }

  replaceCurrentFacts(facts: ReadonlyArray<Omit<Fact, "id">>): void {
    this.db.exec("DELETE FROM facts WHERE valid_until_chapter IS NULL");
    for (const fact of facts) {
      this.addFact(fact);
    }
  }

  resetFacts(): void {
    this.db.exec("DELETE FROM facts");
  }

  // ---------------------------------------------------------------------------
  // Chapter summaries
  // ---------------------------------------------------------------------------

  /** Upsert a chapter summary. */
  upsertSummary(summary: StoredSummary): void {
    this.db.prepare(
      `INSERT OR REPLACE INTO chapter_summaries (chapter, title, characters, events, state_changes, hook_activity, mood, chapter_type)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    ).run(
      summary.chapter, summary.title, summary.characters, summary.events,
      summary.stateChanges, summary.hookActivity, summary.mood, summary.chapterType,
    );
  }

  replaceSummaries(summaries: ReadonlyArray<StoredSummary>): void {
    this.db.exec("DELETE FROM chapter_summaries");
    for (const summary of summaries) {
      this.upsertSummary(summary);
    }
  }

  /** Get summaries for a range of chapters. */
  getSummaries(fromChapter: number, toChapter: number): ReadonlyArray<StoredSummary> {
    return this.db.prepare(
      `SELECT
         chapter,
         title,
         characters,
         events,
         state_changes AS stateChanges,
         hook_activity AS hookActivity,
         mood,
         chapter_type AS chapterType
       FROM chapter_summaries
       WHERE chapter >= ? AND chapter <= ?
       ORDER BY chapter`,
    ).all(fromChapter, toChapter) as unknown as StoredSummary[];
  }

  /** Get summaries matching any of the given character names. */
  getSummariesByCharacters(names: ReadonlyArray<string>): ReadonlyArray<StoredSummary> {
    if (names.length === 0) return [];
    const conditions = names.map(() => "characters LIKE ?").join(" OR ");
    const params = names.map((n) => `%${n}%`);
    return this.db.prepare(
      `SELECT
         chapter,
         title,
         characters,
         events,
         state_changes AS stateChanges,
         hook_activity AS hookActivity,
         mood,
         chapter_type AS chapterType
       FROM chapter_summaries
       WHERE ${conditions}
       ORDER BY chapter`,
    ).all(...params) as unknown as StoredSummary[];
  }

  /** Get total chapter count. */
  getChapterCount(): number {
    const row = this.db.prepare("SELECT COUNT(*) as count FROM chapter_summaries").get() as unknown as { count: number };
    return row.count;
  }

  /** Get the most recent N summaries. */
  getRecentSummaries(count: number): ReadonlyArray<StoredSummary> {
    return this.db.prepare(
      `SELECT
         chapter,
         title,
         characters,
         events,
         state_changes AS stateChanges,
         hook_activity AS hookActivity,
         mood,
         chapter_type AS chapterType
       FROM chapter_summaries
       ORDER BY chapter DESC
       LIMIT ?`,
    ).all(count) as unknown as ReadonlyArray<StoredSummary>;
  }

  // ---------------------------------------------------------------------------
  // Hooks
  // ---------------------------------------------------------------------------

  upsertHook(hook: StoredHook): void {
    this.db.prepare(
      `INSERT OR REPLACE INTO hooks (hook_id, start_chapter, type, status, last_advanced_chapter, expected_payoff, payoff_timing, notes)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    ).run(
      hook.hookId,
      hook.startChapter,
      hook.type,
      hook.status,
      hook.lastAdvancedChapter,
      hook.expectedPayoff,
      hook.payoffTiming ?? "",
      hook.notes,
    );
  }

  /** G1/349 号：upsert chunk 向量（幂等）。 */
  upsertChunkVector(chunk: StoredChunkVector): void {
    this.db.prepare(
      `INSERT OR REPLACE INTO retrieval_chunks (chunk_id, source, fingerprint, dim, vector, created_at)
       VALUES (?, ?, ?, ?, ?, ?)`,
    ).run(
      chunk.chunkId,
      chunk.source,
      chunk.fingerprint,
      chunk.dim,
      JSON.stringify(chunk.vector),
      chunk.createdAt,
    );
  }

  /** chunk 向量全量（检索侧载入后内存余弦；单书量级数千，无性能压力）。 */
  listChunkVectors(): ReadonlyArray<StoredChunkVector> {
    const rows = this.db.prepare(
      `SELECT chunk_id AS chunkId, source, fingerprint, dim, vector, created_at AS createdAt
       FROM retrieval_chunks`,
    ).all() as ReadonlyArray<{ chunkId: string; source: string; fingerprint: string; dim: number; vector: string; createdAt: string }>;
    return rows.flatMap((row) => {
      try {
        const vector = JSON.parse(row.vector) as number[];
        return [{ chunkId: row.chunkId, source: row.source, fingerprint: row.fingerprint, dim: row.dim, vector, createdAt: row.createdAt }];
      } catch {
        return [];
      }
    });
  }

  get chunkVectorCount(): number {
    const row = this.db.prepare("SELECT COUNT(*) AS n FROM retrieval_chunks").get() as { n: number };
    return Number(row.n);
  }

  /** G3/337 号：记入一条质量债务（INSERT OR REPLACE，幂等）。 */
  recordDebt(debt: StoredQualityDebt): void {
    this.db.prepare(
      `INSERT OR REPLACE INTO quality_debts (debt_id, book_id, chapter, issue_category, severity, status, created_at, follow_up_note)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    ).run(
      debt.debtId,
      debt.bookId,
      debt.chapter,
      debt.issueCategory,
      debt.severity,
      debt.status,
      debt.createdAt,
      debt.followUpNote ?? "",
    );
  }

  /** 债务清单：按书/状态过滤（缺省全量），新章在前。 */
  listDebts(bookId?: string, status?: "open" | "deferred" | "resolved"): ReadonlyArray<StoredQualityDebt> {
    const where: string[] = [];
    const params: string[] = [];
    if (bookId) {
      where.push("book_id = ?");
      params.push(bookId);
    }
    if (status) {
      where.push("status = ?");
      params.push(status);
    }
    const rows = this.db.prepare(
      `SELECT debt_id AS debtId, book_id AS bookId, chapter, issue_category AS issueCategory,
              severity, status, created_at AS createdAt, follow_up_note AS followUpNote
       FROM quality_debts ${where.length ? `WHERE ${where.join(" AND ")}` : ""}
       ORDER BY chapter DESC, created_at DESC`,
    ).all(...params) as ReadonlyArray<StoredQualityDebt>;
    return rows;
  }

  /** 债务状态流转（defer/resolve/reopen 的落库口；附跟进备注）。 */
  updateDebtStatus(
    debtId: string,
    status: "open" | "deferred" | "resolved",
    followUpNote?: string,
  ): boolean {
    const result = this.db.prepare(
      `UPDATE quality_debts SET status = ?, follow_up_note = CASE WHEN ? = '' THEN follow_up_note ELSE ? END
       WHERE debt_id = ?`,
    ).run(status, followUpNote ?? "", followUpNote ?? "", debtId);
    return Number(result.changes) > 0;
  }

  /**
   * R3/361 号：沉淀章节审查指标（contentHash 幂等——同章同内容重放跳过返回
   * false；内容更新后修订覆盖返回 true）。
   */
  recordReviewMetric(metric: StoredReviewMetric): boolean {
    const existing = this.db.prepare(
      "SELECT content_hash FROM review_metrics WHERE chapter = ?",
    ).get(metric.chapter) as { content_hash: string } | undefined;
    if (existing && existing.content_hash === metric.contentHash) {
      return false;
    }
    this.db.prepare(
      `INSERT OR REPLACE INTO review_metrics (chapter, overall_score, passed, critical_count, warning_count, info_count, content_hash, recorded_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    ).run(
      metric.chapter,
      typeof metric.overallScore === "number" ? metric.overallScore : null,
      metric.passed ? 1 : 0,
      metric.criticalCount,
      metric.warningCount,
      metric.infoCount,
      metric.contentHash,
      metric.recordedAt,
    );
    return true;
  }

  /** 审查指标清单（章号升序）——quality-trend 端点数据源。 */
  listReviewMetrics(): ReadonlyArray<StoredReviewMetric> {
    return this.db.prepare(
      `SELECT chapter, overall_score AS overallScore, passed, critical_count AS criticalCount,
              warning_count AS warningCount, info_count AS infoCount,
              content_hash AS contentHash, recorded_at AS recordedAt
       FROM review_metrics ORDER BY chapter`,
    ).all().map((row: any) => ({
      ...row,
      overallScore: typeof row.overallScore === "number" ? row.overallScore : undefined,
      passed: Number(row.passed) === 1,
    })) as ReadonlyArray<StoredReviewMetric>;
  }

  replaceHooks(hooks: ReadonlyArray<StoredHook>): void {
    this.db.exec("DELETE FROM hooks");
    for (const hook of hooks) {
      this.upsertHook(hook);
    }
  }

  /** G10/338 号：全量 hook（含已兑付）——承诺时间线数据源。 */
  getAllHooks(): ReadonlyArray<StoredHook> {
    return this.db.prepare(
      `SELECT
         hook_id AS hookId,
         start_chapter AS startChapter,
         type,
         status,
         last_advanced_chapter AS lastAdvancedChapter,
         expected_payoff AS expectedPayoff,
         payoff_timing AS payoffTiming,
         notes
       FROM hooks
       ORDER BY start_chapter ASC, hook_id ASC`,
    ).all() as unknown as ReadonlyArray<StoredHook>;
  }

  getActiveHooks(): ReadonlyArray<StoredHook> {
    return this.db.prepare(
      `SELECT
         hook_id AS hookId,
         start_chapter AS startChapter,
         type,
         status,
         last_advanced_chapter AS lastAdvancedChapter,
         expected_payoff AS expectedPayoff,
         payoff_timing AS payoffTiming,
         notes
       FROM hooks
       WHERE lower(status) NOT IN ('resolved', 'closed', '已回收', '已解决')
       ORDER BY last_advanced_chapter DESC, start_chapter DESC, hook_id ASC`,
    ).all() as unknown as ReadonlyArray<StoredHook>;
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  close(): void {
    this.db.close();
  }
}
