//! 统一错误类型和用户友好错误消息
//!
//! 所有 Tauri 命令返回 Result<T, AppError>，前端自动获得结构化错误。

use serde::{Deserialize, Serialize};
use std::fmt;

/// 应用错误类型（Tauri 命令返回值）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    /// 错误类型（用于前端分类处理）
    pub kind: ErrorKind,

    /// 用户友好的错误消息（中文）
    pub message: String,

    /// 技术细节（可选，调试用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,

    /// 建议的用户操作（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// 错误分类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// 网络错误（无法连接、超时）
    Network,

    /// 文件系统错误（权限、不存在）
    Filesystem,

    /// 配置错误（无效配置、缺少必需字段）
    Config,

    /// 项目错误（无效项目、损坏）
    Project,

    /// Engine 错误（下载失败、启动失败）
    Engine,

    /// Keychain 错误（权限被拒、读写失败）
    Keychain,

    /// 更新错误（下载失败、校验失败）
    Update,

    /// 内部错误（不应发生的错误）
    Internal,
}

impl AppError {
    /// 创建新错误
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            details: None,
            suggestion: None,
        }
    }

    /// 添加技术细节
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// 添加用户建议
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// 网络错误
    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Network, message)
            .with_suggestion("请检查网络连接，或稍后重试")
    }

    /// 文件系统错误
    pub fn filesystem(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Filesystem, message)
    }

    /// 配置错误
    pub fn config(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Config, message)
            .with_suggestion("请检查配置文件格式是否正确")
    }

    /// 项目错误
    pub fn project(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Project, message)
    }

    /// Engine 错误
    pub fn engine(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Engine, message)
    }

    /// Keychain 错误
    pub fn keychain(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Keychain, message)
            .with_suggestion("请在系统设置中授予 Keychain 访问权限")
    }

    /// 更新错误
    pub fn update(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Update, message)
    }

    /// 内部错误
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
            .with_suggestion("这是一个内部错误，请报告给开发者")
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(details) = &self.details {
            write!(f, " ({})", details)?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

// 从标准错误类型转换
impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind as IoKind;

        match err.kind() {
            IoKind::NotFound => {
                Self::filesystem("文件或目录不存在")
                    .with_details(err.to_string())
            }
            IoKind::PermissionDenied => {
                Self::filesystem("权限被拒绝")
                    .with_details(err.to_string())
                    .with_suggestion("请检查文件权限，或以管理员身份运行")
            }
            IoKind::ConnectionRefused | IoKind::ConnectionReset | IoKind::TimedOut => {
                Self::network("网络连接失败")
                    .with_details(err.to_string())
            }
            _ => {
                Self::filesystem("文件操作失败")
                    .with_details(err.to_string())
            }
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::config("JSON 格式错误")
            .with_details(err.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        Self::internal("Tauri 内部错误")
            .with_details(err.to_string())
    }
}

impl From<tauri_plugin_updater::Error> for AppError {
    fn from(err: tauri_plugin_updater::Error) -> Self {
        Self::update("更新失败")
            .with_details(err.to_string())
            .with_suggestion("请稍后重试，或手动下载最新版本")
    }
}

// keyring 错误转换
impl From<keyring::Error> for AppError {
    fn from(err: keyring::Error) -> Self {
        use keyring::Error as KErr;

        match err {
            KErr::NoEntry => {
                Self::keychain("密钥不存在")
                    .with_details("Keychain 中未找到该密钥")
            }
            KErr::PlatformFailure(ref e) => {
                let msg = e.to_string();
                if msg.contains("denied") || msg.contains("access") {
                    Self::keychain("Keychain 访问被拒绝")
                        .with_details(err.to_string())
                        .with_suggestion("请在系统设置中授予应用 Keychain 访问权限")
                } else {
                    Self::keychain("Keychain 操作失败")
                        .with_details(err.to_string())
                }
            }
            _ => {
                Self::keychain("Keychain 操作失败")
                    .with_details(err.to_string())
            }
        }
    }
}

/// Tauri 命令结果类型
pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_serialization() {
        let err = AppError::network("无法连接到服务器")
            .with_details("Connection refused");

        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("network"));
        assert!(json.contains("无法连接到服务器"));
        assert!(json.contains("请检查网络连接"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file.txt");
        let app_err: AppError = io_err.into();

        assert_eq!(app_err.kind, ErrorKind::Filesystem);
        assert!(app_err.message.contains("不存在"));
    }

    #[test]
    fn test_json_error_conversion() {
        let json_err = serde_json::from_str::<serde_json::Value>("{invalid}").unwrap_err();
        let app_err: AppError = json_err.into();

        assert_eq!(app_err.kind, ErrorKind::Config);
        assert!(app_err.message.contains("JSON"));
    }
}
