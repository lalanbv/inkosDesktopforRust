//! 项目生命周期管理器

use crate::project::{ProjectDetector, ProjectIndex, ProjectMeta, ProjectScanner};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// 项目生命周期管理器
///
/// 统一项目 CRUD + 内存缓存，整合 Index/Detector/Scanner
pub struct ProjectManager {
    // 直接持有（不套 Arc）：rusqlite::Connection 是 Send 但不是 Sync，
    // Arc<T>: Send 要求 T: Send + Sync，因此 Arc<ProjectIndex> 反而破坏 Send。
    // 调用方（AppState）已用 Mutex<ProjectManager> 提供同步，无需 unsafe。
    index: ProjectIndex,
    scanner: ProjectScanner,
    cache: Arc<Mutex<HashMap<String, ProjectMeta>>>,
}

impl ProjectManager {
    /// 创建管理器
    pub fn new(db_path: &Path) -> Result<Self> {
        let index = ProjectIndex::open(db_path)
            .context("Failed to open project database")?;

        Ok(Self {
            index,
            scanner: ProjectScanner::new(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 扫描并添加项目（异步）
    ///
    /// 扫描指定目录，发现所有项目并添加到数据库。
    /// 返回新添加的项目列表（已存在的项目会被跳过）。
    pub async fn scan_and_add(&self, root: &Path) -> Result<Vec<ProjectMeta>> {
        // 扫描目录
        let discovered = self.scanner.scan(root).await
            .context("Failed to scan directory")?;

        let mut added = Vec::new();

        for meta in discovered {
            // 检查项目是否已存在
            match self.index.get_by_path(&meta.path)? {
                Some(_existing) => {
                    // 已存在，跳过
                    tracing::debug!("项目已存在，跳过: {:?}", meta.path);
                }
                None => {
                    // 新项目，添加到数据库
                    self.index.insert(&meta)
                        .context(format!("Failed to insert project: {:?}", meta.path))?;

                    // 更新缓存
                    self.cache.lock().expect("cache mutex 中毒").insert(meta.id.clone(), meta.clone());

                    tracing::info!("添加项目: {} ({:?})", meta.name, meta.path);
                    added.push(meta);
                }
            }
        }

        Ok(added)
    }

    /// 添加单个项目
    pub fn add_project(&self, path: &Path) -> Result<ProjectMeta> {
        // 检查是否已存在
        if self.index.get_by_path(path)?.is_some() {
            anyhow::bail!("项目已存在: {:?}", path);
        }

        // 检测项目类型
        let (project_type, name) = ProjectDetector::detect_with_name(path)
            .context("Failed to detect project type")?;

        // 创建元数据
        let meta = ProjectMeta::new(name, path.to_path_buf(), project_type);

        // 写入数据库
        self.index.insert(&meta)
            .context("Failed to insert project")?;

        // 更新缓存
        self.cache.lock().expect("cache mutex 中毒").insert(meta.id.clone(), meta.clone());

        tracing::info!("添加项目: {} ({:?})", meta.name, meta.path);
        Ok(meta)
    }

    /// 更新项目元数据（写穿缓存）
    pub fn update_project(&self, meta: &ProjectMeta) -> Result<()> {
        // 写入数据库
        self.index.update(meta)
            .context("Failed to update project")?;

        // 更新缓存
        self.cache.lock().expect("cache mutex 中毒").insert(meta.id.clone(), meta.clone());

        tracing::debug!("更新项目: {}", meta.id);
        Ok(())
    }

    /// 删除项目
    pub fn remove_project(&self, id: &str) -> Result<()> {
        // 从数据库删除
        self.index.delete(id)
            .context("Failed to delete project")?;

        // 清除缓存
        self.cache.lock().expect("cache mutex 中毒").remove(id);

        tracing::info!("删除项目: {}", id);
        Ok(())
    }

    /// 获取项目（缓存优先）
    pub fn get_project(&self, id: &str) -> Result<Option<ProjectMeta>> {
        // 先查缓存
        {
            let cache = self.cache.lock().expect("cache mutex 中毒");
            if let Some(meta) = cache.get(id) {
                return Ok(Some(meta.clone()));
            }
        }

        // 缓存未命中，查数据库
        let meta = self.index.get_by_id(id)?;

        // 更新缓存
        if let Some(ref m) = meta {
            self.cache.lock().expect("cache mutex 中毒").insert(id.to_string(), m.clone());
        }

        Ok(meta)
    }

    /// 打开项目（更新 last_opened_at）
    pub fn open_project(&self, id: &str) -> Result<ProjectMeta> {
        let mut meta = self.index.get_by_id(id)?
            .ok_or_else(|| anyhow::anyhow!("Project not found: {}", id))?;

        // 更新 last_opened_at
        meta.mark_opened();

        // 写入数据库
        self.index.update(&meta)
            .context("Failed to update last_opened_at")?;

        // 刷新缓存
        self.cache.lock().expect("cache mutex 中毒").insert(id.to_string(), meta.clone());

        tracing::info!("打开项目: {} ({})", meta.name, id);
        Ok(meta)
    }

    /// 列出所有项目
    pub fn list_projects(&self) -> Result<Vec<ProjectMeta>> {
        self.index.list_all()
    }

    /// 列出最近打开的项目
    pub fn list_recent(&self, limit: usize) -> Result<Vec<ProjectMeta>> {
        self.index.list_recent(limit)
    }

    /// 列出收藏项目
    pub fn list_favorites(&self) -> Result<Vec<ProjectMeta>> {
        self.index.list_favorites()
    }

    /// 切换收藏状态
    pub fn toggle_favorite(&self, id: &str) -> Result<()> {
        let mut meta = self.index.get_by_id(id)?
            .ok_or_else(|| anyhow::anyhow!("Project not found: {}", id))?;

        // 切换状态
        meta.toggle_favorite();

        // 写入数据库
        self.index.update(&meta)
            .context("Failed to update favorite status")?;

        // 刷新缓存
        self.cache.lock().expect("cache mutex 中毒").insert(id.to_string(), meta.clone());

        tracing::debug!("切换收藏状态: {} -> {}", id, meta.is_favorite);
        Ok(())
    }

    /// 搜索项目
    pub fn search_projects(&self, query: &str) -> Result<Vec<ProjectMeta>> {
        self.index.search(query)
    }

    /// 清空缓存（仅测试用）
    #[cfg(test)]
    pub fn clear_cache(&self) {
        self.cache.lock().expect("cache mutex 中毒").clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectType;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_manager() -> (TempDir, ProjectManager) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("projects.db");
        let manager = ProjectManager::new(&db_path).unwrap();
        (temp_dir, manager)
    }

    fn create_test_project_dir(_base: &Path, name: &str, content: &str) -> TempDir {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().join(name);
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(project_dir.join("package.json"), content).unwrap();
        temp
    }

    #[test]
    fn test_add_project() {
        let (_temp_db, manager) = create_test_manager();
        let project_temp = create_test_project_dir(
            Path::new("/tmp"),
            "test-app",
            r#"{"name": "test-app"}"#,
        );

        let project_path = project_temp.path().join("test-app");
        let meta = manager.add_project(&project_path).unwrap();

        assert_eq!(meta.name, "test-app");
        assert_eq!(meta.project_type, ProjectType::NodeJs);
        assert_eq!(meta.path, project_path);
    }

    #[test]
    fn test_add_duplicate_project() {
        let (_temp_db, manager) = create_test_manager();
        let project_temp = create_test_project_dir(
            Path::new("/tmp"),
            "test-app",
            r#"{"name": "test-app"}"#,
        );

        let project_path = project_temp.path().join("test-app");
        manager.add_project(&project_path).unwrap();

        let result = manager.add_project(&project_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("已存在"));
    }

    #[test]
    fn test_update_project() {
        let (_temp_db, manager) = create_test_manager();
        let project_temp = create_test_project_dir(
            Path::new("/tmp"),
            "test-app",
            r#"{"name": "test-app"}"#,
        );

        let project_path = project_temp.path().join("test-app");
        let mut meta = manager.add_project(&project_path).unwrap();

        meta.description = Some("Updated description".to_string());
        manager.update_project(&meta).unwrap();

        let retrieved = manager.get_project(&meta.id).unwrap().unwrap();
        assert_eq!(retrieved.description, Some("Updated description".to_string()));
    }

    #[test]
    fn test_remove_project() {
        let (_temp_db, manager) = create_test_manager();
        let project_temp = create_test_project_dir(
            Path::new("/tmp"),
            "test-app",
            r#"{"name": "test-app"}"#,
        );

        let project_path = project_temp.path().join("test-app");
        let meta = manager.add_project(&project_path).unwrap();

        manager.remove_project(&meta.id).unwrap();

        let result = manager.get_project(&meta.id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_project_cache_hit() {
        let (_temp_db, manager) = create_test_manager();
        let project_temp = create_test_project_dir(
            Path::new("/tmp"),
            "test-app",
            r#"{"name": "test-app"}"#,
        );

        let project_path = project_temp.path().join("test-app");
        let meta = manager.add_project(&project_path).unwrap();

        // 第二次 get 应该命中缓存
        let cached = manager.get_project(&meta.id).unwrap().unwrap();
        assert_eq!(cached.id, meta.id);
        assert_eq!(cached.name, "test-app");
    }

    #[test]
    fn test_get_project_cache_miss() {
        let (_temp_db, manager) = create_test_manager();
        let project_temp = create_test_project_dir(
            Path::new("/tmp"),
            "test-app",
            r#"{"name": "test-app"}"#,
        );

        let project_path = project_temp.path().join("test-app");
        let meta = manager.add_project(&project_path).unwrap();

        // 清空缓存
        manager.clear_cache();

        // 重新获取（从数据库）
        let retrieved = manager.get_project(&meta.id).unwrap().unwrap();
        assert_eq!(retrieved.id, meta.id);
        assert_eq!(retrieved.name, "test-app");
    }

    #[test]
    fn test_open_project() {
        let (_temp_db, manager) = create_test_manager();
        let project_temp = create_test_project_dir(
            Path::new("/tmp"),
            "test-app",
            r#"{"name": "test-app"}"#,
        );

        let project_path = project_temp.path().join("test-app");
        let meta = manager.add_project(&project_path).unwrap();

        assert!(meta.last_opened_at.is_none());

        let opened = manager.open_project(&meta.id).unwrap();
        assert!(opened.last_opened_at.is_some());
    }

    #[test]
    fn test_toggle_favorite() {
        let (_temp_db, manager) = create_test_manager();
        let project_temp = create_test_project_dir(
            Path::new("/tmp"),
            "test-app",
            r#"{"name": "test-app"}"#,
        );

        let project_path = project_temp.path().join("test-app");
        let meta = manager.add_project(&project_path).unwrap();

        assert!(!meta.is_favorite);

        manager.toggle_favorite(&meta.id).unwrap();
        let updated = manager.get_project(&meta.id).unwrap().unwrap();
        assert!(updated.is_favorite);

        manager.toggle_favorite(&meta.id).unwrap();
        let updated2 = manager.get_project(&meta.id).unwrap().unwrap();
        assert!(!updated2.is_favorite);
    }

    #[test]
    fn test_list_projects() {
        let (_temp_db, manager) = create_test_manager();

        let temp1 = create_test_project_dir(
            Path::new("/tmp"),
            "app1",
            r#"{"name": "app1"}"#,
        );
        let temp2 = create_test_project_dir(
            Path::new("/tmp"),
            "app2",
            r#"{"name": "app2"}"#,
        );

        manager.add_project(&temp1.path().join("app1")).unwrap();
        manager.add_project(&temp2.path().join("app2")).unwrap();

        let projects = manager.list_projects().unwrap();
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn test_list_recent() {
        let (_temp_db, manager) = create_test_manager();

        let temp1 = create_test_project_dir(
            Path::new("/tmp"),
            "app1",
            r#"{"name": "app1"}"#,
        );
        let temp2 = create_test_project_dir(
            Path::new("/tmp"),
            "app2",
            r#"{"name": "app2"}"#,
        );

        let meta1 = manager.add_project(&temp1.path().join("app1")).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let meta2 = manager.add_project(&temp2.path().join("app2")).unwrap();

        // 先打开 app1
        let opened1 = manager.open_project(&meta1.id).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));

        // 后打开 app2
        let opened2 = manager.open_project(&meta2.id).unwrap();

        // 验证时间戳
        assert!(opened2.last_opened_at.unwrap() > opened1.last_opened_at.unwrap());

        let recent = manager.list_recent(10).unwrap();
        assert_eq!(recent.len(), 2);

        // 最近打开的在前（meta2 后打开，应该在 [0]）
        assert_eq!(recent[0].id, meta2.id);
        assert_eq!(recent[1].id, meta1.id);
    }

    #[test]
    fn test_search_projects() {
        let (_temp_db, manager) = create_test_manager();

        let temp1 = create_test_project_dir(
            Path::new("/tmp"),
            "my-app",
            r#"{"name": "my-app"}"#,
        );
        let temp2 = create_test_project_dir(
            Path::new("/tmp"),
            "another-project",
            r#"{"name": "another-project"}"#,
        );

        manager.add_project(&temp1.path().join("my-app")).unwrap();
        manager.add_project(&temp2.path().join("another-project")).unwrap();

        let results = manager.search_projects("app").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "my-app");
    }

    #[tokio::test]
    async fn test_scan_and_add() {
        let (_temp_db, manager) = create_test_manager();
        let scan_root = TempDir::new().unwrap();

        // 创建多个项目
        let p1 = scan_root.path().join("project1");
        fs::create_dir_all(&p1).unwrap();
        fs::write(p1.join("package.json"), r#"{"name": "proj1"}"#).unwrap();

        let p2 = scan_root.path().join("project2");
        fs::create_dir_all(&p2).unwrap();
        fs::write(p2.join("Cargo.toml"), "[package]\nname = \"proj2\"").unwrap();

        let added = manager.scan_and_add(scan_root.path()).await.unwrap();
        assert_eq!(added.len(), 2);

        let projects = manager.list_projects().unwrap();
        assert_eq!(projects.len(), 2);
    }

    #[tokio::test]
    async fn test_scan_and_add_skip_existing() {
        let (_temp_db, manager) = create_test_manager();
        let scan_root = TempDir::new().unwrap();

        let p1 = scan_root.path().join("project1");
        fs::create_dir_all(&p1).unwrap();
        fs::write(p1.join("package.json"), r#"{"name": "proj1"}"#).unwrap();

        // 第一次扫描
        let added1 = manager.scan_and_add(scan_root.path()).await.unwrap();
        assert_eq!(added1.len(), 1);

        // 第二次扫描（应该跳过已存在项目）
        let added2 = manager.scan_and_add(scan_root.path()).await.unwrap();
        assert_eq!(added2.len(), 0);

        let projects = manager.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
    }
}
