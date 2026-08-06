//! 配置文件路径管理

use std::path::PathBuf;

/// 配置路径解析器
#[derive(Clone)]
pub struct ConfigPaths {
    app_data: PathBuf,
}

impl ConfigPaths {
    /// 创建配置路径解析器
    pub fn new(app_data: PathBuf) -> Self {
        Self { app_data }
    }

    /// 系统配置文件路径（硬编码默认值，不持久化）
    /// 注意：系统配置使用 AppConfig::default()，不从文件加载
    pub fn system_config(&self) -> PathBuf {
        self.app_data.join("config").join("default.toml")
    }

    /// 工作区配置文件路径
    pub fn workspace_config(&self, workspace_id: &str) -> PathBuf {
        self.app_data
            .join("config")
            .join(format!("workspace-{}", workspace_id))
            .join("config.toml")
    }

    /// 项目配置文件路径
    pub fn project_config(project_root: &std::path::Path) -> PathBuf {
        project_root.join(".inkos").join("config.toml")
    }

    /// 确保配置目录存在
    pub fn ensure_config_dirs(&self) -> anyhow::Result<()> {
        let config_dir = self.app_data.join("config");
        if !config_dir.exists() {
            std::fs::create_dir_all(&config_dir)?;
        }
        Ok(())
    }

    /// 确保工作区配置目录存在
    pub fn ensure_workspace_config_dir(&self, workspace_id: &str) -> anyhow::Result<()> {
        let ws_config_dir = self.app_data
            .join("config")
            .join(format!("workspace-{}", workspace_id));
        if !ws_config_dir.exists() {
            std::fs::create_dir_all(&ws_config_dir)?;
        }
        Ok(())
    }

    /// 确保项目配置目录存在
    pub fn ensure_project_config_dir(project_root: &std::path::Path) -> anyhow::Result<()> {
        let proj_config_dir = project_root.join(".inkos");
        if !proj_config_dir.exists() {
            std::fs::create_dir_all(&proj_config_dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_paths() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());

        let system = paths.system_config();
        assert!(system.ends_with("config/default.toml"));

        let workspace = paths.workspace_config("ws-123");
        assert!(workspace.ends_with("config/workspace-ws-123/config.toml"));

        let project = ConfigPaths::project_config(&temp.path().join("my-project"));
        assert!(project.ends_with("my-project/.inkos/config.toml"));
    }

    #[test]
    fn test_ensure_config_dirs() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());

        paths.ensure_config_dirs().unwrap();
        assert!(temp.path().join("config").exists());
    }

    #[test]
    fn test_ensure_workspace_config_dir() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());

        paths.ensure_workspace_config_dir("ws-456").unwrap();
        assert!(temp.path().join("config/workspace-ws-456").exists());
    }

    #[test]
    fn test_ensure_project_config_dir() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("my-project");
        std::fs::create_dir(&project_root).unwrap();

        ConfigPaths::ensure_project_config_dir(&project_root).unwrap();
        assert!(project_root.join(".inkos").exists());
    }
}
