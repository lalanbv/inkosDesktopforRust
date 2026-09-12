//! R12 sqlite-vec 真向量检索升级（380 号契约批，三轮 P0 末件）。
//!
//! TS 真源：`packages/core/src/utils/vector-engine.ts`；golden 唯一事实源：
//! `packages/core/src/__tests__/golden/vector-engine-vectors.json`（差分测试
//! `tests/golden_vector_engine_diff.rs` 读同一文件）。
//!
//! 决策表：可用性一票否决（扩展不可加载 → 永远内存余弦，347 行为不变）；
//! 规模超阈值（缺省 5000，严格大于）才切换 sqlite-vec。

pub const SQLITE_VEC_DEFAULT_THRESHOLD: i64 = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VectorEngine {
    MemoryCosine,
    SqliteVec,
}

impl VectorEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            VectorEngine::MemoryCosine => "memory-cosine",
            VectorEngine::SqliteVec => "sqlite-vec",
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorEngineContext {
    pub chunk_count: i64,
    pub vec_extension_available: bool,
    #[serde(default)]
    pub threshold: Option<i64>,
}

/// 引擎决策表：可用性一票否决，规模超阈值（严格大于）才切换。
pub fn resolve_vector_engine(context: &VectorEngineContext) -> VectorEngine {
    if !context.vec_extension_available {
        return VectorEngine::MemoryCosine;
    }
    let threshold = context.threshold.unwrap_or(SQLITE_VEC_DEFAULT_THRESHOLD);
    if context.chunk_count > threshold {
        VectorEngine::SqliteVec
    } else {
        VectorEngine::MemoryCosine
    }
}
