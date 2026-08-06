//! 配置文件监听器（热重载支持）

use crate::config::{AppConfig, ConfigLoader, ConfigPaths};
use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 配置变更事件
#[derive(Debug, Clone)]
pub enum ConfigChangeEvent {
    /// 工作区配置变更
    WorkspaceChanged(String),
    /// 项目配置变更
    ProjectChanged(PathBuf),
}

/// 配置文件监听器
pub struct ConfigWatcher {
    watcher: RecommendedWatcher,
    event_rx: Receiver<Result<Event, notify::Error>>,
    watched_paths: Arc<Mutex<HashMap<PathBuf, ConfigChangeEvent>>>,
    debounce_map: Arc<Mutex<HashMap<PathBuf, Instant>>>,
    debounce_duration: Duration,
}

impl ConfigWatcher {
    /// 创建配置监听器
    pub fn new() -> Result<Self> {
        let (tx, rx) = channel();
        let watcher = RecommendedWatcher::new(
            tx,
            notify::Config::default().with_poll_interval(Duration::from_secs(2)),
        )
        .context("创建文件监听器失败")?;

        Ok(Self {
            watcher,
            event_rx: rx,
            watched_paths: Arc::new(Mutex::new(HashMap::new())),
            debounce_map: Arc::new(Mutex::new(HashMap::new())),
            debounce_duration: Duration::from_millis(500),
        })
    }

    /// 监听工作区配置文件
    pub fn watch_workspace_config(
        &mut self,
        paths: &ConfigPaths,
        workspace_id: &str,
    ) -> Result<()> {
        let config_path = paths.workspace_config(workspace_id);
        if let Some(parent) = config_path.parent() {
            self.watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .with_context(|| format!("监听工作区配置目录失败: {}", parent.display()))?;

            let mut watched = self
                .watched_paths
                .lock()
                .expect("watched_paths mutex 中毒");
            watched.insert(
                config_path.clone(),
                ConfigChangeEvent::WorkspaceChanged(workspace_id.to_string()),
            );

            tracing::info!("开始监听工作区配置: {}", config_path.display());
        }
        Ok(())
    }

    /// 监听项目配置文件
    pub fn watch_project_config(&mut self, project_root: &Path) -> Result<()> {
        let config_path = ConfigPaths::project_config(project_root);
        if let Some(parent) = config_path.parent() {
            self.watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .with_context(|| format!("监听项目配置目录失败: {}", parent.display()))?;

            let mut watched = self
                .watched_paths
                .lock()
                .expect("watched_paths mutex 中毒");
            watched.insert(
                config_path.clone(),
                ConfigChangeEvent::ProjectChanged(project_root.to_path_buf()),
            );

            tracing::info!("开始监听项目配置: {}", config_path.display());
        }
        Ok(())
    }

    /// 停止监听工作区配置
    pub fn unwatch_workspace_config(
        &mut self,
        paths: &ConfigPaths,
        workspace_id: &str,
    ) -> Result<()> {
        let config_path = paths.workspace_config(workspace_id);
        if let Some(parent) = config_path.parent() {
            self.watcher
                .unwatch(parent)
                .with_context(|| format!("停止监听工作区配置失败: {}", parent.display()))?;

            let mut watched = self
                .watched_paths
                .lock()
                .expect("watched_paths mutex 中毒");
            watched.remove(&config_path);

            tracing::info!("停止监听工作区配置: {}", config_path.display());
        }
        Ok(())
    }

    /// 停止监听项目配置
    pub fn unwatch_project_config(&mut self, project_root: &Path) -> Result<()> {
        let config_path = ConfigPaths::project_config(project_root);
        if let Some(parent) = config_path.parent() {
            self.watcher
                .unwatch(parent)
                .with_context(|| format!("停止监听项目配置失败: {}", parent.display()))?;

            let mut watched = self
                .watched_paths
                .lock()
                .expect("watched_paths mutex 中毒");
            watched.remove(&config_path);

            tracing::info!("停止监听项目配置: {}", config_path.display());
        }
        Ok(())
    }

    /// 轮询配置变更事件（带防抖）
    pub fn poll_events(&self) -> Vec<ConfigChangeEvent> {
        let mut events = Vec::new();
        let now = Instant::now();

        // 非阻塞轮询所有事件
        while let Ok(result) = self.event_rx.try_recv() {
            match result {
                Ok(event) => {
                    if let EventKind::Modify(_) | EventKind::Create(_) = event.kind {
                        for path in event.paths {
                            // 检查是否是监听的配置文件
                            let watched = self
                                .watched_paths
                                .lock()
                                .expect("watched_paths mutex 中毒");
                            if let Some(change_event) = watched.get(&path) {
                                // 防抖检查
                                let mut debounce = self
                                    .debounce_map
                                    .lock()
                                    .expect("debounce_map mutex 中毒");

                                let should_notify = debounce
                                    .get(&path)
                                    .map(|last_time| now.duration_since(*last_time) > self.debounce_duration)
                                    .unwrap_or(true);

                                if should_notify {
                                    debounce.insert(path.clone(), now);
                                    events.push(change_event.clone());
                                    tracing::info!("检测到配置文件变更: {}", path.display());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("文件监听错误: {:#}", e);
                }
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigPaths;
    use tempfile::TempDir;

    #[test]
    fn test_config_watcher_creation() {
        let watcher = ConfigWatcher::new();
        assert!(watcher.is_ok());
    }

    #[test]
    fn test_watch_workspace_config() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());
        paths.ensure_workspace_config_dir("ws-test").unwrap();

        let mut watcher = ConfigWatcher::new().unwrap();
        let result = watcher.watch_workspace_config(&paths, "ws-test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_watch_project_config() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("project");
        std::fs::create_dir(&project_root).unwrap();
        ConfigPaths::ensure_project_config_dir(&project_root).unwrap();

        let mut watcher = ConfigWatcher::new().unwrap();
        let result = watcher.watch_project_config(&project_root);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unwatch_configs() {
        let temp = TempDir::new().unwrap();
        let paths = ConfigPaths::new(temp.path().to_path_buf());
        paths.ensure_workspace_config_dir("ws-test").unwrap();

        let mut watcher = ConfigWatcher::new().unwrap();
        watcher.watch_workspace_config(&paths, "ws-test").unwrap();
        let result = watcher.unwatch_workspace_config(&paths, "ws-test");
        assert!(result.is_ok());
    }
}
