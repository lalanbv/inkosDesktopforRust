//! 项目索引数据库操作层

use crate::project::types::{HealthStatus, ProjectHealth, ProjectMeta, ProjectType};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

/// 项目索引数据库
pub struct ProjectIndex {
    conn: Connection,
}

impl ProjectIndex {
    /// 打开或创建数据库
    pub fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)
            .context(format!("Failed to open database at {:?}", db_path))?;

        let index = Self { conn };
        index.init_schema()?;
        Ok(index)
    }

    /// 初始化数据库 Schema
    fn init_schema(&self) -> Result<()> {
        // 创建项目表
        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                project_type TEXT NOT NULL,
                workspace_id TEXT,
                created_at INTEGER NOT NULL,
                last_opened_at INTEGER,
                last_scanned_at INTEGER,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                tags TEXT,
                description TEXT
            )
            "#,
            [],
        )?;

        // 创建健康检查表
        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS project_health (
                project_id TEXT PRIMARY KEY,
                checked_at INTEGER NOT NULL,
                status TEXT NOT NULL,
                issues TEXT,
                dependency_count INTEGER NOT NULL,
                missing_dependencies TEXT,
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            )
            "#,
            [],
        )?;

        // 创建索引
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_projects_workspace ON projects(workspace_id)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_projects_last_opened ON projects(last_opened_at DESC)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_projects_favorite ON projects(is_favorite)",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_projects_type ON projects(project_type)",
            [],
        )?;

        Ok(())
    }

    /// 插入项目
    pub fn insert(&self, meta: &ProjectMeta) -> Result<()> {
        let tags_json = serde_json::to_string(&meta.tags)?;
        let path_str = meta.path.to_string_lossy();

        self.conn.execute(
            r#"
            INSERT INTO projects (
                id, name, path, project_type, workspace_id,
                created_at, last_opened_at, last_scanned_at,
                is_favorite, tags, description
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                meta.id,
                meta.name,
                path_str,
                meta.project_type.as_str(),
                meta.workspace_id,
                meta.created_at,
                meta.last_opened_at,
                meta.last_scanned_at,
                meta.is_favorite as i32,
                tags_json,
                meta.description,
            ],
        )?;

        Ok(())
    }

    /// 更新项目
    pub fn update(&self, meta: &ProjectMeta) -> Result<()> {
        let tags_json = serde_json::to_string(&meta.tags)?;
        let path_str = meta.path.to_string_lossy();

        let rows_affected = self.conn.execute(
            r#"
            UPDATE projects SET
                name = ?2,
                path = ?3,
                project_type = ?4,
                workspace_id = ?5,
                last_opened_at = ?6,
                last_scanned_at = ?7,
                is_favorite = ?8,
                tags = ?9,
                description = ?10
            WHERE id = ?1
            "#,
            params![
                meta.id,
                meta.name,
                path_str,
                meta.project_type.as_str(),
                meta.workspace_id,
                meta.last_opened_at,
                meta.last_scanned_at,
                meta.is_favorite as i32,
                tags_json,
                meta.description,
            ],
        )?;

        if rows_affected == 0 {
            anyhow::bail!("Project not found: {}", meta.id);
        }

        Ok(())
    }

    /// 删除项目
    pub fn delete(&self, id: &str) -> Result<()> {
        let rows_affected = self
            .conn
            .execute("DELETE FROM projects WHERE id = ?1", params![id])?;

        if rows_affected == 0 {
            anyhow::bail!("Project not found: {}", id);
        }

        Ok(())
    }

    /// 根据 ID 查询项目
    pub fn get_by_id(&self, id: &str) -> Result<Option<ProjectMeta>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, name, path, project_type, workspace_id, created_at,
                        last_opened_at, last_scanned_at, is_favorite, tags, description
                 FROM projects WHERE id = ?1",
                params![id],
                |row| {
                    let tags_json: String = row.get(9)?;
                    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

                    Ok(ProjectMeta {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        path: PathBuf::from(row.get::<_, String>(2)?),
                        project_type: ProjectType::from_str(&row.get::<_, String>(3)?)
                            .unwrap_or(ProjectType::Generic),
                        workspace_id: row.get(4)?,
                        created_at: row.get(5)?,
                        last_opened_at: row.get(6)?,
                        last_scanned_at: row.get(7)?,
                        is_favorite: row.get::<_, i32>(8)? != 0,
                        tags,
                        description: row.get(10)?,
                    })
                },
            )
            .optional()?;

        Ok(result)
    }

    /// 根据路径查询项目
    pub fn get_by_path(&self, path: &Path) -> Result<Option<ProjectMeta>> {
        let path_str = path.to_string_lossy();

        let result = self
            .conn
            .query_row(
                "SELECT id, name, path, project_type, workspace_id, created_at,
                        last_opened_at, last_scanned_at, is_favorite, tags, description
                 FROM projects WHERE path = ?1",
                params![path_str],
                |row| {
                    let tags_json: String = row.get(9)?;
                    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

                    Ok(ProjectMeta {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        path: PathBuf::from(row.get::<_, String>(2)?),
                        project_type: ProjectType::from_str(&row.get::<_, String>(3)?)
                            .unwrap_or(ProjectType::Generic),
                        workspace_id: row.get(4)?,
                        created_at: row.get(5)?,
                        last_opened_at: row.get(6)?,
                        last_scanned_at: row.get(7)?,
                        is_favorite: row.get::<_, i32>(8)? != 0,
                        tags,
                        description: row.get(10)?,
                    })
                },
            )
            .optional()?;

        Ok(result)
    }

    /// 列出所有项目
    pub fn list_all(&self) -> Result<Vec<ProjectMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, project_type, workspace_id, created_at,
                    last_opened_at, last_scanned_at, is_favorite, tags, description
             FROM projects ORDER BY created_at DESC",
        )?;

        let projects = stmt
            .query_map([], |row| {
                let tags_json: String = row.get(9)?;
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

                Ok(ProjectMeta {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: PathBuf::from(row.get::<_, String>(2)?),
                    project_type: ProjectType::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(ProjectType::Generic),
                    workspace_id: row.get(4)?,
                    created_at: row.get(5)?,
                    last_opened_at: row.get(6)?,
                    last_scanned_at: row.get(7)?,
                    is_favorite: row.get::<_, i32>(8)? != 0,
                    tags,
                    description: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    /// 列出工作区项目
    pub fn list_by_workspace(&self, workspace_id: &str) -> Result<Vec<ProjectMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, project_type, workspace_id, created_at,
                    last_opened_at, last_scanned_at, is_favorite, tags, description
             FROM projects WHERE workspace_id = ?1 ORDER BY created_at DESC",
        )?;

        let projects = stmt
            .query_map(params![workspace_id], |row| {
                let tags_json: String = row.get(9)?;
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

                Ok(ProjectMeta {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: PathBuf::from(row.get::<_, String>(2)?),
                    project_type: ProjectType::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(ProjectType::Generic),
                    workspace_id: row.get(4)?,
                    created_at: row.get(5)?,
                    last_opened_at: row.get(6)?,
                    last_scanned_at: row.get(7)?,
                    is_favorite: row.get::<_, i32>(8)? != 0,
                    tags,
                    description: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    /// 列出最近打开的项目
    pub fn list_recent(&self, limit: usize) -> Result<Vec<ProjectMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, project_type, workspace_id, created_at,
                    last_opened_at, last_scanned_at, is_favorite, tags, description
             FROM projects
             WHERE last_opened_at IS NOT NULL
             ORDER BY last_opened_at DESC
             LIMIT ?1",
        )?;

        let projects = stmt
            .query_map(params![limit], |row| {
                let tags_json: String = row.get(9)?;
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

                Ok(ProjectMeta {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: PathBuf::from(row.get::<_, String>(2)?),
                    project_type: ProjectType::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(ProjectType::Generic),
                    workspace_id: row.get(4)?,
                    created_at: row.get(5)?,
                    last_opened_at: row.get(6)?,
                    last_scanned_at: row.get(7)?,
                    is_favorite: row.get::<_, i32>(8)? != 0,
                    tags,
                    description: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    /// 列出收藏项目
    pub fn list_favorites(&self) -> Result<Vec<ProjectMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, project_type, workspace_id, created_at,
                    last_opened_at, last_scanned_at, is_favorite, tags, description
             FROM projects WHERE is_favorite = 1 ORDER BY name ASC",
        )?;

        let projects = stmt
            .query_map([], |row| {
                let tags_json: String = row.get(9)?;
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

                Ok(ProjectMeta {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: PathBuf::from(row.get::<_, String>(2)?),
                    project_type: ProjectType::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(ProjectType::Generic),
                    workspace_id: row.get(4)?,
                    created_at: row.get(5)?,
                    last_opened_at: row.get(6)?,
                    last_scanned_at: row.get(7)?,
                    is_favorite: row.get::<_, i32>(8)? != 0,
                    tags,
                    description: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    /// 搜索项目（按名称）
    pub fn search(&self, query: &str) -> Result<Vec<ProjectMeta>> {
        let search_pattern = format!("%{}%", query);

        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, project_type, workspace_id, created_at,
                    last_opened_at, last_scanned_at, is_favorite, tags, description
             FROM projects WHERE name LIKE ?1 ORDER BY name ASC",
        )?;

        let projects = stmt
            .query_map(params![search_pattern], |row| {
                let tags_json: String = row.get(9)?;
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

                Ok(ProjectMeta {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: PathBuf::from(row.get::<_, String>(2)?),
                    project_type: ProjectType::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(ProjectType::Generic),
                    workspace_id: row.get(4)?,
                    created_at: row.get(5)?,
                    last_opened_at: row.get(6)?,
                    last_scanned_at: row.get(7)?,
                    is_favorite: row.get::<_, i32>(8)? != 0,
                    tags,
                    description: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    /// 保存健康检查结果
    pub fn save_health(&self, health: &ProjectHealth) -> Result<()> {
        let issues_json = serde_json::to_string(&health.issues)?;
        let missing_deps_json = serde_json::to_string(&health.missing_dependencies)?;

        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO project_health (
                project_id, checked_at, status, issues,
                dependency_count, missing_dependencies
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                health.project_id,
                health.checked_at,
                health.status.as_str(),
                issues_json,
                health.dependency_count,
                missing_deps_json,
            ],
        )?;

        Ok(())
    }

    /// 获取健康检查结果
    pub fn get_health(&self, project_id: &str) -> Result<Option<ProjectHealth>> {
        let result = self
            .conn
            .query_row(
                "SELECT project_id, checked_at, status, issues,
                        dependency_count, missing_dependencies
                 FROM project_health WHERE project_id = ?1",
                params![project_id],
                |row| {
                    let issues_json: String = row.get(3)?;
                    let issues = serde_json::from_str(&issues_json).unwrap_or_default();

                    let missing_deps_json: String = row.get(5)?;
                    let missing_dependencies =
                        serde_json::from_str(&missing_deps_json).unwrap_or_default();

                    Ok(ProjectHealth {
                        project_id: row.get(0)?,
                        checked_at: row.get(1)?,
                        status: HealthStatus::from_str(&row.get::<_, String>(2)?)
                            .unwrap_or(HealthStatus::Unknown),
                        issues,
                        dependency_count: row.get(4)?,
                        missing_dependencies,
                    })
                },
            )
            .optional()?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_db() -> (TempDir, ProjectIndex) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let index = ProjectIndex::open(&db_path).unwrap();
        (temp_dir, index)
    }

    #[test]
    fn test_database_initialization() {
        let (_temp, _index) = create_test_db();
        // 如果没有 panic，说明初始化成功
    }

    #[test]
    fn test_insert_and_get_by_id() {
        let (_temp, index) = create_test_db();

        let meta = ProjectMeta::new(
            "test-project".to_string(),
            PathBuf::from("/path/to/project"),
            ProjectType::NodeJs,
        );

        index.insert(&meta).unwrap();

        let retrieved = index.get_by_id(&meta.id).unwrap().unwrap();
        assert_eq!(retrieved.id, meta.id);
        assert_eq!(retrieved.name, meta.name);
        assert_eq!(retrieved.path, meta.path);
        assert_eq!(retrieved.project_type, ProjectType::NodeJs);
    }

    #[test]
    fn test_insert_duplicate_path_fails() {
        let (_temp, index) = create_test_db();

        let meta1 = ProjectMeta::new(
            "project-1".to_string(),
            PathBuf::from("/same/path"),
            ProjectType::NodeJs,
        );

        let meta2 = ProjectMeta::new(
            "project-2".to_string(),
            PathBuf::from("/same/path"),
            ProjectType::Python,
        );

        index.insert(&meta1).unwrap();
        let result = index.insert(&meta2);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_project() {
        let (_temp, index) = create_test_db();

        let mut meta = ProjectMeta::new(
            "original-name".to_string(),
            PathBuf::from("/path"),
            ProjectType::NodeJs,
        );

        index.insert(&meta).unwrap();

        meta.name = "updated-name".to_string();
        meta.is_favorite = true;
        meta.tags = vec!["tag1".to_string(), "tag2".to_string()];

        index.update(&meta).unwrap();

        let retrieved = index.get_by_id(&meta.id).unwrap().unwrap();
        assert_eq!(retrieved.name, "updated-name");
        assert!(retrieved.is_favorite);
        assert_eq!(retrieved.tags.len(), 2);
    }

    #[test]
    fn test_delete_project() {
        let (_temp, index) = create_test_db();

        let meta = ProjectMeta::new(
            "to-delete".to_string(),
            PathBuf::from("/path"),
            ProjectType::Generic,
        );

        index.insert(&meta).unwrap();
        assert!(index.get_by_id(&meta.id).unwrap().is_some());

        index.delete(&meta.id).unwrap();
        assert!(index.get_by_id(&meta.id).unwrap().is_none());
    }

    #[test]
    fn test_get_by_path() {
        let (_temp, index) = create_test_db();

        let path = PathBuf::from("/unique/path");
        let meta = ProjectMeta::new("test".to_string(), path.clone(), ProjectType::Rust);

        index.insert(&meta).unwrap();

        let retrieved = index.get_by_path(&path).unwrap().unwrap();
        assert_eq!(retrieved.id, meta.id);
    }

    #[test]
    fn test_list_all() {
        let (_temp, index) = create_test_db();

        for i in 0..3 {
            let meta = ProjectMeta::new(
                format!("project-{}", i),
                PathBuf::from(format!("/path/{}", i)),
                ProjectType::Generic,
            );
            index.insert(&meta).unwrap();
        }

        let all = index.list_all().unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_list_by_workspace() {
        let (_temp, index) = create_test_db();

        let mut meta1 = ProjectMeta::new(
            "proj1".to_string(),
            PathBuf::from("/path1"),
            ProjectType::NodeJs,
        );
        meta1.workspace_id = Some("workspace-1".to_string());

        let mut meta2 = ProjectMeta::new(
            "proj2".to_string(),
            PathBuf::from("/path2"),
            ProjectType::Python,
        );
        meta2.workspace_id = Some("workspace-1".to_string());

        let mut meta3 = ProjectMeta::new(
            "proj3".to_string(),
            PathBuf::from("/path3"),
            ProjectType::Rust,
        );
        meta3.workspace_id = Some("workspace-2".to_string());

        index.insert(&meta1).unwrap();
        index.insert(&meta2).unwrap();
        index.insert(&meta3).unwrap();

        let workspace1_projects = index.list_by_workspace("workspace-1").unwrap();
        assert_eq!(workspace1_projects.len(), 2);
    }

    #[test]
    fn test_list_recent() {
        let (_temp, index) = create_test_db();

        let mut meta1 = ProjectMeta::new(
            "proj1".to_string(),
            PathBuf::from("/path1"),
            ProjectType::Generic,
        );
        meta1.last_opened_at = Some(1000);

        let mut meta2 = ProjectMeta::new(
            "proj2".to_string(),
            PathBuf::from("/path2"),
            ProjectType::Generic,
        );
        meta2.last_opened_at = Some(2000);

        let meta3 = ProjectMeta::new(
            "proj3".to_string(),
            PathBuf::from("/path3"),
            ProjectType::Generic,
        );
        // meta3 没有 last_opened_at

        index.insert(&meta1).unwrap();
        index.insert(&meta2).unwrap();
        index.insert(&meta3).unwrap();

        let recent = index.list_recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].name, "proj2"); // 最近的在前
    }

    #[test]
    fn test_list_favorites() {
        let (_temp, index) = create_test_db();

        let mut meta1 = ProjectMeta::new(
            "fav1".to_string(),
            PathBuf::from("/path1"),
            ProjectType::Generic,
        );
        meta1.is_favorite = true;

        let meta2 = ProjectMeta::new(
            "normal".to_string(),
            PathBuf::from("/path2"),
            ProjectType::Generic,
        );

        index.insert(&meta1).unwrap();
        index.insert(&meta2).unwrap();

        let favorites = index.list_favorites().unwrap();
        assert_eq!(favorites.len(), 1);
        assert_eq!(favorites[0].name, "fav1");
    }

    #[test]
    fn test_search_projects() {
        let (_temp, index) = create_test_db();

        let meta1 = ProjectMeta::new(
            "my-app".to_string(),
            PathBuf::from("/path1"),
            ProjectType::NodeJs,
        );

        let meta2 = ProjectMeta::new(
            "your-app".to_string(),
            PathBuf::from("/path2"),
            ProjectType::Python,
        );

        let meta3 = ProjectMeta::new(
            "another-project".to_string(),
            PathBuf::from("/path3"),
            ProjectType::Rust,
        );

        index.insert(&meta1).unwrap();
        index.insert(&meta2).unwrap();
        index.insert(&meta3).unwrap();

        let results = index.search("app").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_save_and_get_health() {
        let (_temp, index) = create_test_db();

        let meta = ProjectMeta::new(
            "test".to_string(),
            PathBuf::from("/path"),
            ProjectType::NodeJs,
        );
        index.insert(&meta).unwrap();

        let mut health = ProjectHealth::new(meta.id.clone());
        health.status = HealthStatus::Healthy;
        health.dependency_count = 10;

        index.save_health(&health).unwrap();

        let retrieved = index.get_health(&meta.id).unwrap().unwrap();
        assert_eq!(retrieved.project_id, meta.id);
        assert_eq!(retrieved.status, HealthStatus::Healthy);
        assert_eq!(retrieved.dependency_count, 10);
    }

    #[test]
    fn test_health_cascade_delete() {
        let (_temp, index) = create_test_db();

        let meta = ProjectMeta::new(
            "test".to_string(),
            PathBuf::from("/path"),
            ProjectType::NodeJs,
        );
        index.insert(&meta).unwrap();

        let health = ProjectHealth::new(meta.id.clone());
        index.save_health(&health).unwrap();

        assert!(index.get_health(&meta.id).unwrap().is_some());

        index.delete(&meta.id).unwrap();

        assert!(index.get_health(&meta.id).unwrap().is_none());
    }
}
