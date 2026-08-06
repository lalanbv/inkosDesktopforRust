//! 配置三层合并逻辑（0GC 优化）

use crate::config::types::*;

/// 配置管理器（四层架构：System < User < Workspace < Project）
#[derive(Debug, Clone)]
pub struct ConfigManager {
    system: AppConfig,
    user: Option<AppConfig>,
    workspace: Option<AppConfig>,
    project: Option<AppConfig>,
    merged: AppConfig,
    dirty: bool, // 标记是否需要重新合并
}

impl ConfigManager {
    /// 创建配置管理器（使用系统默认值）
    pub fn new() -> Self {
        let system = AppConfig::default();
        Self {
            merged: system.clone(),
            system,
            user: None,
            workspace: None,
            project: None,
            dirty: false,
        }
    }

    /// 设置用户全局配置
    pub fn set_user(&mut self, user_cfg: AppConfig) {
        self.user = Some(user_cfg);
        self.dirty = true;
    }

    /// 清除用户全局配置
    pub fn clear_user(&mut self) {
        self.user = None;
        self.dirty = true;
    }

    /// 设置工作区配置
    pub fn set_workspace(&mut self, workspace_cfg: AppConfig) {
        self.workspace = Some(workspace_cfg);
        self.dirty = true;
    }

    /// 设置项目配置
    pub fn set_project(&mut self, project_cfg: AppConfig) {
        self.project = Some(project_cfg);
        self.dirty = true;
    }

    /// 清除工作区配置
    pub fn clear_workspace(&mut self) {
        self.workspace = None;
        self.dirty = true;
    }

    /// 清除项目配置
    pub fn clear_project(&mut self) {
        self.project = None;
        self.dirty = true;
    }

    /// 获取合并后的配置（懒加载）
    pub fn merged(&mut self) -> &AppConfig {
        if self.dirty {
            self.recompute_merged();
            self.dirty = false;
        }
        &self.merged
    }

    /// 重新计算合并配置
    fn recompute_merged(&mut self) {
        let mut merged = self.system.clone();

        // 优先级：System < User < Workspace < Project（后者覆盖前者）
        if let Some(ref user) = self.user {
            Self::merge_into(&mut merged, user);
        }

        // 工作区覆盖 系统+用户
        if let Some(ref ws) = self.workspace {
            Self::merge_into(&mut merged, ws);
        }

        // 项目覆盖 工作区+用户+系统
        if let Some(ref proj) = self.project {
            Self::merge_into(&mut merged, proj);
        }

        self.merged = merged;
    }

    /// 字段级合并（source 覆盖 target）
    ///
    /// 覆盖层只覆盖「自己显式设置过」的字段。上层配置文件里省略的字段经
    /// `#[serde(default)]` 反序列化后等于默认值，因此「字段 != 默认值」正好等价于
    /// 「该字段在这一层被显式设置」——据此逐字段判断，避免上层用默认值把下层
    /// 已设置的值冲掉（例如项目层只设 updates.channel，不应把工作区的
    /// logging.level 打回 "info"）。
    ///
    /// 已知取舍：显式写入与默认值相同的值，等同于不写——此时合并结果一致，
    /// 唯一无法表达的是「上层把下层的非默认值显式重置回默认值」。若将来需要
    /// 该语义，需把覆盖层类型改为逐字段 `Option<T>` 的稀疏结构。
    fn merge_into(target: &mut AppConfig, source: &AppConfig) {
        let defaults = AppConfig::default();

        // Engine 配置
        if source.engine.version_policy != defaults.engine.version_policy {
            target.engine.version_policy = source.engine.version_policy.clone();
        }
        if source.engine.auto_download != defaults.engine.auto_download {
            target.engine.auto_download = source.engine.auto_download;
        }

        // Updates 配置
        if source.updates.check_interval_hours != defaults.updates.check_interval_hours {
            target.updates.check_interval_hours = source.updates.check_interval_hours;
        }
        if source.updates.channel != defaults.updates.channel {
            target.updates.channel = source.updates.channel.clone();
        }
        if source.updates.auto_apply != defaults.updates.auto_apply {
            target.updates.auto_apply = source.updates.auto_apply;
        }

        // Logging 配置
        if source.logging.level != defaults.logging.level {
            target.logging.level = source.logging.level.clone();
        }
        if source.logging.retention_days != defaults.logging.retention_days {
            target.logging.retention_days = source.logging.retention_days;
        }

        // Network 配置（proxy 是 Option，None 即未设置）
        if source.network.proxy.is_some() {
            target.network.proxy = source.network.proxy.clone();
        }
        if source.network.timeout_seconds != defaults.network.timeout_seconds {
            target.network.timeout_seconds = source.network.timeout_seconds;
        }
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    #[test]
    fn test_config_manager_default() {
        let mut mgr = ConfigManager::new();
        let merged = mgr.merged();

        // 应该使用系统默认值
        assert_eq!(merged.logging.level, "info");
        assert_eq!(merged.updates.channel, "stable");
    }

    #[test]
    fn test_workspace_overrides_system() {
        let mut mgr = ConfigManager::new();

        let mut workspace_cfg = AppConfig::default();
        workspace_cfg.logging.level = "debug".to_string();
        workspace_cfg.updates.channel = "beta".to_string();

        mgr.set_workspace(workspace_cfg);

        let merged = mgr.merged();
        assert_eq!(merged.logging.level, "debug");
        assert_eq!(merged.updates.channel, "beta");
        assert_eq!(merged.network.timeout_seconds, 30); // 未覆盖字段保持系统默认
    }

    #[test]
    fn test_user_overrides_system() {
        let mut mgr = ConfigManager::new();

        let mut user_cfg = AppConfig::default();
        user_cfg.logging.level = "debug".to_string();
        user_cfg.updates.channel = "beta".to_string();
        mgr.set_user(user_cfg);

        let merged = mgr.merged();
        assert_eq!(merged.logging.level, "debug");
        assert_eq!(merged.updates.channel, "beta");
        assert_eq!(merged.network.timeout_seconds, 30); // 未覆盖字段保持系统默认
    }

    #[test]
    fn test_workspace_and_project_override_user() {
        // 优先级 System < User < Workspace < Project
        let mut mgr = ConfigManager::new();

        let mut user_cfg = AppConfig::default();
        user_cfg.logging.level = "debug".to_string();
        user_cfg.updates.channel = "beta".to_string();
        user_cfg.network.proxy = Some("http://user-proxy:8080".to_string());
        mgr.set_user(user_cfg);

        // 工作区覆盖用户的 logging.level，保留用户的 channel/proxy
        let mut workspace_cfg = AppConfig::default();
        workspace_cfg.logging.level = "trace".to_string();
        mgr.set_workspace(workspace_cfg);

        // 项目覆盖用户的 proxy
        let mut project_cfg = AppConfig::default();
        project_cfg.network.proxy = Some("http://proj-proxy:9090".to_string());
        mgr.set_project(project_cfg);

        let merged = mgr.merged();
        assert_eq!(merged.logging.level, "trace");      // 工作区覆盖用户
        assert_eq!(merged.updates.channel, "beta");     // 用户保留（未被更高层覆盖）
        assert_eq!(
            merged.network.proxy,
            Some("http://proj-proxy:9090".to_string())
        ); // 项目覆盖用户
        assert_eq!(merged.network.timeout_seconds, 30); // 系统默认
    }

    #[test]
    fn test_clear_user_falls_back_to_system() {
        let mut mgr = ConfigManager::new();

        let mut user_cfg = AppConfig::default();
        user_cfg.logging.level = "debug".to_string();
        mgr.set_user(user_cfg);
        assert_eq!(mgr.merged().logging.level, "debug");

        mgr.clear_user();
        assert_eq!(mgr.merged().logging.level, "info"); // 回退系统默认
    }

    #[test]
    fn test_project_overrides_workspace() {
        let mut mgr = ConfigManager::new();

        let mut workspace_cfg = AppConfig::default();
        workspace_cfg.logging.level = "debug".to_string();
        workspace_cfg.updates.channel = "beta".to_string();
        mgr.set_workspace(workspace_cfg);

        let mut project_cfg = AppConfig::default();
        project_cfg.logging.level = "trace".to_string();
        project_cfg.network.proxy = Some("http://proxy:8080".to_string());
        mgr.set_project(project_cfg);

        let merged = mgr.merged();
        assert_eq!(merged.logging.level, "trace");          // 项目覆盖
        assert_eq!(merged.updates.channel, "beta");         // 工作区保留
        assert_eq!(merged.network.proxy, Some("http://proxy:8080".to_string())); // 项目覆盖
        assert_eq!(merged.network.timeout_seconds, 30);     // 系统默认
    }

    #[test]
    fn test_merge_is_lazy() {
        let mut mgr = ConfigManager::new();

        let mut workspace_cfg = AppConfig::default();
        workspace_cfg.logging.level = "debug".to_string();
        mgr.set_workspace(workspace_cfg.clone());

        // 第一次合并
        let merged1 = mgr.merged();
        assert_eq!(merged1.logging.level, "debug");

        // 再次设置相同配置
        mgr.set_workspace(workspace_cfg);

        // 第二次合并应该复用缓存
        let merged2 = mgr.merged();
        assert_eq!(merged2.logging.level, "debug");
    }
}
