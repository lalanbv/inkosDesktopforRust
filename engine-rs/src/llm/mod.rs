//! LLM 层（Phase 3 高层域基础）。
//!
//! 自 `packages/core/src/llm/*.ts` 移植。当前：
//! - [`think_tag_stripper`]：剥离响应起始处的完整 `<think>...</think>` 块。
//!
//! ## 待移植（按依赖序）
//! provider（1414 行流式/多格式/重试/工具调用，多会话工程）/ config-migration /
//! service-resolver / service-presets / secrets / cover-providers

pub mod think_tag_stripper;
