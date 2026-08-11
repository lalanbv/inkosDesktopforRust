//! 状态域（运行时状态机）。
//!
//! 移植自 `packages/core/src/state/`。当前：
//! - [`validator`]：validateRuntimeState（快照结构 + 语义校验）
//! - [`memory_db`]：MemoryDB（node:sqlite → rusqlite，temporal facts + summaries + hooks）
//!
//! ## 待移植（依赖未移植模块）
//! state-reducer（依赖 hook-governance + hook-lifecycle + validator）/ state-projections /
//! manager / state-bootstrap（持久化编排）/ runtime-state-store（依赖本模块）。

pub mod chapter_word_sync;
pub mod memory_db;
pub mod projections;
pub mod reducer;
pub mod validator;
