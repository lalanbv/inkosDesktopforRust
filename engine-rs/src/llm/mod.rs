//! LLM 层（Phase 3 高层域基础）。
//!
//! 自 `packages/core/src/llm/*.ts` 移植。当前：
//! - [`think_tag_stripper`]：剥离响应起始处的完整 `<think>...</think>` 块。
//! - [`provider`]：纯函数基础（token 估算 / 瞬时错误判定 / 头部清洗 / 消息类型）。
//!
//! ## 待移植（按依赖序）
//! createLLMClient（流式 chat/responses 客户端，需 reqwest + 录制回放测试）/
//! createStreamMonitor（定时器）/ estimatePiContextTokens（依赖 pi-ai PiContext）/
//! withTransientLLMRetry / config-migration / service-resolver / secrets

pub mod lookup;
pub mod provider;
pub mod providers;
pub mod registry;
pub mod secrets;
pub mod service_presets;
pub mod sse_parser;
pub mod streaming_client;
pub mod think_tag_stripper;
