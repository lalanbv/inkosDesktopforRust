//! 配置管理 Tauri 命令

use crate::config::{
    AppConfig, ConfigChangeEvent, ConfigLayer, ConfigLoader, ConfigManager, ConfigPaths,
    ConfigReloader, ConfigWatcher,
};
use std::path::PathBuf;
use tauri::Emitter;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Tauri 应用状态
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<ConfigManager>>,
    pub config_loader: Arc<ConfigLoader>,
    pub config_reloader: Arc<ConfigReloader>,
    pub config_watcher: Arc<Mutex<Option<ConfigWatcher>>>,
    pub current_workspace_id: Arc<Mutex<Option<String>>>,
    pub current_project_root: Arc<Mutex<Option<PathBuf>>>,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(app_data: PathBuf) -> Self {
        let paths = ConfigPaths::new(app_data);
        let loader = ConfigLoader::new(paths);
        let mut manager = loader.init_manager().unwrap_or_else(|e| {
            tracing::error!("初始化配置管理器失败（如目录创建），回退系统默认: {:#}", e);
            ConfigManager::new()
        });
        let initial_config = manager.merged().clone();
        let reloader = ConfigReloader::new(loader.clone(), initial_config);

        Self {
            config: Arc::new(Mutex::new(manager)),
            config_loader: Arc::new(loader),
            config_reloader: Arc::new(reloader),
            config_watcher: Arc::new(Mutex::new(None)),
            current_workspace_id: Arc::new(Mutex::new(None)),
            current_project_root: Arc::new(Mutex::new(None)),
        }
    }
}

/// 获取合并后的配置
#[tauri::command]
pub async fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, String> {
    let mut mgr = state.config.lock().await;
    Ok(mgr.merged().clone())
}

/// 更新指定层级的配置（自动持久化）
#[tauri::command]
pub async fn update_config(
    state: tauri::State<'_, AppState>,
    layer: ConfigLayer,
    config: AppConfig,
) -> Result<(), String> {
    // 验证配置
    config.validate().map_err(|e| e.to_string())?;

    // 先持久化（文件 I/O 含 fsync），再取锁更新内存——避免在持 config mutex 时
    // 阻塞 tokio worker（fsync 抖动可达数十毫秒~秒级，会卡住所有 get_config 等
    // 等待该锁的命令）。崩溃一致性：save 成功后取锁前崩溃 → 下次启动 init_manager
    // 从文件加载，最终一致。
    match layer {
        ConfigLayer::System => {
            return Err("不允许修改系统配置".to_string());
        }
        ConfigLayer::User => {
            // 用户全局层常驻可写，无需工作区/项目上下文——settings 面板的核心编辑入口。
            state
                .config_loader
                .save_user_config(&config)
                .map_err(|e| e.to_string())?;
        }
        ConfigLayer::Workspace => {
            let workspace_id = state
                .current_workspace_id
                .lock()
                .await
                .as_ref()
                .ok_or("未选择工作区")?
                .clone();
            state
                .config_loader
                .save_workspace_config(&workspace_id, &config)
                .map_err(|e| e.to_string())?;
        }
        ConfigLayer::Project => {
            let project_root = state
                .current_project_root
                .lock()
                .await
                .as_ref()
                .ok_or("未选择项目")?
                .clone();
            state
                .config_loader
                .save_project_config(&project_root, &config)
                .map_err(|e| e.to_string())?;
        }
    }

    // 持久化成功后再取锁更新内存（set_* 是纯内存操作，持锁极短）
    let mut mgr = state.config.lock().await;
    match layer {
        ConfigLayer::System => unreachable!("System 在上 match 已 return"),
        ConfigLayer::User => mgr.set_user(config),
        ConfigLayer::Workspace => mgr.set_workspace(config),
        ConfigLayer::Project => mgr.set_project(config),
    }

    Ok(())
}

/// 重置指定层级的配置
#[tauri::command]
pub async fn reset_config(
    state: tauri::State<'_, AppState>,
    layer: ConfigLayer,
) -> Result<(), String> {
    // User 层：先删持久化文件（避免重启被 init_manager→apply_user_config 重新加载
    // 而"复活"，违反最小惊讶）；删失败仅 warn 不阻断（仍清内存）。
    // Workspace/Project 沿用既有语义（仅清内存，按需重载）。
    match layer {
        ConfigLayer::System => {
            return Err("不允许重置系统配置".to_string());
        }
        ConfigLayer::User => {
            if let Err(e) = state.config_loader.delete_user_config() {
                tracing::warn!("删除用户全局配置文件失败（仍清除内存）: {:#}", e);
            }
        }
        ConfigLayer::Workspace | ConfigLayer::Project => {}
    }

    let mut mgr = state.config.lock().await;
    match layer {
        ConfigLayer::System => unreachable!("System 在上 match 已 return"),
        ConfigLayer::User => mgr.clear_user(),
        ConfigLayer::Workspace => mgr.clear_workspace(),
        ConfigLayer::Project => mgr.clear_project(),
    }

    Ok(())
}

/// 加载工作区配置（工作区切换时调用）
#[tauri::command]
pub async fn load_workspace_config(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
) -> Result<(), String> {
    let mut mgr = state.config.lock().await;

    state
        .config_loader
        .apply_workspace_config(&mut mgr, &workspace_id)
        .map_err(|e| e.to_string())?;

    *state.current_workspace_id.lock().await = Some(workspace_id);

    Ok(())
}

/// 加载项目配置（项目选择时调用）
#[tauri::command]
pub async fn load_project_config(
    state: tauri::State<'_, AppState>,
    project_root: String,
) -> Result<(), String> {
    let project_path = PathBuf::from(&project_root);
    let mut mgr = state.config.lock().await;

    state
        .config_loader
        .apply_project_config(&mut mgr, &project_path)
        .map_err(|e| e.to_string())?;

    *state.current_project_root.lock().await = Some(project_path);

    Ok(())
}

/// 启动配置热重载监听
#[tauri::command]
pub async fn start_config_watch(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut watcher_guard = state.config_watcher.lock().await;

    if watcher_guard.is_some() {
        return Ok(()); // 已经启动
    }

    let mut watcher = ConfigWatcher::new().map_err(|e| e.to_string())?;
    let paths = state.config_loader.paths();

    // 监听用户全局配置（无上下文门槛，常驻监听——用户偏好随时可被外部编辑）
    watcher.watch_user_config(paths).map_err(|e| e.to_string())?;

    // 监听当前工作区配置
    if let Some(workspace_id) = state.current_workspace_id.lock().await.as_ref() {
        watcher
            .watch_workspace_config(paths, workspace_id)
            .map_err(|e| e.to_string())?;
    }

    // 监听当前项目配置
    if let Some(project_root) = state.current_project_root.lock().await.as_ref() {
        watcher
            .watch_project_config(project_root)
            .map_err(|e| e.to_string())?;
    }

    *watcher_guard = Some(watcher);

    // 启动后台轮询任务
    let state_clone = state.inner().clone();
    let app_handle_clone = app_handle.clone();

    tokio::spawn(async move {
        poll_config_changes(state_clone, app_handle_clone).await;
    });

    tracing::info!("配置热重载监听已启动");
    Ok(())
}

/// 停止配置热重载监听
#[tauri::command]
pub async fn stop_config_watch(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut watcher_guard = state.config_watcher.lock().await;
    *watcher_guard = None;
    tracing::info!("配置热重载监听已停止");
    Ok(())
}

/// 后台轮询配置变更
async fn poll_config_changes(state: AppState, app_handle: tauri::AppHandle) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        let watcher_guard = state.config_watcher.lock().await;
        if let Some(watcher) = watcher_guard.as_ref() {
            let events = watcher.poll_events();
            drop(watcher_guard); // 释放锁

            for event in events {
                match event {
                    ConfigChangeEvent::WorkspaceChanged(workspace_id) => {
                        tracing::info!("检测到工作区配置变更: {}", workspace_id);

                        let mut mgr = state.config.lock().await;
                        if let Ok(new_config) = state
                            .config_reloader
                            .reload_workspace_config(&workspace_id, &mut mgr)
                        {
                            // 通知前端
                            app_handle
                                .emit("config-changed", serde_json::json!({
                                    "layer": "Workspace",
                                    "workspace_id": workspace_id,
                                    "config": new_config,
                                }))
                                .ok();
                        }
                    }
                    ConfigChangeEvent::ProjectChanged(project_root) => {
                        tracing::info!("检测到项目配置变更: {}", project_root.display());

                        let mut mgr = state.config.lock().await;
                        if let Ok(new_config) = state
                            .config_reloader
                            .reload_project_config(&project_root, &mut mgr)
                        {
                            // 通知前端
                            app_handle
                                .emit("config-changed", serde_json::json!({
                                    "layer": "Project",
                                    "project_root": project_root,
                                    "config": new_config,
                                }))
                                .ok();
                        }
                    }
                    ConfigChangeEvent::UserChanged => {
                        tracing::info!("检测到用户全局配置变更");

                        let mut mgr = state.config.lock().await;
                        if let Ok(new_config) =
                            state.config_reloader.reload_user_config(&mut mgr)
                        {
                            // 通知前端
                            app_handle
                                .emit("config-changed", serde_json::json!({
                                    "layer": "User",
                                    "config": new_config,
                                }))
                                .ok();
                        }
                    }
                }
            }
        } else {
            break; // watcher 已停止
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    use tempfile::TempDir;

    fn create_test_state() -> (TempDir, AppState) {
        let temp = TempDir::new().unwrap();
        let state = AppState::new(temp.path().to_path_buf());
        (temp, state)
    }

    // 说明：`tauri::State` 没有公开构造函数，无法在单元测试里合成，
    // 因此这里直接驱动命令的唯一依赖 `AppState`（与命令体等价的调用路径），
    // 端到端的命令注册由 tests/config_integration.rs 覆盖。
    #[tokio::test]
    async fn test_default_config_is_info_level() {
        let (_temp, state) = create_test_state();
        let mut mgr = state.config.lock().await;
        assert_eq!(mgr.merged().logging.level, "info");
    }

    #[tokio::test]
    async fn test_workspace_layer_overrides_default() {
        let (_temp, state) = create_test_state();

        // 模拟 load_workspace_config + update_config(Workspace) 的效果
        *state.current_workspace_id.lock().await = Some("ws-123".to_string());

        let mut ws_cfg = AppConfig::default();
        ws_cfg.logging.level = "debug".to_string();
        state
            .config_loader
            .save_workspace_config("ws-123", &ws_cfg)
            .unwrap();

        let mut mgr = state.config.lock().await;
        mgr.set_workspace(ws_cfg);
        assert_eq!(mgr.merged().logging.level, "debug");
    }

    #[tokio::test]
    async fn test_saved_workspace_config_round_trips() {
        let (_temp, state) = create_test_state();

        let mut ws_cfg = AppConfig::default();
        ws_cfg.logging.level = "warn".to_string();
        state
            .config_loader
            .save_workspace_config("ws-rt", &ws_cfg)
            .unwrap();

        let loaded = state
            .config_loader
            .load_workspace_config("ws-rt")
            .unwrap();
        assert_eq!(loaded.logging.level, "warn");
    }

    #[tokio::test]
    async fn test_no_workspace_selected_by_default() {
        let (_temp, state) = create_test_state();
        assert!(state.current_workspace_id.lock().await.is_none());
        assert!(state.current_project_root.lock().await.is_none());
    }

    #[tokio::test]
    async fn test_user_layer_update_and_reset_flow() {
        // 模拟 update_config(User, cfg) 的等价路径：save_user_config + set_user。
        // （tauri::State 不可在单测合成，端到端命令注册由集成测试覆盖。）
        let (_temp, state) = create_test_state();

        // update：写入用户全局配置并应用
        let mut user_cfg = AppConfig::default();
        user_cfg.logging.level = "debug".to_string();
        user_cfg.updates.channel = "beta".to_string();
        state.config_loader.save_user_config(&user_cfg).unwrap();
        {
            let mut mgr = state.config.lock().await;
            mgr.set_user(user_cfg);
        }

        // merged 反映用户覆盖
        {
            let mut mgr = state.config.lock().await;
            assert_eq!(mgr.merged().logging.level, "debug");
            assert_eq!(mgr.merged().updates.channel, "beta");
        }

        // reset：清除用户层 → 回退系统默认
        {
            let mut mgr = state.config.lock().await;
            mgr.clear_user();
            assert_eq!(mgr.merged().logging.level, "info");
            assert_eq!(mgr.merged().updates.channel, "stable");
        }
    }
}
