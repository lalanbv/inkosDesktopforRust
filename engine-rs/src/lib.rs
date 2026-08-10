//! # inkos-engine
//!
//! packages/core（TS，~107k 行）的 Rust 移植目标。Phase 0 仅提供模块骨架——
//! 每个域一个模块，与 `packages/core/src` 的 16 个业务域一一对应。
//!
//! ## 现状：占位
//! 所有模块当前为空（或仅含类型骨架）。按迁移规划 v1 的自下而上顺序逐域填充：
//! - Phase 1（叶子）：[`models`] / [`translation`] / [`utils`]
//! - Phase 2（中层）：[`notify`] / [`materials`] / [`prompts`] / [`state`] / [`skills`]
//! - Phase 3（高层）：[`pipeline`] / [`play`] / [`interaction`] / [`forecast`]
//!   / [`interactive_film`] / [`llm`] / [`agent`] / [`agents`]
//!
//! ## 接入方式（未来）
//! 1. 作为库被 `src-tauri` 依赖（Tauri 命令直接调）
//! 2. 或被独立 axum HTTP 服务挂载（保持 `/api/v1/*` 契约，前端无感切换）
//!
//! ## 类型同步
//! canonical 类型定义在 [`models`]，`#[cfg(feature = "export-bindings")]` 时
//! 经 ts-rs 生成 `.ts` 供前端消费。详见 [`models`] 模块文档。

pub mod agent;
pub mod agents;
pub mod forecast;
pub mod interaction;
pub mod interactive_film;
pub mod llm;
pub mod materials;
pub mod models;
pub mod notify;
pub mod pipeline;
pub mod play;
pub mod prompts;
pub mod server;
pub mod skills;
pub mod state;
pub mod translation;
pub mod utils;

/// crate 级错误类型（与壳层 AppError 同模式：thiserror + 上下文）。
/// 域内错误各模块自定义，按需 `#[from]` 转入此处。
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("LLM 调用失败: {0}")]
    Llm(String),
    #[error("业务约束违反: {0}")]
    Constraint(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// crate 版本（与 packages/core 的 1.7.2 对齐跟踪，移植完成前保持 0.0.x）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
