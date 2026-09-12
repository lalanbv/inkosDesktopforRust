//! 语义检索层契约（G1/347 号，Phase C 首项首批）。
//!
//! TS 真源：`packages/core/src/retrieval/semantic-retrieval.ts`；共享向量：
//! `packages/core/src/__tests__/golden/semantic-retrieval-vectors.json`
//! （差分测试 `tests/golden_semantic_retrieval_diff.rs`）。
//!
//! 任务驱动查询构造 / 余弦相似度 / 向量 topK / RRF 混合融合 / FTS5 降级契约，
//! 语义详见 TS 模块 doc。

use serde::Deserialize;
use serde_json::Value;
use serde::Serialize;

pub const RETRIEVAL_MODES: [&str; 2] = ["semantic", "fts5-fallback"];

pub const QUERY_TERM_LIMIT: usize = 8;
pub const QUERY_CHAR_LIMIT: usize = 200;
const QUERY_TERM_CHARS: usize = 40;
const RRF_K: i64 = 60;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQueryInput {
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub outline_node: Option<String>,
    #[serde(default)]
    pub must_keep: Vec<String>,
    #[serde(default)]
    pub thread_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoredChunk {
    pub id: String,
    pub similarity: f64,
}

/// 任务驱动查询构造（确定性）：goal 主干 + outline + mustKeep/threadRefs 术语，
/// 去重（大小写不敏感）、每段截 40 字、上限 8 术语、总长截 200 字、分隔「；」。
pub fn build_task_driven_query(input: &TaskQueryInput) -> String {
    let mut terms: Vec<String> = Vec::new();
    let mut push = |value: &str, terms: &mut Vec<String>| {
        let trimmed = value.trim();
        if trimmed.is_empty() || terms.len() >= QUERY_TERM_LIMIT {
            return;
        }
        let lower = trimmed.to_lowercase();
        if terms.iter().any(|existing| existing.to_lowercase() == lower) {
            return;
        }
        let chars: Vec<char> = trimmed.chars().collect();
        if chars.len() > QUERY_TERM_CHARS {
            let head: String = chars[..QUERY_TERM_CHARS - 1].iter().collect();
            terms.push(format!("{head}…"));
        } else {
            terms.push(trimmed.to_string());
        }
    };
    push(&input.goal, &mut terms);
    if let Some(outline) = &input.outline_node {
        push(outline, &mut terms);
    }
    for item in &input.must_keep {
        push(item, &mut terms);
    }
    for reference in &input.thread_refs {
        push(reference, &mut terms);
    }
    let joined = terms.join("；");
    let joined: String = joined.chars().take(QUERY_CHAR_LIMIT).collect();
    joined
}

/// 余弦相似度：零向量恒 0；维度不一致视为 0（防御，不抛错）。
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChunkVector {
    pub id: String,
    pub vector: Vec<f64>,
}

/// 向量 topK：相似度降序，并列按 id 码元序，截取前 k；维度不匹配的 chunk 跳过。
pub fn top_k_by_similarity(
    query_vector: &[f64],
    chunks: &[ChunkVector],
    k: usize,
) -> Vec<ScoredChunk> {
    let mut scored: Vec<ScoredChunk> = chunks
        .iter()
        .filter(|chunk| chunk.vector.len() == query_vector.len() && !query_vector.is_empty())
        .map(|chunk| ScoredChunk {
            id: chunk.id.clone(),
            similarity: cosine_similarity(query_vector, &chunk.vector),
        })
        .collect();
    scored.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    scored.truncate(k);
    scored
}

// ── 混合召回融合（RRF）──

#[derive(Debug, Clone, Deserialize)]
pub struct RankedHit {
    pub id: String,
    pub rank: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FusedHit {
    pub id: String,
    pub score: f64,
    pub sources: Vec<String>,
}

/// RRF 融合（k=60）：语义排名与 FTS5 排名融合；分数降序，并列按 id 码元序。
pub fn reciprocal_rank_fusion(
    semantic_ranking: &[RankedHit],
    fts_ranking: &[RankedHit],
    k: i64,
) -> Vec<FusedHit> {
    let mut order: Vec<String> = Vec::new();
    let mut scores: std::collections::HashMap<String, (f64, bool, bool)> =
        std::collections::HashMap::new();
    let mut accumulate = |ranking: &[RankedHit], semantic: bool, order: &mut Vec<String>| {
        for hit in ranking {
            let entry = scores.entry(hit.id.clone()).or_insert((0.0, false, false));
            entry.0 += 1.0 / (k as f64 + hit.rank as f64);
            if semantic {
                entry.1 = true;
            } else {
                entry.2 = true;
            }
            if !order.contains(&hit.id) {
                order.push(hit.id.clone());
            }
        }
    };
    accumulate(semantic_ranking, true, &mut order);
    accumulate(fts_ranking, false, &mut order);

    let mut fused: Vec<FusedHit> = scores
        .into_iter()
        .map(|(id, (score, semantic, fts))| {
            let mut sources = Vec::new();
            if semantic {
                sources.push("semantic".to_string());
            }
            if fts {
                sources.push("fts5".to_string());
            }
            FusedHit { id, score, sources }
        })
        .collect();
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    fused
}

/// 检索模式判定：embedding 可用 → semantic；未配置或失败 → fts5-fallback。
pub fn resolve_retrieval_mode(embedding_available: bool, embedding_error: bool) -> &'static str {
    if embedding_available && !embedding_error {
        "semantic"
    } else {
        "fts5-fallback"
    }
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticRetrievalContract {
    pub modes: Vec<&'static str>,
    pub rrf_k: i64,
    #[serde(rename = "queryTermLimit")]
    pub query_term_limit: usize,
    #[serde(rename = "queryCharLimit")]
    pub query_char_limit: usize,
    #[serde(rename = "dialogueWindowNote")]
    pub dialogue_window_note: &'static str,
}

pub fn semantic_retrieval_contract() -> SemanticRetrievalContract {
    SemanticRetrievalContract {
        modes: RETRIEVAL_MODES.to_vec(),
        rrf_k: RRF_K,
        query_term_limit: QUERY_TERM_LIMIT,
        query_char_limit: QUERY_CHAR_LIMIT,
        dialogue_window_note: "embedding 服务后续号接入；本号只定确定性契约",
    }
}

// ── chunk 指纹与增量向量化选择（G1/348 号）──

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a 64 位内容指纹（UTF-8 字节序，hex 16 位小写；双端一致的确定性锚）。
pub fn chunk_fingerprint(text: &str) -> String {
    let mut hash: u64 = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingChunk {
    pub id: String,
    pub text: String,
}

/// 增量向量化选择：指纹与缓存不一致（含无缓存）的 chunk 需要重新嵌入。
pub fn select_stale_chunks(
    chunks: &[EmbeddingChunk],
    cached: &std::collections::HashMap<String, String>,
) -> Vec<EmbeddingChunk> {
    chunks
        .iter()
        .filter(|chunk| cached.get(&chunk.id).map(|f| f.as_str()) != Some(chunk_fingerprint(&chunk.text).as_str()))
        .cloned()
        .collect()
}

// ── embedding 配置与双协议客户端（G1/348 号）──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddingProvider {
    OpenAICompatible,
    Ollama,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingConfig {
    pub provider: EmbeddingProvider,
    pub base_url: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "timeoutMs")]
    pub timeout_ms: Option<u64>,
}

pub const DEFAULT_EMBEDDING_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedBatch {
    pub vectors: Vec<Vec<f64>>,
}

/// 双协议批量嵌入（OpenAI 兼容 /embeddings + Bearer；ollama /api/embed 免 key）。
/// 网络失败向上抛错，由调用方按降级契约回退 FTS5。
pub async fn embed_batch(
    config: &EmbeddingConfig,
    texts: &[String],
    api_key: Option<&str>,
) -> Result<Vec<Vec<f64>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let client = reqwest::Client::new();
    let timeout = std::time::Duration::from_millis(
        config.timeout_ms.unwrap_or(DEFAULT_EMBEDDING_TIMEOUT_MS),
    );
    let url = match config.provider {
        EmbeddingProvider::OpenAICompatible => {
            format!("{}/embeddings", config.base_url.trim_end_matches('/'))
        }
        EmbeddingProvider::Ollama => {
            format!("{}/api/embed", config.base_url.trim_end_matches('/'))
        }
    };
    let mut request = client
        .post(&url)
        .timeout(timeout)
        .header("Content-Type", "application/json");
    if config.provider == EmbeddingProvider::OpenAICompatible {
        if let Some(key) = api_key.filter(|key| !key.is_empty()) {
            request = request.bearer_auth(key);
        }
    }
    let body = match config.provider {
        EmbeddingProvider::OpenAICompatible => {
            serde_json::json!({ "model": config.model, "input": texts })
        }
        EmbeddingProvider::Ollama => {
            serde_json::json!({ "model": config.model, "input": texts })
        }
    };
    let response = request.json(&body).send().await.map_err(|e| e.to_string())?;
    let json: Value = response.json().await.map_err(|e| e.to_string())?;
    if let Some(data) = json.get("data").and_then(Value::as_array) {
        let vectors: Vec<Vec<f64>> = data
            .iter()
            .filter_map(|entry| {
                entry
                    .get("embedding")
                    .and_then(Value::as_array)
                    .map(|nums| nums.iter().filter_map(Value::as_f64).collect())
            })
            .collect();
        return Ok(vectors);
    }
    if let Some(embeddings) = json.get("embeddings").and_then(Value::as_array) {
        let vectors: Vec<Vec<f64>> = embeddings
            .iter()
            .map(|nums| {
                nums.as_array()
                    .map(|items| items.iter().filter_map(Value::as_f64).collect())
                    .unwrap_or_default()
            })
            .collect();
        return Ok(vectors);
    }
    Err("embedding response missing vectors".to_string())
}
