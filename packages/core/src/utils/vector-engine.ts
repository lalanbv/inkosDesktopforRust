/**
 * R12 sqlite-vec 真向量检索升级（380 号契约批，三轮 P0 末件；v3 §3 R12）。
 *
 * 引擎决策纯函数：混合召回（349/350a）当前以内存余弦扫描（347 号替代
 * 方案）；chunk 规模超阈值且 sqlite-vec 扩展可加载时切换 sqlite-vec，
 * 任何条件不满足**自动回退内存余弦**（347 行为不变）。
 *
 * 决策表（golden 锚定）：
 * - 扩展不可用 → memory-cosine（无论规模）；
 * - 扩展可用 + chunkCount > threshold（缺省 5000）→ sqlite-vec；
 * - 扩展可用 + chunkCount ≤ threshold → memory-cosine（小规模内存更快）。
 *
 * 双端：`engine-rs/src/utils/vector_engine.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/vector-engine-vectors.json`（TS 断言
 * `golden-vector-engine.test.ts`；Rust 差分 `tests/golden_vector_engine_diff.rs`）。
 */

export const SQLITE_VEC_DEFAULT_THRESHOLD = 5000;

export type VectorEngine = "memory-cosine" | "sqlite-vec";

export interface VectorEngineContext {
  /** 当前 chunk 总数（retrieval_chunks 表行数）。 */
  readonly chunkCount: number;
  /** sqlite-vec 扩展是否可加载（探测结果注入）。 */
  readonly vecExtensionAvailable: boolean;
  /** 启用阈值（缺省 5000）。 */
  readonly threshold?: number;
}

/** 引擎决策表：可用性一票否决，规模超阈值才切换。 */
export function resolveVectorEngine(context: VectorEngineContext): VectorEngine {
  if (!context.vecExtensionAvailable) return "memory-cosine";
  const threshold = context.threshold ?? SQLITE_VEC_DEFAULT_THRESHOLD;
  return context.chunkCount > threshold ? "sqlite-vec" : "memory-cosine";
}

/** 探测结果缓存键（env 覆盖路径名）。 */
export const SQLITE_VEC_PATH_ENV = "INKOS_SQLITE_VEC_PATH";
