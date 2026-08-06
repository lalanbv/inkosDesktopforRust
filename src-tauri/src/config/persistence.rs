//! 配置持久化（TOML 读写）

use std::path::Path;
use anyhow::Context;
use crate::config::types::AppConfig;

impl AppConfig {
    /// 读取配置文件（缺失 → 默认值，损坏 → Err）
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let cfg: Self = toml::from_str(&content)
                    .with_context(|| format!("解析配置文件失败: {}", path.display()))?;
                cfg.validate()
                    .with_context(|| format!("配置验证失败: {}", path.display()))?;
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // 文件不存在 → 返回默认配置
                Ok(Self::default())
            }
            Err(e) => {
                Err(e).with_context(|| format!("读取配置文件失败: {}", path.display()))
            }
        }
    }

    /// 写入配置文件（原子写入 + 0600 权限）
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        // 写入前验证
        self.validate().context("配置验证失败")?;

        let toml = toml::to_string_pretty(self)
            .context("序列化配置失败")?;

        crate::util::atomic_write_0600(path, toml.as_bytes())
            .with_context(|| format!("写入配置文件失败: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::*;
    use tempfile::TempDir;

    #[test]
    fn test_read_nonexistent_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.toml");

        let cfg = AppConfig::read(&path).unwrap();
        assert_eq!(cfg, AppConfig::default());
    }

    #[test]
    fn test_write_and_read() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.toml");

        let mut cfg = AppConfig::default();
        cfg.logging.level = "debug".to_string();
        cfg.updates.channel = "beta".to_string();
        cfg.network.proxy = Some("http://proxy:8080".to_string());

        // 写入
        cfg.write(&path).unwrap();

        // 读取
        let loaded = AppConfig::read(&path).unwrap();
        assert_eq!(loaded.logging.level, "debug");
        assert_eq!(loaded.updates.channel, "beta");
        assert_eq!(loaded.network.proxy, Some("http://proxy:8080".to_string()));
    }

    #[test]
    fn test_read_corrupted_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, b"invalid toml [[[").unwrap();

        let result = AppConfig::read(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("解析"));
    }
}
