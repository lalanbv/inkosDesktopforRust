//! 预测域（多分支叙事投影）。
//!
//! 移植自 `packages/core/src/forecast/`。当前移植类型 + 模型输出解析（schema.ts）。
//! runner/context-builder/store/prompts/agent/render 待 state + llm。

pub mod schema;

pub use schema::{
    parse_forecast_model_output, ForecastBeat, ForecastBranch, ForecastCharacterDecision,
    ForecastIntentAlignment, ForecastModelBranch, ForecastModelOutput, ForecastProjectedChanges,
    ForecastRisk, ForecastStatus, NarrativeForecast,
};
