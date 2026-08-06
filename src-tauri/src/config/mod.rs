//! 配置管理（三层架构 + 全局常量 + 热重载）

pub mod constants;
pub mod loader;
pub mod merge;
pub mod paths;
pub mod persistence;
pub mod reload;
pub mod types;
pub mod validation;
pub mod watcher;

// 重新导出常量（保持向后兼容）
pub use constants::*;

// 重新导出类型
pub use loader::ConfigLoader;
pub use merge::ConfigManager;
pub use paths::ConfigPaths;
pub use reload::ConfigReloader;
pub use types::{
    AppConfig, ConfigLayer, EngineConfig, LoggingConfig, NetworkConfig,
    UpdatesConfig, VersionPolicy,
};
pub use validation::validate_config;
pub use watcher::{ConfigChangeEvent, ConfigWatcher};
