//! Tauri 命令层 - 项目管理 API

use crate::project::{ProjectHealthChecker, ProjectManager, ProjectMeta};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// 应用状态
///
/// `ProjectManager` 内部已用细粒度锁保护 SQLite 连接与缓存，因此这里直接共享
/// `Arc<ProjectManager>`——不再套外层 `Mutex`，避免锁跨 `await` 持有导致的
/// 异步运行时阻塞与死锁风险（clippy::await_holding_lock）。
pub struct AppState {
    pub project_manager: Arc<ProjectManager>,
}

/// 扫描并添加项目
#[tauri::command]
pub async fn scan_projects(
    root: String,
    state: State<'_, AppState>,
) -> Result<Vec<ProjectMeta>, String> {
    let root_path = PathBuf::from(root);
    if !root_path.exists() {
        return Err("目录不存在".to_string());
    }

    let manager = &state.project_manager;
    let projects = manager
        .scan_and_add(&root_path)
        .await
        .map_err(|e| e.to_string())?;

    Ok(projects)
}

/// 添加单个项目
#[tauri::command]
pub async fn add_project(path: String, state: State<'_, AppState>) -> Result<ProjectMeta, String> {
    let project_path = PathBuf::from(path);
    if !project_path.exists() {
        return Err("项目路径不存在".to_string());
    }

    let manager = &state.project_manager;
    let meta = manager
        .add_project(&project_path)
        .map_err(|e| e.to_string())?;

    Ok(meta)
}

/// 删除项目
#[tauri::command]
pub async fn remove_project(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let manager = &state.project_manager;
    manager.remove_project(&id).map_err(|e| e.to_string())?;

    Ok(())
}

/// 更新项目元数据
#[tauri::command]
pub async fn update_project(
    meta: ProjectMeta,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let manager = &state.project_manager;
    manager.update_project(&meta).map_err(|e| e.to_string())?;

    Ok(())
}

/// 获取项目
#[tauri::command]
pub async fn get_project(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<ProjectMeta>, String> {
    let manager = &state.project_manager;
    let meta = manager.get_project(&id).map_err(|e| e.to_string())?;

    Ok(meta)
}

/// 列出所有项目
#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectMeta>, String> {
    let manager = &state.project_manager;
    let projects = manager.list_projects().map_err(|e| e.to_string())?;

    Ok(projects)
}

/// 列出最近打开的项目
#[tauri::command]
pub async fn list_recent(
    limit: usize,
    state: State<'_, AppState>,
) -> Result<Vec<ProjectMeta>, String> {
    let manager = &state.project_manager;
    let projects = manager.list_recent(limit).map_err(|e| e.to_string())?;

    Ok(projects)
}

/// 列出收藏项目
#[tauri::command]
pub async fn list_favorites(state: State<'_, AppState>) -> Result<Vec<ProjectMeta>, String> {
    let manager = &state.project_manager;
    let projects = manager.list_favorites().map_err(|e| e.to_string())?;

    Ok(projects)
}

/// 切换收藏状态
#[tauri::command]
pub async fn toggle_favorite(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let manager = &state.project_manager;
    manager.toggle_favorite(&id).map_err(|e| e.to_string())?;

    Ok(())
}

/// 搜索项目
#[tauri::command]
pub async fn search_projects(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<ProjectMeta>, String> {
    let manager = &state.project_manager;
    let projects = manager.search_projects(&query).map_err(|e| e.to_string())?;

    Ok(projects)
}

/// 打开项目（更新 last_opened_at）
#[tauri::command]
pub async fn open_project(id: String, state: State<'_, AppState>) -> Result<ProjectMeta, String> {
    let manager = &state.project_manager;
    let meta = manager.open_project(&id).map_err(|e| e.to_string())?;

    Ok(meta)
}

/// 检查项目健康状态
#[tauri::command]
pub async fn check_project_health(
    id: String,
    state: State<'_, AppState>,
) -> Result<crate::project::ProjectHealth, String> {
    let manager = &state.project_manager;

    // 获取项目元数据
    let meta = manager
        .get_project(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "项目不存在".to_string())?;

    // 执行健康检查
    let health = ProjectHealthChecker::check(&meta)
        .await
        .map_err(|e| e.to_string())?;

    Ok(health)
}

// 说明：`tauri::State` 没有公开构造函数，无法在单元测试里直接合成，
// 因此命令层的行为通过其唯一依赖 `ProjectManager` 覆盖（见 manager.rs 的测试
// 与 tests/project_commands.rs 的集成测试）。此处只验证 AppState 可正常构造，
// 并且满足 Tauri 对托管状态的 Send + Sync 约束。
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn assert_send_sync<T: Send + Sync + 'static>() {}

    #[test]
    fn test_app_state_is_send_sync() {
        // Tauri 的 `State<'_, T>` 要求 T: Send + Sync + 'static。
        // 这个断言保证 AppState 始终满足该约束（编译期检查）。
        assert_send_sync::<AppState>();
    }

    #[test]
    fn test_app_state_construction() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("projects.db");
        let manager = ProjectManager::new(&db_path).unwrap();

        let state = AppState {
            project_manager: Arc::new(manager),
        };

        // 通过状态访问 manager，确认锁与数据库均可用
        // 通过状态访问 manager，确认数据库可用（内部已自带细粒度锁，无需外层加锁）
        let projects = state.project_manager.list_projects().unwrap();
        assert!(projects.is_empty());
    }
}
