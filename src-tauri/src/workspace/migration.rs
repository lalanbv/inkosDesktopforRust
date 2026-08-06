//! Phase 2 → Phase 3 自动迁移
//!
//! 检测 projects.json（旧格式），迁移为默认工作区。

use std::path::Path;
use anyhow::Context;
use serde::Deserialize;
use super::{WorkspaceList};

/// Phase 2 projects.json schema
#[derive(Debug, Deserialize)]
struct Phase2ProjectList {
    projects: Vec<Phase2Project>,
    #[serde(default)]
    last_selected: usize,
}

#[derive(Debug, Deserialize)]
struct Phase2Project {
    path: String,
}

/// 迁移 Phase 2 数据到 Phase 3（自动触发，幂等）
///
/// 返回：true = 执行了迁移，false = 不需要迁移
pub fn migrate_from_phase2(app_data_dir: &Path) -> anyhow::Result<bool> {
    let ws_path = app_data_dir.join("workspaces.json");
    let old_path = app_data_dir.join("projects.json");

    // 已迁移（workspaces.json 存在）→ 跳过
    if ws_path.exists() {
        return Ok(false);
    }

    // 无旧数据 → 跳过
    if !old_path.exists() {
        return Ok(false);
    }

    // 读取旧格式
    let old_json = std::fs::read(&old_path)
        .context("读取 projects.json 失败")?;
    let old_list: Phase2ProjectList = serde_json::from_slice(&old_json)
        .context("解析 projects.json 失败")?;

    // 创建默认工作区
    let mut ws_list = WorkspaceList::default();
    let id = ws_list.create("默认工作区");

    // 迁移项目列表
    for proj in old_list.projects {
        let _ = ws_list.add_project(&id, &proj.path); // 忽略重复错误
    }

    // 设置为激活工作区
    ws_list.switch(&id).expect("切换默认工作区失败");

    // 写入新格式
    ws_list.write(&ws_path).context("写入 workspaces.json 失败")?;

    // 备份旧文件（保留，允许用户回退）
    let backup_path = app_data_dir.join("projects.json.phase2-backup");
    std::fs::rename(&old_path, &backup_path)
        .context("备份 projects.json 失败")?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_migrate_from_projects_json() {
        let temp = TempDir::new().unwrap();
        let app_data = temp.path();

        // 创建旧格式 projects.json
        let projects_json = r#"{
            "projects": [
                {"path": "/path/to/project1"},
                {"path": "/path/to/project2"}
            ],
            "last_selected": 0
        }"#;
        std::fs::write(app_data.join("projects.json"), projects_json).unwrap();

        // 执行迁移
        let migrated = migrate_from_phase2(app_data).unwrap();
        assert!(migrated);

        // 验证 workspaces.json 生成
        let ws_list = WorkspaceList::read(&app_data.join("workspaces.json")).unwrap();
        assert_eq!(ws_list.workspaces.len(), 1);
        assert_eq!(ws_list.workspaces[0].name, "默认工作区");
        assert_eq!(ws_list.workspaces[0].projects.len(), 2);
        assert_eq!(ws_list.active_id, Some(ws_list.workspaces[0].id.clone()));

        // 验证 projects.json 备份
        assert!(app_data.join("projects.json.phase2-backup").exists());
    }

    #[test]
    fn test_no_migration_needed_workspace_exists() {
        let temp = TempDir::new().unwrap();
        let app_data = temp.path();

        // workspaces.json 已存在
        let ws_list = WorkspaceList::default();
        ws_list.write(&app_data.join("workspaces.json")).unwrap();

        let migrated = migrate_from_phase2(app_data).unwrap();
        assert!(!migrated); // 不需要迁移
    }

    #[test]
    fn test_no_migration_needed_no_old_data() {
        let temp = TempDir::new().unwrap();
        let app_data = temp.path();

        // 无任何文件
        let migrated = migrate_from_phase2(app_data).unwrap();
        assert!(!migrated); // 不需要迁移
    }
}
