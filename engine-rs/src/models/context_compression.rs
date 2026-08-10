//! 上下文压缩事件。
//!
//! 移植自 `packages/core/src/models/context-compression.ts`（14 行，纯类型）。
//! 注：TS 的 `ContextCompressionCallback` 是函数类型，Rust 端用 `Fn(&ContextCompressionEvent)`
//! 闭包在调用处表达（不单独建模为类型）。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 压缩类别。对齐 TS `"session_context" | "story_context"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"session_context\" | \"story_context\""))]
pub enum ContextCompressionCategory {
    #[serde(rename = "session_context")]
    SessionContext,
    #[serde(rename = "story_context")]
    StoryContext,
}

/// 压缩阶段。对齐 TS `"start" | "end" | "error"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"start\" | \"end\" | \"error\""))]
pub enum ContextCompressionPhase {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "end")]
    End,
    #[serde(rename = "error")]
    Error,
}

/// 上下文压缩事件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ContextCompressionEvent {
    pub category: ContextCompressionCategory,
    pub phase: ContextCompressionPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protected_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressible_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<String>>,
}
