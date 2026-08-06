//! 插件系统核心类型

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// 插件错误
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("插件清单无效: {0}")]
    InvalidManifest(String),

    #[error("插件未找到: {0}")]
    NotFound(String),

    #[error("插件安装失败: {0}")]
    InstallFailed(String),

    #[error("插件版本不兼容: {0}")]
    IncompatibleVersion(String),

    #[error("权限被拒绝: {0}")]
    PermissionDenied(String),

    #[error("插件加载失败: {0}")]
    LoadFailed(String),

    #[error("插件执行失败: {0}")]
    ExecutionFailed(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("Wasm 错误: {0}")]
    Wasm(String),
}

/// 插件元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// 插件 ID（kebab-case，全局唯一）
    pub id: String,

    /// 插件名称
    pub name: String,

    /// 插件版本（SemVer）
    pub version: String,

    /// 插件描述
    pub description: String,

    /// 插件作者
    pub author: String,

    /// 插件主页
    pub homepage: Option<String>,

    /// 许可证
    pub license: String,

    /// ABI 版本（兼容性检查）
    pub abi_version: String,

    /// 插件能力声明
    pub capabilities: Vec<Capability>,

    /// 插件入口点（wasm 文件相对路径）
    pub entrypoint: String,

    /// 依赖的其他插件
    pub dependencies: HashMap<String, String>,

    /// 是否启用
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// 插件能力（权限声明）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// 读取项目文件
    ReadProject,

    /// 修改项目文件
    WriteProject,

    /// 访问网络
    Network,

    /// 访问文件系统
    Filesystem { path: String },

    /// 执行系统命令
    SystemCommand,

    /// 访问环境变量
    Environment,

    /// 数据库访问
    Database,

    /// UI 扩展
    Ui,
}

/// 插件状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    /// 未加载
    Unloaded,

    /// 加载中
    Loading,

    /// 已加载
    Loaded,

    /// 运行中
    Running,

    /// 已暂停
    Paused,

    /// 错误
    Error,
}

/// 插件实例
#[derive(Debug)]
pub struct Plugin {
    /// 元数据
    pub metadata: PluginMetadata,

    /// 插件路径
    pub path: PathBuf,

    /// 当前状态
    pub state: PluginState,

    /// 错误消息（如果有）
    pub error_message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_serialization() {
        let cap = Capability::ReadProject;
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(json, "\"read_project\"");

        let cap = Capability::Filesystem {
            path: "/tmp".to_string(),
        };
        let json = serde_json::to_string(&cap).unwrap();
        assert!(json.contains("filesystem"));
        assert!(json.contains("/tmp"));
    }

    #[test]
    fn test_plugin_state_serialization() {
        let state = PluginState::Loaded;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"loaded\"");
    }

    #[test]
    fn test_plugin_metadata() {
        let meta = PluginMetadata {
            id: "test-plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: "A test plugin".to_string(),
            author: "Test Author".to_string(),
            homepage: Some("https://example.com".to_string()),
            license: "MIT".to_string(),
            abi_version: "1".to_string(),
            capabilities: vec![Capability::ReadProject],
            entrypoint: "plugin.wasm".to_string(),
            dependencies: HashMap::new(),
            enabled: true,
        };

        assert_eq!(meta.id, "test-plugin");
        assert_eq!(meta.capabilities.len(), 1);
    }
}
