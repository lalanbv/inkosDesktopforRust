//! Context Lens——上下文装配透明投影（R21/391 号，四轮 P0）。
//!
//! TS 真源：`packages/core/src/utils/context-lens.ts`。纯函数零 IO：
//! 每章治理落盘的 `chapter-NNNN.context.json`（预算后实际进入 prompt 的包）
//! 与 `chapter-NNNN.trace.json`（治理留痕）投影成单一只读视图。
//! 共享向量 `packages/core/src/__tests__/golden/context-lens-vectors.json`
//! 差分：`tests/golden_context_lens_diff.rs`。

use serde::Serialize;
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

use crate::models::input_governance::{
    ChapterTrace, ContextPackage, ContextSourceRank, TraceSourceTokens,
};
use crate::utils::context_assembly::{
    estimate_context_source_tokens, is_protected_context_source,
};
use crate::utils::context_source_tier::context_source_tier;

pub const CONTEXT_LENS_VERSION: u32 = 1;

/// 层内排序特征透传（复用 ContextSourceRank 序列化：缺省维度省略）。
pub type ContextLensRank = ContextSourceRank;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ContextLensEntry {
    /// 1 起装配序（context.json 数组序，即 G2 优先级契约的最终序）。
    pub order: u32,
    pub source: String,
    pub tier: String,
    pub tier_precedence: i32,
    pub protected: bool,
    pub tokens: u64,
    pub compiled: bool,
    pub rank: Option<ContextSourceRank>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ContextLensCompression {
    pub compiled_source: String,
    pub budget_tokens: u64,
    pub protected_tokens: u64,
    pub compressible_tokens: u64,
    /// 压缩前被编译的原始可压缩来源与逐源 token（trace.compression.sourceTokens 透传）。
    pub pre_compression_sources: Vec<TraceSourceTokens>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ContextLensTotals {
    pub entries: usize,
    pub protected_entries: usize,
    pub compiled_entries: usize,
    pub tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ContextLens {
    pub version: u32,
    pub chapter: u32,
    pub entries: Vec<ContextLensEntry>,
    /// 无留痕时序列化为 null（与 TS `compression: null` 对齐，不加 skip）。
    pub compression: Option<ContextLensCompression>,
    pub notes: Vec<String>,
    pub totals: ContextLensTotals,
}

/// 上下文装配透明投影：context.json（实况）× trace.json（留痕）→ 单一只读视图。
pub fn build_context_lens(context_package: &ContextPackage, trace: &ChapterTrace) -> ContextLens {
    let compression = trace.compression.as_ref();
    let entries: Vec<ContextLensEntry> = context_package
        .selected_context
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let tier = context_source_tier(&entry.source);
            ContextLensEntry {
                order: index as u32 + 1,
                source: entry.source.clone(),
                tier: tier.as_str().to_string(),
                tier_precedence: tier.precedence(),
                protected: is_protected_context_source(&entry.source),
                tokens: u64::from(estimate_context_source_tokens(entry)),
                compiled: compression
                    .map(|trail| trail.compiled_source == entry.source)
                    .unwrap_or(false),
                rank: entry.rank.clone(),
            }
        })
        .collect();
    let totals = ContextLensTotals {
        entries: entries.len(),
        protected_entries: entries.iter().filter(|entry| entry.protected).count(),
        compiled_entries: entries.iter().filter(|entry| entry.compiled).count(),
        tokens: entries.iter().map(|entry| entry.tokens).sum(),
    };
    ContextLens {
        version: CONTEXT_LENS_VERSION,
        chapter: context_package.chapter,
        entries,
        compression: compression.map(|trail| ContextLensCompression {
            compiled_source: trail.compiled_source.clone(),
            budget_tokens: trail.budget_tokens,
            protected_tokens: trail.protected_tokens,
            compressible_tokens: trail.compressible_tokens,
            pre_compression_sources: trail
                .source_tokens
                .iter()
                .map(|item| TraceSourceTokens {
                    source: item.source.clone(),
                    tokens: item.tokens,
                })
                .collect(),
        }),
        notes: trace.notes.clone(),
        totals,
    }
}
