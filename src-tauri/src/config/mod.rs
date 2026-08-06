//! 配置管理（三层架构 + 全局常量）

pub mod constants;
pub mod types;
pub mod validation;

// 重新导出常量（保持向后兼容）
pub use constants::*;

// 重新导出类型
pub use types::{
    AppConfig, ConfigLayer, EngineConfig, LoggingConfig, NetworkConfig,
    UpdatesConfig, VersionPolicy,
};
