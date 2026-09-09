//! 交互域（用户意图/请求/执行状态）。
//!
//! 移植自 `packages/core/src/interaction/` 的类型 + 纯函数子集：
//! - [`modes`] / [`events`] / [`intents`] / [`export_artifact`] /
//!   [`edit_controller`]（49 号 chapter-replace；89 号 entity-rename 与
//!   chapter-local-edit）
//!
//! ## 待移植（需 state + llm）
//! runtime（1153 行交互运行时）/ edit-controller 其余 kind（truth-file-edit、
//! focus-edit、chapter-rewrite）/ project-tools / session-transcript 等。

pub mod agent_loop;
pub mod book_edit_tools;
pub mod chat_prompts;
pub mod book_reference_tool;
pub mod book_session_store;
pub mod import_chapters_tool;
pub mod material_tools;
pub mod edit_controller;
pub mod forecast_tools;
pub mod events;
pub mod export_artifact;
pub mod film_authoring_tools;
pub mod intents;
pub mod modes;
pub mod play_tools;
pub mod project_tools;
pub mod propose_action_tool;
pub mod research_tool;
pub mod sub_agent_tool;
pub mod session;
pub mod session_restore;
pub mod skill_tool;
pub mod session_transcript;
