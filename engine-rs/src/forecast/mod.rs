//! 预测域（多分支叙事投影，RFC #342）。
//!
//! 移植自 `packages/core/src/forecast/`：
//! - schema.rs：类型 + 模型输出解析 + zod 约束等价校验
//! - store.rs：工件存储（`story/runtime/narrative-forecasts/` 安全边界）
//! - context_builder.rs：正史只读上下文 + 内容指纹
//! - prompts.rs / agent.rs / render.rs / runner.rs：三操作全链（90 号）

pub mod agent;
pub mod context_builder;
pub mod prompts;
pub mod render;
pub mod runner;
pub mod schema;
pub mod store;

pub use schema::{
    parse_forecast_model_output, validate_narrative_forecast, ForecastBeat, ForecastBranch,
    ForecastCharacterDecision, ForecastIntentAlignment, ForecastModelBranch, ForecastModelOutput,
    ForecastProjectedChanges, ForecastRisk, ForecastStatus, NarrativeForecast,
};
