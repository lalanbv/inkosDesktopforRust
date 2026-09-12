/**
 * G11 best-of-N（371 号契约层，择机项启动；328/354 号清算结论）。
 *
 * 质量优先时的多版选优：首版审查分数低于 minScore 时，追加生成
 * `candidates - 1` 版候选；候选间按审查分数选优（分数降序稳定，缺分最低，
 * 同分保序）。评分走既定 continuity 管道（AuditResult.overallScore 0–100），
 * 选优结果再接 qualityGates（354 号前置已就位）。
 *
 * 双端：`engine-rs/src/utils/best_of_n.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/best-of-n-vectors.json`（TS 断言
 * `golden-best-of-n.test.ts`；Rust 差分 `tests/golden_best_of_n_diff.rs`）。
 */

export interface BestOfNConfig {
  enabled?: boolean;
  /** 候选总数（含首版），2–3；缺省 2。 */
  candidates?: number;
  /** 触发追加候选的分数下限（0–100，含边界比较：score < minScore 才扩展）。 */
  minScore?: number;
}

export const BEST_OF_N_DEFAULTS = {
  candidates: 2,
  minScore: 75,
} as const;

export interface BestOfNPlan {
  /** 是否启用。 */
  readonly enabled: boolean;
  /** 首版之后还需生成的候选数（0 = 只用首版）。 */
  readonly extraCandidates: number;
  /** 候选总数（含首版）。 */
  readonly totalCandidates: number;
  readonly minScore: number;
}

/**
 * 计划解析：未启用 → 0 追加；启用 → 首版分数已知且 ≥ minScore 时 0 追加
 * （首版够好）；分数未知（审查未评分）按需扩展（保守触发）。
 */
export function resolveBestOfNPlan(
  config: BestOfNConfig | undefined,
  firstScore?: number,
): BestOfNPlan {
  const enabled = config?.enabled === true;
  const candidates = Math.min(3, Math.max(2, Math.trunc(config?.candidates ?? BEST_OF_N_DEFAULTS.candidates)));
  const minScore = config?.minScore ?? BEST_OF_N_DEFAULTS.minScore;
  if (!enabled) {
    return { enabled: false, extraCandidates: 0, totalCandidates: 1, minScore };
  }
  const needsMore = firstScore === undefined || firstScore < minScore;
  return {
    enabled: true,
    extraCandidates: needsMore ? candidates - 1 : 0,
    totalCandidates: candidates,
    minScore,
  };
}

export interface ScoredCandidate<T> {
  readonly payload: T;
  /** 审查分数 0–100；缺分视为最低优先。 */
  readonly score?: number;
}

/**
 * 候选选优：score 降序稳定排序（同分保持输入序；缺分排在有分之后，
 * 缺分之间保序）。返回胜者 payload 与其排序位置信息。
 */
export function selectBestCandidate<T>(
  candidates: ReadonlyArray<ScoredCandidate<T>>,
): { winner: T; index: number; score?: number } {
  if (candidates.length === 0) {
    throw new Error("selectBestCandidate requires at least one candidate");
  }
  const order = candidates
    .map((candidate, index) => ({ candidate, index }))
    .sort((left, right) => {
      const leftScore = left.candidate.score;
      const rightScore = right.candidate.score;
      if (leftScore === undefined && rightScore === undefined) return left.index - right.index;
      if (leftScore === undefined) return 1;
      if (rightScore === undefined) return -1;
      return rightScore - leftScore || left.index - right.index;
    });
  const best = order[0]!;
  return { winner: best.candidate.payload, index: best.index, score: best.candidate.score };
}
