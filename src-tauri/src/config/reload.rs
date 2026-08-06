//! 配置热重载逻辑

use crate::config::{AppConfig, ConfigLoader, ConfigManager};
use anyhow::{Context, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// 配置重载器
pub struct ConfigReloader {
    loader: ConfigLoader,
    last_valid_config: Arc<Mutex<AppConfig>>,
}

impl ConfigReloader {
    /// 创建配置重载器
    pub fn new(loader: ConfigLoader, initial_config: AppConfig) -> Self {
        Self {
            loader,
            last_valid_config: Arc::new(Mutex::new(initial_config)),
        }
    }

    /// 重新加载工作区配置
    pub fn reload_workspace_config(
        &self,
        workspace_id: &str,
        manager: &mut ConfigManager,
    ) -> Result<AppConfig> {
        tracing::info!("重新加载工作区配置: {}", workspace_id);

        // 尝试加载新配置
        match self.loader.load_workspace_config(workspace_id) {
            Ok(new_config) => {
                // 验证配置
                if let Err(e) = new_config.validate() {
                    tracing::error!("工作区配置验证失败: {:#}", e);
                    return Err(e).context("工作区配置验证失败，保持旧配置");
                }

                // 应用新配置
                manager.set_workspace(new_config.clone());
                let merged = manager.merged().clone();

                // 保存为最后有效配置
                *self.last_valid_config.lock().expect("last_valid_config mutex 中毒") =
                    merged.clone();

                tracing::info!("工作区配置重新加载成功");
                Ok(merged)
            }
            Err(e) => {
                tracing::error!("加载工作区配置失败: {:#}", e);
                Err(e).context("加载工作区配置失败，保持旧配置")
            }
        }
    }

    /// 重新加载项目配置
    pub fn reload_project_config(
        &self,
        project_root: &Path,
        manager: &mut ConfigManager,
    ) -> Result<AppConfig> {
        tracing::info!("重新加载项目配置: {}", project_root.display());

        // 尝试加载新配置
        match self.loader.load_project_config(project_root) {
            Ok(new_config) => {
                // 验证配置
                if let Err(e) = new_config.validate() {
                    tracing::error!("项目配置验证失败: {:#}", e);
                    return Err(e).context("项目配置验证失败，保持旧配置");
                }

                // 应用新配置
                manager.set_project(new_config.clone());
                let merged = manager.merged().clone();

                // 保存为最后有效配置
                *self.last_valid_config.lock().expect("last_valid_config mutex 中毒") =
                    merged.clone();

                tracing::info!("项目配置重新加载成功");
                Ok(merged)
            }
            Err(e) => {
                tracing::error!("加载项目配置失败: {:#}", e);
                Err(e).context("加载项目配置失败，保持旧配置")
            }
        }
    }

    /// 获取最后有效配置（回退用）
    pub fn get_last_valid_config(&self) -> AppConfig {
        self.last_valid_config
            .lock()
            .expect("last_valid_config mutex 中毒")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ConfigLoader, ConfigManager, ConfigPaths};
    use tempfile::TempDir;

    #[test]
    fn test_reload_workspace_config_success() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());
        let loader = ConfigLoader::new(paths.clone());
        let mut manager = ConfigManager::new();

        // 保存工作区配置
        let workspace_config = AppConfig {
            logging: crate::config::LoggingConfig {
                level: "debug".to_string(),
                retention_days: 14,
            },
            ..Default::default()
        };
        loader
            .save_workspace_config("ws-test", &workspace_config)
            .unwrap();

        // 创建重载器
        let reloader = ConfigReloader::new(loader.clone(), AppConfig::default());

        // 重新加载
        let result = reloader.reload_workspace_config("ws-test", &mut manager);
        assert!(result.is_ok());

        let merged = result.unwrap();
        assert_eq!(merged.logging.level, "debug");
        assert_eq!(merged.logging.retention_days, 14);
    }

    #[test]
    fn test_reload_project_config_success() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("project");
        std::fs::create_dir(&project_root).unwrap();

        let paths = ConfigPaths::new(temp.path().to_path_buf());
        let loader = ConfigLoader::new(paths.clone());
        let mut manager = ConfigManager::new();

        // 保存项目配置
        let project_config = AppConfig {
            engine: crate::config::EngineConfig {
                auto_download: false,
                ..Default::default()
            },
            ..Default::default()
        };
        loader
            .save_project_config(&project_root, &project_config)
            .unwrap();

        // 创建重载器
        let reloader = ConfigReloader::new(loader.clone(), AppConfig::default());

        // 重新加载
        let result = reloader.reload_project_config(&project_root, &mut manager);
        assert!(result.is_ok());

        let merged = result.unwrap();
        assert!(!merged.engine.auto_download);
    }

    #[test]
    fn test_reload_invalid_config_keeps_old() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());
        let loader = ConfigLoader::new(paths.clone());
        let mut manager = ConfigManager::new();

        // 直接写入非法 TOML——不能走 save_workspace_config，那条路径自带校验、
        // 会拒绝写入非法值。这里要模拟的正是「磁盘上已存在非法配置」
        // （手工编辑 / 旧版本残留），验证 reload 能识别并拒绝它。
        let config_path = paths.workspace_config("ws-test");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            "[logging]\nlevel = \"invalid_level\"\nretention_days = 14\n",
        )
        .unwrap();

        // 创建重载器
        let reloader = ConfigReloader::new(loader.clone(), AppConfig::default());

        // 重新加载应该失败
        let result = reloader.reload_workspace_config("ws-test", &mut manager);
        assert!(result.is_err());

        // 应该保持默认配置
        let last_valid = reloader.get_last_valid_config();
        assert_eq!(last_valid.logging.level, "info");
    }
}
