//! 统一错误类型和用户友好错误消息
//!
//! 所有 Tauri 命令返回 Result<T, AppError>，前端自动获得结构化错误。
//!
//! 1009 号：直显文案双语化——message/suggestion 是纯展示面（前端按 kind
//! 分类渲染，不解析内容判定），按桌面壳 locale 直接双语输出（见
//! `crate::locale` 模块头的政策边界定性）；details 为技术细节折叠面，
//! 其中固定中文前缀同步双语化，动态片段（io 错误原文等）保持原样。

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::locale::pick;

/// 应用错误类型（Tauri 命令返回值）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    /// 错误类型（用于前端分类处理）
    pub kind: ErrorKind,

    /// 用户友好的错误消息（按桌面壳 locale 双语输出）
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
        Self::new(ErrorKind::Network, message).with_suggestion(pick(
            "请检查网络连接，或稍后重试",
            "Check your network connection and try again later",
        ))
    }

    /// 文件系统错误
    pub fn filesystem(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Filesystem, message)
    }

    /// 配置错误
    pub fn config(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Config, message).with_suggestion(pick(
            "请检查配置文件格式是否正确",
            "Check that the configuration file is well-formed",
        ))
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
        Self::new(ErrorKind::Keychain, message).with_suggestion(pick(
            "请在系统设置中授予 Keychain 访问权限",
            "Grant Keychain access in System Settings",
        ))
    }

    /// 更新错误
    pub fn update(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Update, message)
    }

    /// 内部错误
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message).with_suggestion(pick(
            "这是一个内部错误，请报告给开发者",
            "This is an internal error; please report it to the developers",
        ))
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
            IoKind::NotFound => Self::filesystem(pick("文件或目录不存在", "File or directory not found"))
                .with_details(err.to_string()),
            IoKind::PermissionDenied => Self::filesystem(pick("权限被拒绝", "Permission denied"))
                .with_details(err.to_string())
                .with_suggestion(pick(
                    "请检查文件权限，或以管理员身份运行",
                    "Check the file permissions, or run as administrator",
                )),
            IoKind::ConnectionRefused | IoKind::ConnectionReset | IoKind::TimedOut => {
                Self::network(pick("网络连接失败", "Network connection failed"))
                    .with_details(err.to_string())
            }
            _ => Self::filesystem(pick("文件操作失败", "File operation failed"))
                .with_details(err.to_string()),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::config(pick("JSON 格式错误", "Malformed JSON"))
            .with_details(err.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        Self::internal(pick("Tauri 内部错误", "Internal Tauri error"))
            .with_details(err.to_string())
    }
}

impl From<tauri_plugin_updater::Error> for AppError {
    fn from(err: tauri_plugin_updater::Error) -> Self {
        Self::update(pick("更新失败", "Update failed"))
            .with_details(err.to_string())
            .with_suggestion(pick(
                "请稍后重试，或手动下载最新版本",
                "Try again later, or download the latest version manually",
            ))
    }
}

// keyring 错误转换
impl From<keyring::Error> for AppError {
    fn from(err: keyring::Error) -> Self {
        use keyring::Error as KErr;

        match err {
            KErr::NoEntry => Self::keychain(pick("密钥不存在", "Secret not found"))
                .with_details(pick(
                    "Keychain 中未找到该密钥",
                    "The secret was not found in the Keychain",
                )),
            KErr::PlatformFailure(ref e) => {
                let msg = e.to_string();
                if msg.contains("denied") || msg.contains("access") {
                    Self::keychain(pick("Keychain 访问被拒绝", "Keychain access denied"))
                        .with_details(err.to_string())
                        .with_suggestion(pick(
                            "请在系统设置中授予应用 Keychain 访问权限",
                            "Grant the app Keychain access in System Settings",
                        ))
                } else {
                    Self::keychain(pick("Keychain 操作失败", "Keychain operation failed"))
                        .with_details(err.to_string())
                }
            }
            _ => Self::keychain(pick("Keychain 操作失败", "Keychain operation failed"))
                .with_details(err.to_string()),
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
        // suggestion 文案随 locale：持锁防并行线程改全局态（locale.rs 1009 号）
        let _g = crate::locale::tests::test_locale_lock();
        crate::locale::set_locale(crate::locale::Locale::Zh);
        let err = AppError::network("无法连接到服务器")
            .with_details("Connection refused");

        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("network"));
        assert!(json.contains("无法连接到服务器"));
        assert!(json.contains("请检查网络连接"));
    }

    #[test]
    fn test_io_error_conversion() {
        // message 文案随 locale：持锁（locale.rs 1009 号）
        let _g = crate::locale::tests::test_locale_lock();
        crate::locale::set_locale(crate::locale::Locale::Zh);
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

    // 1009 号：直显文案随桌面壳 locale 双语输出（message/suggestion/details
    // 均为展示面；前端按 kind 分类渲染不解析内容）。
    #[test]
    fn test_direct_display_text_follows_locale() {
        use crate::locale::set_locale;
        let _g = crate::locale::tests::test_locale_lock();

        // zh 默认形态（原行为零漂移）
        set_locale(crate::locale::Locale::Zh);
        let err = AppError::network("x");
        assert_eq!(
            err.suggestion.as_deref(),
            Some("请检查网络连接，或稍后重试")
        );

        // en 形态
        set_locale(crate::locale::Locale::En);
        let err = AppError::network("x");
        assert_eq!(
            err.suggestion.as_deref(),
            Some("Check your network connection and try again later")
        );
        let io_err: AppError = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied")
            .into();
        assert_eq!(io_err.message, "Permission denied");

        // 复位 zh（全局态，避免影响其它用例）
        set_locale(crate::locale::Locale::Zh);
        let io_err: AppError =
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied").into();
        assert_eq!(io_err.message, "权限被拒绝");
    }
}
