//! 状态域（运行时状态机）。
//!
//! 移植自 `packages/core/src/state/`。当前：
//! - [`validator`]：validateRuntimeState（快照结构 + 语义校验）
//!
//! ## 待移植（依赖未移植模块）
//! state-reducer（依赖 hook-governance + hook-lifecycle + validator）/ state-projections /
//! memory-db（node:sqlite）/ manager / state-bootstrap（持久化编排）。

pub mod reducer;
pub mod validator;
