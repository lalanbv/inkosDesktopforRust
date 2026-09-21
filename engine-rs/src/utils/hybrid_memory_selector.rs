//! 混合召回记忆精选器（G1/350a 号；545 号接线）。
//!
//! TS 真源：`packages/core/src/retrieval/hybrid-memory-selector.ts`。
//! BM25 候选的向量重排：候选指纹对比 `MemoryDb.retrieval_chunks` 缓存
//! （写面随本接线首次激活），仅增量嵌入；查询与候选余弦 topN 精选。
//! 降级契约（347 号）：embedding 任何失败 → `Ok(vec![])`，
//! 调用方回退 BM25 排序（行为不变）。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::state::memory_db::{MemoryDb, StoredChunkVector};
use crate::utils::memory_retrieval::{MemorySemanticSelectionRequest, MemorySemanticSelector};
use crate::utils::semantic_retrieval::{
    chunk_fingerprint, embed_batch, select_stale_chunks, top_k_by_similarity, ChunkVector,
    EmbeddingChunk, EmbeddingConfig,
};

/// 精选数量上限（TS `HybridSelectorOptions.limit` 缺省 6，对齐 LLM 精选量级）。
pub const HYBRID_SELECT_LIMIT: usize = 6;

/// embedding 出口端口：生产 = [`ProductionEmbedder`]（双协议 embed_batch），
/// 测试注入 mock（对齐 TS `EmbeddingClient` 接口形态）。
#[async_trait]
pub trait EmbedFn: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f64>>, String>;
}

/// 生产 embedder：`llm.embedding` 配置 + apiKeyEnv 环境变量在装配处解析。
pub struct ProductionEmbedder {
    pub config: EmbeddingConfig,
    pub api_key: String,
}

#[async_trait]
impl EmbedFn for ProductionEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f64>>, String> {
        embed_batch(&self.config, texts, Some(&self.api_key)).await
    }
}

pub struct HybridMemorySelector {
    embedder: Arc<dyn EmbedFn>,
    /// rusqlite Connection !Sync——包 Mutex 满足 MemorySemanticSelector 的
    /// Send + Sync；锁内均为同步调用（list/upsert），无跨 await 持锁。
    cache: std::sync::Mutex<MemoryDb>,
    limit: usize,
}

impl HybridMemorySelector {
    pub fn new(embedder: Arc<dyn EmbedFn>, cache: MemoryDb, limit: usize) -> Self {
        Self { embedder, cache: std::sync::Mutex::new(cache), limit }
    }

    async fn run_selection(
        &self,
        request: &MemorySemanticSelectionRequest<'_>,
    ) -> Result<Vec<String>, String> {
        if request.candidates.is_empty() {
            return Ok(Vec::new());
        }
        let query_vector = self
            .embedder
            .embed(std::slice::from_ref(&request.query.to_string()))
            .await?
            .into_iter()
            .next()
            .unwrap_or_default();
        if query_vector.is_empty() {
            return Ok(Vec::new());
        }

        let mut cached: HashMap<String, (String, Vec<f64>)> = self
            .cache
            .lock()
            .unwrap()
            .list_chunk_vectors()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|chunk| (chunk.chunk_id, (chunk.fingerprint, chunk.vector)))
            .collect();

        // 增量嵌入：仅指纹与缓存不一致（含无缓存）的候选；文本 = excerpt 空回退 title。
        let cached_fingerprints: HashMap<String, String> = cached
            .iter()
            .map(|(id, (fingerprint, _))| (id.clone(), fingerprint.clone()))
            .collect();
        let stale_inputs: Vec<EmbeddingChunk> = request
            .candidates
            .iter()
            .map(|candidate| EmbeddingChunk {
                id: candidate.id.clone(),
                text: if candidate.excerpt.is_empty() {
                    candidate.title.clone()
                } else {
                    candidate.excerpt.clone()
                },
            })
            .collect();
        let stale = select_stale_chunks(&stale_inputs, &cached_fingerprints);
        if !stale.is_empty() {
            let texts: Vec<String> = stale.iter().map(|chunk| chunk.text.clone()).collect();
            let vectors = self.embedder.embed(&texts).await?;
            let now = crate::utils::utc_time::utc_now_iso();
            for (chunk, vector) in stale.iter().zip(vectors) {
                if vector.is_empty() {
                    continue;
                }
                let fingerprint = chunk_fingerprint(&chunk.text);
                let source = request
                    .candidates
                    .iter()
                    .find(|candidate| candidate.id == chunk.id)
                    .map(|candidate| candidate.source.clone())
                    .unwrap_or_default();
                self.cache
                    .lock()
                    .unwrap()
                    .upsert_chunk_vector(&StoredChunkVector {
                        chunk_id: chunk.id.clone(),
                        source,
                        fingerprint: fingerprint.clone(),
                        dim: vector.len() as u32,
                        vector: vector.clone(),
                        created_at: now.clone(),
                    })
                    .map_err(|e| e.to_string())?;
                cached.insert(chunk.id.clone(), (fingerprint, vector));
            }
        }

        // 向量重排：全部候选取缓存最新向量（新嵌 + 已缓存；缺失补空向量），
        // 余弦 topN（维度不匹配的 chunk 由 top_k_by_similarity 跳过，双端同构）。
        let chunk_inputs: Vec<ChunkVector> = request
            .candidates
            .iter()
            .map(|candidate| ChunkVector {
                id: candidate.id.clone(),
                vector: cached
                    .get(&candidate.id)
                    .map(|(_, vector)| vector.clone())
                    .unwrap_or_default(),
            })
            .collect();
        let ranked = top_k_by_similarity(&query_vector, &chunk_inputs, self.limit);
        Ok(ranked.into_iter().map(|hit| hit.id).collect())
    }
}

#[async_trait]
impl MemorySemanticSelector for HybridMemorySelector {
    async fn select(
        &self,
        request: &MemorySemanticSelectionRequest<'_>,
    ) -> Result<Vec<String>, String> {
        // 降级契约（347 号）：embedding 任何失败 → 空精选（TS catch → []）。
        self.run_selection(request).await.or(Ok(Vec::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::memory_retrieval::MemoryCandidate;

    fn vector_of(text: &str) -> Vec<f64> {
        match text {
            "青镇旧事 怀表" => vec![0.95, 0.15],
            "关于怀表的悬念" => vec![1.0, 0.0],
            "王胖的日常" => vec![0.0, 1.0],
            "青镇旧事" => vec![0.9, 0.1],
            _ => vec![0.0, 0.0],
        }
    }

    struct MockEmbedder {
        calls: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    }

    // EmbedFn 的 async_trait 方法在 #[tokio::test] 内直接 await。
    #[async_trait]
    impl EmbedFn for MockEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f64>>, String> {
            self.calls.lock().unwrap().push(texts.to_vec());
            Ok(texts.iter().map(|text| vector_of(text)).collect())
        }
    }

    struct FailEmbedder;

    #[async_trait]
    impl EmbedFn for FailEmbedder {
        async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f64>>, String> {
            Err("embedding service down".to_string())
        }
    }

    fn candidate(id: &str, title: &str, excerpt: &str) -> MemoryCandidate {
        MemoryCandidate {
            id: id.to_string(),
            kind: "summary".to_string(),
            source: format!("story/chapter_summaries.md#{id}"),
            title: title.to_string(),
            excerpt: excerpt.to_string(),
        }
    }

    /// 镜像 TS「reranks candidates by query similarity and caches embeddings」：
    /// 指纹增量嵌入（query 与候选分两批）、余弦 topN、二轮仅嵌 query。
    #[tokio::test]
    async fn reranks_by_similarity_and_caches_embeddings() {
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let selector = HybridMemorySelector::new(
            Arc::new(MockEmbedder { calls: Arc::clone(&calls) }),
            MemoryDb::open_in_memory().unwrap(),
            2,
        );
        let candidates = [
            candidate("s-2", "日常", "王胖的日常"),
            candidate("s-1", "悬念", "关于怀表的悬念"),
            candidate("s-3", "旧事", "青镇旧事"),
        ];
        let request = MemorySemanticSelectionRequest {
            chapter_number: 4,
            query: "青镇旧事 怀表",
            candidates: &candidates,
        };

        let first = selector.select(&request).await.unwrap();
        assert_eq!(first, vec!["s-3".to_string(), "s-1".to_string()]);
        {
            let calls = calls.lock().unwrap();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0], vec!["青镇旧事 怀表".to_string()]);
            assert_eq!(
                calls[1],
                vec![
                    "王胖的日常".to_string(),
                    "关于怀表的悬念".to_string(),
                    "青镇旧事".to_string()
                ]
            );
        }
        assert_eq!(selector.cache.lock().unwrap().chunk_vector_count().unwrap(), 3);

        // 二轮同候选：全部缓存命中，仅嵌入 query。
        let second = selector.select(&request).await.unwrap();
        assert_eq!(second, vec!["s-3".to_string(), "s-1".to_string()]);
        assert_eq!(calls.lock().unwrap().len(), 3);
        assert_eq!(
            calls.lock().unwrap()[2],
            vec!["青镇旧事 怀表".to_string()]
        );
    }

    /// 镜像 TS「returns empty on embedding failure」：降级契约返回空精选。
    #[tokio::test]
    async fn returns_empty_on_embedding_failure() {
        let selector = HybridMemorySelector::new(
            Arc::new(FailEmbedder),
            MemoryDb::open_in_memory().unwrap(),
            2,
        );
        let candidates = [candidate("a", "t", "e")];
        let request = MemorySemanticSelectionRequest {
            chapter_number: 1,
            query: "任意",
            candidates: &candidates,
        };
        assert_eq!(selector.select(&request).await.unwrap(), Vec::<String>::new());
    }

    /// 镜像 TS「short-circuits empty candidate lists」：空候选零嵌入调用。
    #[tokio::test]
    async fn short_circuits_empty_candidates() {
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let selector = HybridMemorySelector::new(
            Arc::new(MockEmbedder { calls: Arc::clone(&calls) }),
            MemoryDb::open_in_memory().unwrap(),
            HYBRID_SELECT_LIMIT,
        );
        let request = MemorySemanticSelectionRequest {
            chapter_number: 1,
            query: "x",
            candidates: &[],
        };
        assert_eq!(selector.select(&request).await.unwrap(), Vec::<String>::new());
        assert!(calls.lock().unwrap().is_empty());
    }
}
