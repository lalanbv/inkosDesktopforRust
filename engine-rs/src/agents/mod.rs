//! agents 域（Phase P3）。
//!
//! 移植自 `packages/core/src/agents/`。自下而上：先纯逻辑解析叶子（无 LLM 调用），
//! 再移植依赖 streaming_client / state / prompts 的 agent 编排。
//!
//! ## 已移植
//! - [`detection_insights`]：检测历史聚合统计
//! - [`settler_parser`]：结算输出 `=== TAG ===` 段提取
//! - [`rules_reader`]：规则读取链（genre 画像 / book_rules / book.json language），
//!   ContinuityAuditor 等编排 agent 的数据入口

pub mod ai_tells;
pub mod continuity;
pub mod detection_insights;
pub mod detector;
pub mod rules_reader;
pub mod sensitive_words;
pub mod settler_delta_parser;
pub mod settler_parser;
pub mod style_analyzer;
pub mod writer_parser;
