//! 工作区管理（多项目并行工作）
//!
//! 工作区隔离：独立项目列表、Engine 版本、配置。
//! 数据持久化：workspaces.json（app_data_dir）。

use serde::{Deserialize, Serialize};

/// 工作区 ID（UUID v4 格式字符串）
pub type WorkspaceId = String;

/// 工作区元数据
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub created_at: i64,       // Unix 时间戳（秒）
    pub last_used: i64,        // 最近使用时间
    pub projects: Vec<String>, // 项目绝对路径列表
    pub engine_version: Option<String>, // 独立 Engine 版本（None = 全局默认）
}

/// 工作区列表（workspaces.json schema）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceList {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_id: Option<WorkspaceId>,
}

impl Default for WorkspaceList {
    fn default() -> Self {
        Self {
            workspaces: Vec::new(),
            active_id: None,
        }
    }
}

impl WorkspaceList {
    /// 创建新工作区（UUID v4 ID，Unix 时间戳）
    pub fn create(&mut self, name: &str) -> WorkspaceId {
        use std::time::{SystemTime, UNIX_EPOCH};

        let id = uuid::Uuid::new_v4().to_string();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let workspace = Workspace {
            id: id.clone(),
            name: name.to_string(),
            created_at: now,
            last_used: now,
            projects: Vec::new(),
            engine_version: None,
        };

        self.workspaces.push(workspace);
        id
    }

    /// 删除工作区（必须存在）
    pub fn delete(&mut self, id: &WorkspaceId) -> Result<(), String> {
        let pos = self
            .workspaces
            .iter()
            .position(|w| &w.id == id)
            .ok_or_else(|| format!("Workspace not found: {}", id))?;

        self.workspaces.remove(pos);

        // 如果删除的是当前激活工作区，清空 active_id
        if self.active_id.as_ref() == Some(id) {
            self.active_id = None;
        }

        Ok(())
    }

    /// 查找工作区（不可变引用，0GC）
    pub fn find(&self, id: &WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| &w.id == id)
    }

    /// 查找工作区（可变引用）
    pub fn find_mut(&mut self, id: &WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| &w.id == id)
    }

    /// 切换当前激活工作区（<50ms 性能目标）
    pub fn switch(&mut self, id: &WorkspaceId) -> Result<(), String> {
        use std::time::{SystemTime, UNIX_EPOCH};

        // 验证工作区存在
        let ws = self
            .find_mut(id)
            .ok_or_else(|| format!("Workspace not found: {}", id))?;

        // 更新 last_used 时间戳
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        ws.last_used = now;

        // 设置为当前激活
        self.active_id = Some(id.clone());

        Ok(())
    }

    /// 添加项目到工作区（去重）
    pub fn add_project(&mut self, id: &WorkspaceId, path: &str) -> Result<(), String> {
        let ws = self
            .find_mut(id)
            .ok_or_else(|| format!("Workspace not found: {}", id))?;

        // 检查重复
        if ws.projects.iter().any(|p| p == path) {
            return Err(format!("Project already exists: {}", path));
        }

        ws.projects.push(path.to_string());
        Ok(())
    }

    /// 从工作区移除项目
    pub fn remove_project(&mut self, id: &WorkspaceId, path: &str) -> Result<(), String> {
        let ws = self
            .find_mut(id)
            .ok_or_else(|| format!("Workspace not found: {}", id))?;

        let pos = ws
            .projects
            .iter()
            .position(|p| p == path)
            .ok_or_else(|| format!("Project not found: {}", path))?;

        ws.projects.remove(pos);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_serialization() {
        let ws = Workspace {
            id: "test-id".to_string(),
            name: "Test Workspace".to_string(),
            created_at: 1234567890,
            last_used: 1234567890,
            projects: vec!["/path/to/project".to_string()],
            engine_version: Some("0.4.0".to_string()),
        };

        let json = serde_json::to_string(&ws).unwrap();
        let parsed: Workspace = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, "test-id");
        assert_eq!(parsed.name, "Test Workspace");
        assert_eq!(parsed.projects.len(), 1);
    }

    #[test]
    fn test_workspace_list_default() {
        let list = WorkspaceList::default();
        assert!(list.workspaces.is_empty());
        assert!(list.active_id.is_none());
    }

    #[test]
    fn test_create_workspace() {
        let mut list = WorkspaceList::default();
        let id = list.create("My Workspace");

        assert_eq!(list.workspaces.len(), 1);
        assert_eq!(list.workspaces[0].name, "My Workspace");
        assert_eq!(list.workspaces[0].id, id);
        assert!(list.workspaces[0].projects.is_empty());
    }

    #[test]
    fn test_delete_workspace() {
        let mut list = WorkspaceList::default();
        let id = list.create("Test");

        assert!(list.delete(&id).is_ok());
        assert_eq!(list.workspaces.len(), 0);
    }

    #[test]
    fn test_delete_nonexistent_workspace() {
        let mut list = WorkspaceList::default();
        let result = list.delete(&"nonexistent".to_string());

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_find_workspace() {
        let mut list = WorkspaceList::default();
        let id = list.create("Test");

        let found = list.find(&id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Test");

        let not_found = list.find(&"invalid".to_string());
        assert!(not_found.is_none());
    }

    #[test]
    fn test_switch_workspace() {
        let mut list = WorkspaceList::default();
        let id = list.create("Test");

        assert!(list.switch(&id).is_ok());
        assert_eq!(list.active_id, Some(id.clone()));
    }

    #[test]
    fn test_switch_nonexistent_workspace() {
        let mut list = WorkspaceList::default();
        let result = list.switch(&"invalid".to_string());

        assert!(result.is_err());
    }

    #[test]
    fn test_add_project() {
        let mut list = WorkspaceList::default();
        let id = list.create("Test");

        assert!(list.add_project(&id, "/path/to/project").is_ok());

        let ws = list.find(&id).unwrap();
        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.projects[0], "/path/to/project");
    }

    #[test]
    fn test_add_duplicate_project() {
        let mut list = WorkspaceList::default();
        let id = list.create("Test");

        list.add_project(&id, "/path/to/project").unwrap();
        let result = list.add_project(&id, "/path/to/project");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn test_remove_project() {
        let mut list = WorkspaceList::default();
        let id = list.create("Test");
        list.add_project(&id, "/path/to/project").unwrap();

        assert!(list.remove_project(&id, "/path/to/project").is_ok());

        let ws = list.find(&id).unwrap();
        assert!(ws.projects.is_empty());
    }
}
