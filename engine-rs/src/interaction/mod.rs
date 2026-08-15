//! 交互域（用户意图/请求/执行状态）。
//!
//! 移植自 `packages/core/src/interaction/` 的类型 + 纯函数子集：
//! - [`modes`] / [`events`] / [`intents`] / [`export_artifact`] /
//!   [`edit_controller`]（49 号：chapter-replace 编辑事务）
//!
//! ## 待移植（需 state + llm）
//! runtime（1153 行交互运行时）/ edit-controller 其余 kind /
//! project-tools / session-transcript 等。

pub mod book_session_store;
pub mod edit_controller;
pub mod events;
pub mod export_artifact;
pub mod intents;
pub mod modes;
pub mod session;
pub mod session_restore;
pub mod session_transcript;
