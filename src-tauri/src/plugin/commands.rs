//! Tauri 命令层 - 插件管理 API

use crate::plugin::{PluginManager, PluginMetadata};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// 应用状态（插件管理器）
pub struct PluginState {
    pub manager: Arc<Mutex<PluginManager>>,
}

/// 列出所有已安装的插件
#[tauri::command]
pub async fn list_plugins(state: State<'_, PluginState>) -> Result<Vec<PluginMetadata>, String> {
    let manager = state.manager.lock().await;
    let plugins = manager.list_plugins();
    Ok(plugins)
}

/// 安装插件
#[tauri::command]
pub async fn install_plugin(
    path: String,
    state: State<'_, PluginState>,
) -> Result<PluginMetadata, String> {
    let mut manager = state.manager.lock().await;
    manager
        .install_plugin(&path)
        .await
        .map_err(|e| format!("安装插件失败: {}", e))
}

/// 卸载插件
#[tauri::command]
pub async fn uninstall_plugin(id: String, state: State<'_, PluginState>) -> Result<(), String> {
    let mut manager = state.manager.lock().await;
    manager
        .uninstall_plugin(&id)
        .map_err(|e| format!("卸载插件失败: {}", e))
}

/// 启用插件
#[tauri::command]
pub async fn enable_plugin(id: String, state: State<'_, PluginState>) -> Result<(), String> {
    let mut manager = state.manager.lock().await;
    manager
        .enable_plugin(&id)
        .map_err(|e| format!("启用插件失败: {}", e))
}

/// 禁用插件
#[tauri::command]
pub async fn disable_plugin(id: String, state: State<'_, PluginState>) -> Result<(), String> {
    let mut manager = state.manager.lock().await;
    manager
        .disable_plugin(&id)
        .map_err(|e| format!("禁用插件失败: {}", e))
}

/// 获取插件详情
#[tauri::command]
pub async fn get_plugin(
    id: String,
    state: State<'_, PluginState>,
) -> Result<Option<PluginMetadata>, String> {
    let manager = state.manager.lock().await;
    Ok(manager.get_plugin(&id))
}

/// 执行插件命令
#[tauri::command]
pub async fn execute_plugin(
    id: String,
    command: String,
    args: serde_json::Value,
    state: State<'_, PluginState>,
) -> Result<serde_json::Value, String> {
    let mut manager = state.manager.lock().await;
    manager
        .execute_plugin(&id, &command, args)
        .await
        .map_err(|e| format!("执行插件命令失败: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 说明：Tauri 命令层的测试依赖 `tauri::State`，该类型没有公开构造函数，
    // 无法在单元测试中直接创建。命令层的行为通过其唯一依赖 `PluginManager`
    // 的单元测试覆盖（见 manager.rs），并通过集成测试（tests/plugin_*.rs）
    // 验证端到端行为。此处仅验证 PluginState 可正常构造，并且满足 Tauri
    // 对托管状态的 Send + Sync 约束。

    fn assert_send_sync<T: Send + Sync + 'static>() {}

    #[test]
    fn test_plugin_state_is_send_sync() {
        // Tauri 的 `State<'_, T>` 要求 T: Send + Sync + 'static。
        // 这个断言保证 PluginState 始终满足该约束（编译期检查）。
        assert_send_sync::<PluginState>();
    }

    #[tokio::test]
    async fn test_plugin_state_construction() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let manager = PluginManager::new(temp_dir.path()).unwrap();

        let state = PluginState {
            manager: Arc::new(Mutex::new(manager)),
        };

        // 通过状态访问 manager，确认锁与数据可用
        let manager = state.manager.lock().await;
        let plugins = manager.list_plugins();
        assert!(plugins.is_empty());
    }
}
