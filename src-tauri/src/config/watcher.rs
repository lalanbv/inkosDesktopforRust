//! 配置文件监听器（热重载支持）

use crate::config::ConfigPaths;
use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
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

/// 归一化配置文件路径，用于监听表的键与事件路径比对。
///
/// 必要性：文件系统后端（macOS FSEvents、Linux inotify）上报的是**解析过符号链接**
/// 的真实路径。而调用方传入的路径常常带符号链接——macOS 上 `/var` 就是
/// `/private/var` 的符号链接（`TempDir` 正落在这里），`$TMPDIR` 同理。若直接用原始
/// 路径做 HashMap 键，事件路径永远匹配不上，热重载静默失效。
///
/// 配置文件本身可能尚未创建（首次写入前就开始监听），因此只对父目录做
/// `canonicalize`，再拼回文件名。父目录也无法解析时退回原路径。
fn normalize_config_path(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(file_name)) => match parent.canonicalize() {
            Ok(real_parent) => real_parent.join(file_name),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
    }
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
                normalize_config_path(&config_path),
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
                normalize_config_path(&config_path),
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
            // 键在 watch 时归一化过，这里必须用同样的形式，否则条目会残留。
            watched.remove(&normalize_config_path(&config_path));

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
            // 键在 watch 时归一化过，这里必须用同样的形式，否则条目会残留。
            watched.remove(&normalize_config_path(&config_path));

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
                            // 事件路径已由后端解析过符号链接；监听表的键在插入时做过
                            // 同样的归一化，因此可以直接比对。
                            let change_event = {
                                let watched = self
                                    .watched_paths
                                    .lock()
                                    .expect("watched_paths mutex 中毒");
                                match watched.get(&path) {
                                    Some(e) => e.clone(),
                                    None => continue,
                                }
                            }; // 先放掉 watched_paths，避免与 debounce_map 嵌套持锁

                            // 防抖：同一文件在窗口内的多次写入只上报一次
                            // （编辑器保存常触发 truncate + write 两个事件）
                            let mut debounce = self
                                .debounce_map
                                .lock()
                                .expect("debounce_map mutex 中毒");

                            let should_notify = debounce
                                .get(&path)
                                .map(|last| now.duration_since(*last) > self.debounce_duration)
                                .unwrap_or(true);

                            if should_notify {
                                debounce.insert(path.clone(), now);
                                events.push(change_event);
                                tracing::info!("检测到配置文件变更: {}", path.display());
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
