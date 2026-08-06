//! Tauri 命令模块

pub mod config;

// 重新导出类型
pub use config::AppState;

// 重新导出命令（保持在 config 模块命名空间下）
