//! 配置验证逻辑

use anyhow::{bail, Result};
use crate::config::types::AppConfig;

impl AppConfig {
    /// 验证配置合法性
    pub fn validate(&self) -> Result<()> {
        self.validate_logging()?;
        self.validate_updates()?;
        self.validate_network()?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
