//! 配置验证逻辑

use anyhow::{bail, Result};
use crate::config::types::{AppConfig, VersionPolicy};
use semver::{Version, VersionReq};

impl AppConfig {
    /// 验证配置合法性
    pub fn validate(&self) -> Result<()> {
        self.validate_logging()?;
        self.validate_updates()?;
        self.validate_network()?;
        self.validate_numerics()?;
        self.validate_version_policy()?;
        Ok(())
    }

    fn validate_logging(&self) -> Result<()> {
        let valid_levels = ["error", "warn", "info", "debug", "trace"];
        if !valid_levels.contains(&self.logging.level.as_str()) {
            bail!(
                "无效的日志级别: {}（允许值: {}）",
                self.logging.level,
                valid_levels.join(", ")
            );
        }
        Ok(())
    }

    fn validate_updates(&self) -> Result<()> {
        let valid_channels = ["stable", "beta", "dev"];
        if !valid_channels.contains(&self.updates.channel.as_str()) {
            bail!(
                "无效的更新通道: {}（允许值: {}）",
                self.updates.channel,
                valid_channels.join(", ")
            );
        }
        Ok(())
    }

    fn validate_network(&self) -> Result<()> {
        if let Some(ref proxy) = self.network.proxy {
            // 简单 URL 格式验证（http://host:port 或 https://host:port）
            if !proxy.starts_with("http://") && !proxy.starts_with("https://") {
                bail!("无效的代理 URL: {}（必须以 http:// 或 https:// 开头）", proxy);
            }
        }
        Ok(())
    }

    /// 数值字段下限校验——防止 0 导致下游异常行为
    /// （retention=0 立即清日志、check_interval=0 忙循环、timeout=0 瞬时超时）。
    fn validate_numerics(&self) -> Result<()> {
        if self.logging.retention_days < 1 {
            bail!(
                "日志保留天数必须 >= 1（当前 {}）",
                self.logging.retention_days
            );
        }
        if self.updates.check_interval_hours < 1 {
            bail!(
                "更新检查间隔必须 >= 1 小时（当前 {}）",
                self.updates.check_interval_hours
            );
        }
        if self.network.timeout_seconds < 1 {
            bail!(
                "网络超时必须 >= 1 秒（当前 {}）",
                self.network.timeout_seconds
            );
        }
        Ok(())
    }

    /// 版本策略校验——Fixed/Range 用 semver 解析，拒绝垃圾值进入更新引擎。
    fn validate_version_policy(&self) -> Result<()> {
        match &self.engine.version_policy {
            VersionPolicy::Fixed(v) => {
                Version::parse(v).map_err(|_| {
                    anyhow::anyhow!("无效的锁定版本: {}（需 semver，如 0.4.0）", v)
                })?;
            }
            VersionPolicy::Range(r) => {
                VersionReq::parse(r).map_err(|_| {
                    anyhow::anyhow!("无效的版本范围: {}（需 semver req，如 ^0.4.0）", r)
                })?;
            }
            VersionPolicy::Latest => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    
    use crate::config::types::*;

    #[test]
    fn test_validate_default_config() {
        let cfg = AppConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_log_level() {
        let mut cfg = AppConfig::default();
        cfg.logging.level = "invalid".to_string();

        let result = cfg.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("无效的日志级别"));
    }

    #[test]
    fn test_validate_invalid_channel() {
        let mut cfg = AppConfig::default();
        cfg.updates.channel = "invalid".to_string();

        let result = cfg.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("无效的更新通道"));
    }

    #[test]
    fn test_validate_invalid_proxy_url() {
        let mut cfg = AppConfig::default();
        cfg.network.proxy = Some("not-a-url".to_string());

        let result = cfg.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("无效的代理 URL"));
    }

    #[test]
    fn test_validate_valid_proxy_url() {
        let mut cfg = AppConfig::default();
        cfg.network.proxy = Some("http://proxy.example.com:8080".to_string());

        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_validate_zero_numerics_rejected() {
        let mut cfg = AppConfig::default();
        cfg.logging.retention_days = 0;
        assert!(cfg.validate()
            .unwrap_err()
            .to_string()
            .contains("日志保留天数"));

        let mut cfg = AppConfig::default();
        cfg.updates.check_interval_hours = 0;
        assert!(cfg.validate().is_err());

        let mut cfg = AppConfig::default();
        cfg.network.timeout_seconds = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_version_policy_rejected() {
        let mut cfg = AppConfig::default();
        cfg.engine.version_policy = VersionPolicy::Fixed("not-a-version".to_string());
        assert!(cfg.validate()
            .unwrap_err()
            .to_string()
            .contains("无效的锁定版本"));

        let mut cfg = AppConfig::default();
        cfg.engine.version_policy = VersionPolicy::Range("garbage".to_string());
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_valid_version_policy_ok() {
        let mut cfg = AppConfig::default();
        cfg.engine.version_policy = VersionPolicy::Fixed("0.4.0".to_string());
        assert!(cfg.validate().is_ok());

        cfg.engine.version_policy = VersionPolicy::Range("^0.4.0".to_string());
        assert!(cfg.validate().is_ok());
    }
}
