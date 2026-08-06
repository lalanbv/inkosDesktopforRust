//! 配置加载器（三层配置加载 + 自动持久化）

use crate::config::{AppConfig, ConfigManager, ConfigPaths};
use anyhow::{Context, Result};
use std::path::Path;

/// 配置加载器
pub struct ConfigLoader {
    paths: ConfigPaths,
}

impl ConfigLoader {
    /// 创建配置加载器
    pub fn new(paths: ConfigPaths) -> Self {
        Self { paths }
    }

    /// 加载系统配置（硬编码默认值）
    pub fn load_system_config(&self) -> Result<AppConfig> {
        Ok(AppConfig::default())
    }

    /// 加载工作区配置（缺失 → 默认值）
    pub fn load_workspace_config(&self, workspace_id: &str) -> Result<AppConfig> {
        let path = self.paths.workspace_config(workspace_id);
        AppConfig::read(&path).with_context(|| {
            format!("加载工作区配置失败: workspace_id={}", workspace_id)
        })
    }

    /// 加载项目配置（缺失 → 默认值）
    pub fn load_project_config(&self, project_root: &Path) -> Result<AppConfig> {
        let path = ConfigPaths::project_config(project_root);
        AppConfig::read(&path).with_context(|| {
            format!("加载项目配置失败: project_root={}", project_root.display())
        })
    }

    /// 保存工作区配置
    pub fn save_workspace_config(
        &self,
        workspace_id: &str,
        config: &AppConfig,
    ) -> Result<()> {
        self.paths.ensure_workspace_config_dir(workspace_id)?;
        let path = self.paths.workspace_config(workspace_id);
        config.write(&path).with_context(|| {
            format!("保存工作区配置失败: workspace_id={}", workspace_id)
        })
    }

    /// 保存项目配置
    pub fn save_project_config(&self, project_root: &Path, config: &AppConfig) -> Result<()> {
        ConfigPaths::ensure_project_config_dir(project_root)?;
        let path = ConfigPaths::project_config(project_root);
        config.write(&path).with_context(|| {
            format!("保存项目配置失败: project_root={}", project_root.display())
        })
    }

    /// 初始化配置管理器（仅加载系统默认配置）
    pub fn init_manager(&self) -> Result<ConfigManager> {
        self.paths.ensure_config_dirs()?;
        Ok(ConfigManager::new())
    }

    /// 应用工作区配置到管理器
    pub fn apply_workspace_config(
        &self,
        manager: &mut ConfigManager,
        workspace_id: &str,
    ) -> Result<()> {
        let ws_config = self.load_workspace_config(workspace_id)?;
        manager.set_workspace(ws_config);
        Ok(())
    }

    /// 应用项目配置到管理器
    pub fn apply_project_config(
        &self,
        manager: &mut ConfigManager,
        project_root: &Path,
    ) -> Result<()> {
        let proj_config = self.load_project_config(project_root)?;
        manager.set_project(proj_config);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_system_config() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());
        let loader = ConfigLoader::new(paths);

        let cfg = loader.load_system_config().unwrap();
        assert_eq!(cfg, AppConfig::default());
    }

    #[test]
    fn test_save_and_load_workspace_config() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());
        let loader = ConfigLoader::new(paths);

        let mut cfg = AppConfig::default();
        cfg.logging.level = "debug".to_string();

        loader.save_workspace_config("ws-123", &cfg).unwrap();
        let loaded = loader.load_workspace_config("ws-123").unwrap();

        assert_eq!(loaded.logging.level, "debug");
    }

    #[test]
    fn test_save_and_load_project_config() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("my-project");
        std::fs::create_dir(&project_root).unwrap();

        let paths = ConfigPaths::new(temp.path().to_path_buf());
        let loader = ConfigLoader::new(paths);

        let mut cfg = AppConfig::default();
        cfg.updates.channel = "beta".to_string();

        loader.save_project_config(&project_root, &cfg).unwrap();
        let loaded = loader.load_project_config(&project_root).unwrap();

        assert_eq!(loaded.updates.channel, "beta");
    }

    #[test]
    fn test_init_manager() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());
        let loader = ConfigLoader::new(paths);

        let mut mgr = loader.init_manager().unwrap();
        let merged = mgr.merged();

        assert_eq!(merged.logging.level, "info");
    }

    #[test]
    fn test_apply_workspace_and_project_config() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("my-project");
        std::fs::create_dir(&project_root).unwrap();

        let paths = ConfigPaths::new(temp.path().to_path_buf());
        let loader = ConfigLoader::new(paths);

        // 保存工作区配置
        let mut ws_cfg = AppConfig::default();
        ws_cfg.logging.level = "debug".to_string();
        loader.save_workspace_config("ws-123", &ws_cfg).unwrap();

        // 保存项目配置
        let mut proj_cfg = AppConfig::default();
        proj_cfg.updates.channel = "beta".to_string();
        loader.save_project_config(&project_root, &proj_cfg).unwrap();

        // 初始化管理器并应用配置
        let mut mgr = loader.init_manager().unwrap();
        loader.apply_workspace_config(&mut mgr, "ws-123").unwrap();
        loader.apply_project_config(&mut mgr, &project_root).unwrap();

        let merged = mgr.merged();
        assert_eq!(merged.logging.level, "debug");       // 工作区覆盖
        assert_eq!(merged.updates.channel, "beta");      // 项目覆盖
    }
}
