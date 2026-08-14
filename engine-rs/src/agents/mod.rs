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
//! - [`fanfic_dimensions`]：同人维度配置（模式 → 维度 34-37 激活/严重度/注记）
//! - [`continuity`]：ContinuityAuditor 全量——类型层 + 37 维度注记/激活集 +
//!   四策略结果解析 + [`continuity::audit_chapter`] 编排（[`continuity::AuditorChat`]
//!   trait 注入 LLM 调用，生产实现待 BaseAgent/LLMRouter 移植）
//! - [`fanfic_prompt_sections`] / [`en_prompt_sections`]：同人/英文 prompt 段（writer
//!   system prompt 的 section 依赖）
//! - [`writer_prompts`]：writer system prompt 总装（zh 21 段 / en 19 段）+ 黄金三章纪律

pub mod ai_tells;
pub mod continuity;
pub mod detection_insights;
pub mod detector;
pub mod en_prompt_sections;
pub mod fanfic_dimensions;
pub mod fanfic_prompt_sections;
pub mod rules_reader;
pub mod sensitive_words;
pub mod settler_delta_parser;
pub mod settler_parser;
pub mod style_analyzer;
pub mod writer_parser;
pub mod writer_prompts;
