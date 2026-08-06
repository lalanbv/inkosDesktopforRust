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
}
