/**
 * 语义检索层契约（G1/347 号，Phase C 首项首批；ANWA A2 任务驱动 RAG 的采纳）。
 *
 * 既有检索 = FTS5 BM25 词法单内核（local-search 双端）。本模块补齐语义层
 * **确定性契约**，embedding 服务（OpenAI 兼容/ollama）后续号接入：
 *
 * 1. **任务驱动查询构造** `buildTaskDrivenQuery`：查询由章任务单组装
 *    （goal + outlineNode + mustKeep + threadRefs/hook 记号），不是拿章标题去搜；
 * 2. **向量相似度与 topK** `cosineSimilarity` / `topKBySimilarity`：
 *    chunk 向量（embedding 产物）与查询向量的余弦召回，零向量恒 0 分，
 *    并列按 id 码元序（双端确定）；
 * 3. **混合召回融合** `reciprocalRankFusion`：语义排名与 FTS5 BM25 排名的
 *    RRF 融合（k=60 经典常数），两路召回互救；
 * 4. **降级契约** `resolveRetrievalMode`：embedding 未配置/失败 → fts5-fallback
 *    （词法单内核，现有行为），绝不因语义层缺失而不可用。

 * 双端：`engine-rs/src/utils/semantic_retrieval.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/semantic-retrieval-vectors.json`。
 */

export const RETRIEVAL_MODES = ["semantic", "fts5-fallback"] as const;
export type RetrievalMode = (typeof RETRIEVAL_MODES)[number];

/** embedding 客户端端口（OpenAI 兼容/ollama 后续号接入；本号只定契约）。 */
export interface EmbeddingClient {
  /** 批量产出文本向量（维度由服务决定，同一次调用内必须一致）。 */
  embed(texts: ReadonlyArray<string>): Promise<ReadonlyArray<ReadonlyArray<number>>>;
}

export interface TaskQueryInput {
  /** 本章目标（查询主干）。 */
  readonly goal: string;
  readonly outlineNode?: string;
  readonly mustKeep?: ReadonlyArray<string>;
  /** 关联线索（planner threadRefs / hookId 记号）。 */
  readonly threadRefs?: ReadonlyArray<string>;
}

const QUERY_TERM_LIMIT = 8;
const QUERY_CHAR_LIMIT = 200;

/**
 * 任务驱动查询构造（确定性）：goal 主干 + outline + mustKeep/threadRefs 术语，
 * 去重、每段截断 40 字、总长截断 200 字。空 goal 时退化为术语拼接。
 */
export function buildTaskDrivenQuery(input: TaskQueryInput): string {
  const terms: string[] = [];
  const push = (value: string | undefined) => {
    const trimmed = (value ?? "").trim();
    if (!trimmed) return;
    if (terms.some((existing) => existing.toLowerCase() === trimmed.toLowerCase())) return;
    if (terms.length >= QUERY_TERM_LIMIT) return;
    terms.push(trimmed.length > 40 ? `${trimmed.slice(0, 39)}…` : trimmed);
  };
  push(input.goal);
  push(input.outlineNode);
  for (const item of input.mustKeep ?? []) push(item);
  for (const ref of input.threadRefs ?? []) push(ref);
  return terms.join("；");
}

/** 余弦相似度：零向量恒 0；维度不一致视为 0（防御，不抛错）。 */
export function cosineSimilarity(a: ReadonlyArray<number>, b: ReadonlyArray<number>): number {
  if (a.length === 0 || a.length !== b.length) return 0;
  let dot = 0;
  let normA = 0;
  let normB = 0;
  for (let i = 0; i < a.length; i++) {
    dot += a[i]! * b[i]!;
    normA += a[i]! * a[i]!;
    normB += b[i]! * b[i]!;
  }
  if (normA === 0 || normB === 0) return 0;
  return dot / (Math.sqrt(normA) * Math.sqrt(normB));
}

export interface ScoredChunk {
  readonly id: string;
  readonly similarity: number;
}

/** 向量 topK：相似度降序，并列按 id 码元序，截取前 k。 */
export function topKBySimilarity(
  queryVector: ReadonlyArray<number>,
  chunks: ReadonlyArray<{ readonly id: string; readonly vector: ReadonlyArray<number> }>,
  k: number,
): ScoredChunk[] {
  const scored: ScoredChunk[] = chunks
    .filter((chunk) => chunk.vector.length === queryVector.length && queryVector.length > 0)
    .map((chunk) => ({ id: chunk.id, similarity: cosineSimilarity(queryVector, chunk.vector) }));
  return scored
    .sort((a, b) => b.similarity - a.similarity || comparePlain(a.id, b.id))
    .slice(0, Math.max(0, k));
}

// ── 混合召回融合（RRF）──

export interface RankedHit {
  readonly id: string;
  readonly rank: number;
}

export interface FusedHit {
  readonly id: string;
  /** RRF 融合分：Σ 1/(k + rank)，双列表各自贡献。 */
  readonly score: number;
  readonly sources: ReadonlyArray<"semantic" | "fts5">;
}

const RRF_K = 60;

/**
 * RRF 融合（k=60）：语义排名与 FTS5 排名融合，双列表互救。
 * 同 id 双路命中 > 单路命中；分数降序，并列按 id 码元序。
 */
export function reciprocalRankFusion(
  semanticRanking: ReadonlyArray<RankedHit>,
  ftsRanking: ReadonlyArray<RankedHit>,
  k = RRF_K,
): FusedHit[] {
  const scores = new Map<string, { score: number; semantic: boolean; fts: boolean }>();
  const accumulate = (ranking: ReadonlyArray<RankedHit>, isSemantic: boolean) => {
    for (const hit of ranking) {
      const entry = scores.get(hit.id) ?? { score: 0, semantic: false, fts: false };
      entry.score += 1 / (k + hit.rank);
      if (isSemantic) entry.semantic = true;
      else entry.fts = true;
      scores.set(hit.id, entry);
    }
  };
  accumulate(semanticRanking, true);
  accumulate(ftsRanking, false);
  return [...scores.entries()]
    .map(([id, { score, semantic, fts }]) => ({
      id,
      score,
      sources: [semantic ? ("semantic" as const) : null, fts ? ("fts5" as const) : null].filter(
        (x): x is "semantic" | "fts5" => x !== null,
      ),
    }))
    .sort((a, b) => b.score - a.score || comparePlain(a.id, b.id));
}

// ── 降级契约 ──

/**
 * 检索模式判定：embedding 可用 → semantic（向量 + FTS5 混合召回）；
 * 未配置或调用失败 → fts5-fallback（现有词法单内核，行为不变）。
 */
export function resolveRetrievalMode(params: {
  readonly embeddingAvailable: boolean;
  readonly embeddingError?: boolean;
}): RetrievalMode {
  return params.embeddingAvailable && !params.embeddingError ? "semantic" : "fts5-fallback";
}

/** 纯码元比较（禁 localeCompare——排序跨端确定）。 */
function comparePlain(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}
