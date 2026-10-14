// ── 只读 MemoryDB Mini 端口（702 号）──
//
// 2.0 移除了 fork 的 SQLite 状态层（state/memory-db.ts），但承诺账本/质量趋势/
// 名册候选等投影路由只需对 legacy memory.db 的**只读 SELECT**。本模块以
// node:sqlite DatabaseSync 提供最小读取面（getAllHooks/getSummaries/
// listReviewMetrics/listDebts），SQL 与 fork memory-db.ts 逐字同源；
// 不建表不迁移（调用方负责文件存在性检查——路由先 access 再构造）。
// 与 engine-rs memory_db.rs 同水位对偶。

import { createRequire } from "node:module";
import { join } from "node:path";
import type { StoredHook } from "../utils/promise-ledger.js";
import type { ReviewMetricRow } from "../utils/quality-trend.js";

const require = createRequire(import.meta.url);

export interface StoredSummary {
  readonly chapter: number;
  readonly title: string;
  readonly characters: string;
  readonly events: string;
  readonly stateChanges: string;
  readonly hookActivity: string;
  readonly mood: string;
  readonly chapterType: string;
}

export interface StoredReviewMetric {
  readonly chapter: number;
  readonly overallScore?: number;
  readonly passed: boolean;
  readonly criticalCount?: number;
  readonly warningCount?: number;
  readonly infoCount?: number;
  readonly contentHash?: string;
  readonly recordedAt?: string;
}

export interface StoredQualityDebt {
  readonly debtId: string;
  readonly bookId?: string;
  readonly chapter?: number;
  readonly issueCategory?: string;
  readonly severity?: string;
  readonly status?: string;
  readonly createdAt?: string;
  readonly followUpNote?: string;
}

export class MemoryDBReadonly {
  private db: any;

  constructor(bookDir: string) {
    const { DatabaseSync } = require("node:sqlite");
    this.db = new DatabaseSync(join(bookDir, "story", "memory.db"));
    // 631 号：并发写者等待而非瞬时 SQLITE_BUSY（Rust 侧同值同批）。
    this.db.exec("PRAGMA busy_timeout = 5000");
    // 只读语义：不执行 fork 构造器的 migrate()（不建表不迁移）。
  }

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
    ).all(fromChapter, toChapter) as unknown as ReadonlyArray<StoredSummary>;
  }

  listReviewMetrics(): ReadonlyArray<ReviewMetricRow> {
    return this.db.prepare(
      `SELECT chapter, overall_score AS overallScore, passed, critical_count AS criticalCount,
              warning_count AS warningCount, info_count AS infoCount,
              content_hash AS contentHash, recorded_at AS recordedAt
       FROM review_metrics ORDER BY chapter`,
    ).all().map((row: any) => ({
      ...row,
      overallScore: typeof row.overallScore === "number" ? row.overallScore : undefined,
      passed: Number(row.passed) === 1,
    })) as unknown as ReadonlyArray<ReviewMetricRow>;
  }

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
       ORDER BY created_at ASC`,
    ).all(...params) as ReadonlyArray<StoredQualityDebt>;
    return rows;
  }
}
