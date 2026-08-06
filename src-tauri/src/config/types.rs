//! 配置类型定义（三层架构：系统/工作区/项目）

use serde::{Deserialize, Serialize};

/// 应用配置（TOML schema）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub engine: EngineConfig,

    #[serde(default)]
    pub updates: UpdatesConfig,

    #[serde(default)]
    pub logging: LoggingConfig,

    #[serde(default)]
    pub network: NetworkConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            engine: EngineConfig::default(),
            updates: UpdatesConfig::default(),
            logging: LoggingConfig::default(),
            network: NetworkConfig::default(),
        }
    }
}

/// Engine 配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineConfig {
    #[serde(default)]
    pub version_policy: VersionPolicy,

    #[serde(default = "default_auto_download")]
    pub auto_download: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            version_policy: VersionPolicy::Latest,
            auto_download: true,
        }
    }
}

fn default_auto_download() -> bool {
    true
}

/// 更新配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdatesConfig {
    #[serde(default = "default_check_interval")]
    pub check_interval_hours: u32,

    #[serde(default = "default_channel")]
    pub channel: String,

    #[serde(default = "default_auto_apply")]
    pub auto_apply: bool,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            check_interval_hours: 24,
            channel: "stable".to_string(),
            auto_apply: false,
        }
    }
}

fn default_check_interval() -> u32 {
    24
}

fn default_channel() -> String {
    "stable".to_string()
}

fn default_auto_apply() -> bool {
    false
}

/// 日志配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,

    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            retention_days: 7,
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_retention_days() -> u32 {
    7
}

/// 网络配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,

    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            proxy: None,
            timeout_seconds: 30,
        }
    }
}

fn default_timeout() -> u32 {
    30
}

/// 版本策略
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionPolicy {
    Fixed(String),      // 锁定版本（企业部署）
    Latest,             // 总是最新（默认）
    Range(String),      // 语义化版本范围（^0.4.0）
}

impl Default for VersionPolicy {
    fn default() -> Self {
        Self::Latest
    }
}

/// 配置层级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigLayer {
    System,     // 系统默认（app_data/config/default.toml）
    Workspace,  // 工作区级（app_data/config/workspace-{id}/config.toml）
    Project,    // 项目级（project/.inkos/config.toml）
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_default() {
        let cfg = AppConfig::default();
        assert!(matches!(cfg.engine.version_policy, VersionPolicy::Latest));
        assert!(cfg.engine.auto_download);
        assert_eq!(cfg.updates.check_interval_hours, 24);
        assert_eq!(cfg.updates.channel, "stable");
        assert_eq!(cfg.logging.level, "info");
        assert_eq!(cfg.network.timeout_seconds, 30);
    }

    #[test]
    fn test_app_config_toml_roundtrip() {
        let cfg = AppConfig::default();
        let toml = toml::to_string(&cfg).unwrap();
        let parsed: AppConfig = toml::from_str(&toml).unwrap();
        assert_eq!(parsed.engine.auto_download, cfg.engine.auto_download);
        assert_eq!(parsed.updates.channel, cfg.updates.channel);
    }

    #[test]
    fn test_version_policy_serialization() {
        // 通过 EngineConfig 测试 VersionPolicy（枚举需要外层结构）
        let configs = vec![
            EngineConfig {
                version_policy: VersionPolicy::Latest,
                auto_download: true,
            },
            EngineConfig {
                version_policy: VersionPolicy::Fixed("0.4.0".to_string()),
                auto_download: false,
            },
            EngineConfig {
                version_policy: VersionPolicy::Range("^0.4.0".to_string()),
                auto_download: true,
            },
        ];

        for cfg in configs {
            let toml = toml::to_string(&cfg).unwrap();
            let parsed: EngineConfig = toml::from_str(&toml).unwrap();
            assert_eq!(parsed.version_policy, cfg.version_policy);
            assert_eq!(parsed.auto_download, cfg.auto_download);
        }
    }
}
