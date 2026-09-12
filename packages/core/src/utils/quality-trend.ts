/**
 * R3 质量趋势与回灌幂等（360 号，二轮 P0 第三件契约层）。
 *
 * 1. `buildQualityTrend`：review_metrics 行 → 章级趋势点列（升序 + 均值 +
 *    未过章清单），供 `/books/:id/quality-trend` 端点与 Dashboard 趋势卡；
 * 2. `reviewMetricContentHash` / `dedupeReplayRows`：回灌链路幂等——哈希
 *    输入不含时间戳（重放时间不同但内容相同必须命中同一指纹），对齐 ANWA
 *    ChapterArtifactSyncCheckpoint 语义；
 * 3. （promise-ledger 紧迫度见 promise-ledger.ts 扩展。）
 *
 * 双端：`engine-rs/src/utils/quality_trend.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/quality-trend-vectors.json`（TS 断言
 * `golden-quality-trend.test.ts`；Rust 差分 `tests/golden_quality_trend_diff.rs`）。
 */

export interface ReviewMetricRow {
  readonly chapter: number;
  /** 0–100 整体质量分（auditor 支持评分时才有；缺分章不进均值）。 */
  readonly overallScore?: number;
  readonly passed: boolean;
  readonly criticalCount: number;
  readonly warningCount: number;
  readonly infoCount: number;
  /** 记录时间（ISO）；不参与幂等哈希。 */
  readonly recordedAt: string;
}

export interface QualityTrendPoint {
  readonly chapter: number;
  readonly overallScore?: number;
  readonly passed: boolean;
  readonly issueCount: number;
}

export interface QualityTrend {
  readonly points: ReadonlyArray<QualityTrendPoint>;
  readonly scoredChapters: number;
  /** 有分章均值，一位小数；全部缺分时 undefined。 */
  readonly averageScore?: number;
  readonly failingChapters: ReadonlyArray<number>;
}

/** 章级趋势聚合：升序稳定排序（禁 localeCompare），缺分章保留点位但不进均值。 */
export function buildQualityTrend(rows: ReadonlyArray<ReviewMetricRow>): QualityTrend {
  const sorted = [...rows].sort((a, b) => a.chapter - b.chapter);
  const points = sorted.map((row) => ({
    chapter: row.chapter,
    overallScore: row.overallScore,
    passed: row.passed,
    issueCount: row.criticalCount + row.warningCount + row.infoCount,
  }));
  const scored = sorted.filter((row) => typeof row.overallScore === "number");
  const averageScore = scored.length > 0
    ? Math.round(
      (scored.reduce((sum, row) => sum + (row.overallScore as number), 0) / scored.length) * 10,
    ) / 10
    : undefined;
  return {
    points,
    scoredChapters: scored.length,
    averageScore,
    failingChapters: sorted.filter((row) => !row.passed).map((row) => row.chapter),
  };
}

const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const FNV_MASK = 0xffffffffffffffffn;

function fnv1aHex16(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let hash = FNV_OFFSET;
  for (const byte of bytes) {
    hash ^= BigInt(byte);
    hash = (hash * FNV_PRIME) & FNV_MASK;
  }
  return hash.toString(16).padStart(16, "0");
}

/**
 * 回灌幂等指纹：FNV-1a 64（UTF-8，hex16，对齐 348 号 chunkFingerprint 口径）。
 * 输入 = chapter|score|passed|critical|warning|info（**不含 recordedAt**——
 * 重放时间不同但内容相同必须命中同一指纹）。
 */
export function reviewMetricContentHash(
  row: Omit<ReviewMetricRow, "recordedAt">,
): string {
  const score = typeof row.overallScore === "number" ? String(row.overallScore) : "-";
  return fnv1aHex16(
    `${row.chapter}|${score}|${row.passed ? 1 : 0}|${row.criticalCount}|${row.warningCount}|${row.infoCount}`,
  );
}

/** 幂等重放：按内容指纹去重（同内容仅首次 fresh；保序；时间戳不同的重复行同判）。 */
export function dedupeReplayRows(
  rows: ReadonlyArray<ReviewMetricRow>,
): { fresh: ReadonlyArray<ReviewMetricRow>; skipped: number } {
  const seen = new Set<string>();
  const fresh: ReviewMetricRow[] = [];
  for (const row of rows) {
    const { recordedAt: _recordedAt, ...content } = row;
    const hash = reviewMetricContentHash(content);
    if (seen.has(hash)) continue;
    seen.add(hash);
    fresh.push(row);
  }
  return { fresh, skipped: rows.length - fresh.length };
}
