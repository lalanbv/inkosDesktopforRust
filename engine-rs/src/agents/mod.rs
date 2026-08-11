//! agents 域（Phase P3）。
//!
//! 移植自 `packages/core/src/agents/`。自下而上：先纯逻辑解析叶子（无 LLM 调用），
//! 再移植依赖 streaming_client / state / prompts 的 agent 编排。
//!
//! ## 已移植
//! - [`detection_insights`]：检测历史聚合统计
//! - [`settler_parser`]：结算输出 `=== TAG ===` 段提取

pub mod ai_tells;
pub mod continuity;
pub mod detector;
pub mod detection_insights;
pub mod sensitive_words;
pub mod settler_delta_parser;
pub mod settler_parser;
pub mod style_analyzer;
pub mod writer_parser;
