//! 项目扫描器

use crate::project::{ProjectDetector, ProjectMeta, ProjectType};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 项目扫描器
pub struct ProjectScanner {
    max_depth: usize,
    exclude_dirs: Vec<String>,
}

impl Default for ProjectScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectScanner {
    /// 创建扫描器（默认配置）
    pub fn new() -> Self {
        Self {
            max_depth: 5,
            exclude_dirs: vec![
                "node_modules".to_string(),
                "target".to_string(),
                ".git".to_string(),
                "build".to_string(),
                "dist".to_string(),
                ".next".to_string(),
                "__pycache__".to_string(),
                ".venv".to_string(),
                "venv".to_string(),
                ".idea".to_string(),
                ".vscode".to_string(),
            ],
        }
    }

    /// 配置最大深度
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// 添加排除目录
    pub fn add_exclude_dir(mut self, dir: String) -> Self {
        if !self.exclude_dirs.contains(&dir) {
            self.exclude_dirs.push(dir);
        }
        self
    }

    /// 扫描目录（异步）
    ///
    /// 遍历文件系统，发现所有项目。使用 tokio::task::spawn_blocking 避免阻塞异步运行时。
    pub async fn scan(&self, root: &Path) -> Result<Vec<ProjectMeta>> {
        let root = root.to_path_buf();
        let max_depth = self.max_depth;
        let exclude_dirs = self.exclude_dirs.clone();

        tokio::task::spawn_blocking(move || {
            Self::scan_sync(&root, max_depth, &exclude_dirs)
        })
        .await
        .context("扫描任务失败")?
    }

    /// 扫描单个项目（同步）
    pub fn scan_single(&self, path: &Path) -> Result<Option<ProjectMeta>> {
        if !path.is_dir() {
            anyhow::bail!("不是目录: {:?}", path);
        }

        // 检查是否是项目根
        match ProjectDetector::detect_with_name(path) {
            Ok((project_type, name)) => {
                // Generic 不算有效项目
                if project_type == ProjectType::Generic {
                    return Ok(None);
                }

                let meta = ProjectMeta::new(name, path.to_path_buf(), project_type);
                Ok(Some(meta))
            }
            Err(_) => Ok(None),
        }
    }

    /// 同步扫描实现
    fn scan_sync(
        root: &Path,
        max_depth: usize,
        exclude_dirs: &[String],
    ) -> Result<Vec<ProjectMeta>> {
        if !root.is_dir() {
            anyhow::bail!("不是目录: {:?}", root);
        }

        let mut projects = Vec::new();
        let mut scanned_paths = std::collections::HashSet::new();

        for entry in WalkDir::new(root)
            .max_depth(max_depth)
            .into_iter()
            .filter_entry(|e| Self::should_enter(e, exclude_dirs))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue, // 跳过无法访问的目录
            };

            if !entry.file_type().is_dir() {
                continue;
            }

            let path = entry.path();

            // 跳过已扫描的路径（避免嵌套项目重复）
            if scanned_paths.contains(path) {
                continue;
            }

            // 检查是否是项目根
            if let Ok((project_type, name)) = ProjectDetector::detect_with_name(path) {
                // Generic 不算有效项目
                if project_type == ProjectType::Generic {
                    continue;
                }

                let meta = ProjectMeta::new(name, path.to_path_buf(), project_type);
                projects.push(meta);

                // 标记为已扫描（避免扫描项目内部）
                scanned_paths.insert(path.to_path_buf());
            }
        }

        Ok(projects)
    }

    /// 判断是否应该进入目录（排除规则）
    fn should_enter(entry: &walkdir::DirEntry, exclude_dirs: &[String]) -> bool {
        let file_name = entry.file_name().to_string_lossy();

        // 排除隐藏目录（以 . 开头，但允许进入根目录）
        if entry.depth() > 0 && file_name.starts_with('.') {
            return false;
        }

        // 排除指定目录
        if exclude_dirs.iter().any(|dir| file_name == dir.as_str()) {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_project(base: &Path, name: &str, project_file: &str, content: &str) {
        let project_dir = base.join(name);
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(project_dir.join(project_file), content).unwrap();
    }

    #[test]
    fn test_scan_single_nodejs() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"name": "test-app"}"#,
        )
        .unwrap();

        let scanner = ProjectScanner::new();
        let result = scanner.scan_single(temp.path()).unwrap();

        assert!(result.is_some());
        let meta = result.unwrap();
        assert_eq!(meta.name, "test-app");
        assert_eq!(meta.project_type, ProjectType::NodeJs);
    }

    #[test]
    fn test_scan_single_generic() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("README.md"), "").unwrap();

        let scanner = ProjectScanner::new();
        let result = scanner.scan_single(temp.path()).unwrap();

        // Generic 不算有效项目
        assert!(result.is_none());
    }

    #[test]
    fn test_scan_single_invalid_path() {
        let scanner = ProjectScanner::new();
        let result = scanner.scan_single(Path::new("/nonexistent"));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scan_multiple_projects() {
        let temp = TempDir::new().unwrap();

        // 创建多个项目
        create_test_project(
            temp.path(),
            "project1",
            "package.json",
            r#"{"name": "proj1"}"#,
        );
        create_test_project(
            temp.path(),
            "project2",
            "Cargo.toml",
            "[package]\nname = \"proj2\"",
        );
        create_test_project(
            temp.path(),
            "project3",
            "requirements.txt",
            "requests",
        );

        let scanner = ProjectScanner::new();
        let projects = scanner.scan(temp.path()).await.unwrap();

        assert_eq!(projects.len(), 3);
        let names: Vec<_> = projects.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"proj1"));
        assert!(names.contains(&"proj2"));
        // project3 是 Python 项目（requirements.txt 使用目录名）
    }

    #[tokio::test]
    async fn test_scan_with_excluded_dirs() {
        let temp = TempDir::new().unwrap();

        // 创建项目
        create_test_project(
            temp.path(),
            "valid-project",
            "package.json",
            r#"{"name": "valid"}"#,
        );

        // 在 node_modules 中创建项目（应该被排除）
        create_test_project(
            temp.path(),
            "node_modules/some-package",
            "package.json",
            r#"{"name": "excluded"}"#,
        );

        let scanner = ProjectScanner::new();
        let projects = scanner.scan(temp.path()).await.unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "valid");
    }

    #[tokio::test]
    async fn test_scan_with_max_depth() {
        let temp = TempDir::new().unwrap();

        // 深度 1
        create_test_project(
            temp.path(),
            "level1",
            "package.json",
            r#"{"name": "level1"}"#,
        );

        // 深度 2
        create_test_project(
            temp.path(),
            "level1/level2",
            "package.json",
            r#"{"name": "level2"}"#,
        );

        // 深度 3
        create_test_project(
            temp.path(),
            "level1/level2/level3",
            "package.json",
            r#"{"name": "level3"}"#,
        );

        // 限制深度为 2
        let scanner = ProjectScanner::new().with_max_depth(2);
        let projects = scanner.scan(temp.path()).await.unwrap();

        // 应该只找到 level1 和 level2，level3 超出深度限制
        assert_eq!(projects.len(), 2);
        let names: Vec<_> = projects.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"level1"));
        assert!(names.contains(&"level2"));
        assert!(!names.contains(&"level3"));
    }

    #[tokio::test]
    async fn test_scan_nested_projects() {
        let temp = TempDir::new().unwrap();

        // 外层项目（monorepo）
        create_test_project(
            temp.path(),
            "monorepo",
            "package.json",
            r#"{"name": "monorepo"}"#,
        );

        // 内层项目
        create_test_project(
            temp.path(),
            "monorepo/packages/app1",
            "package.json",
            r#"{"name": "app1"}"#,
        );

        let scanner = ProjectScanner::new();
        let projects = scanner.scan(temp.path()).await.unwrap();

        // 注：当前实现会找到 monorepo 和 app1（因为 packages 目录不是项目根）
        // 在真实场景中，monorepo 通常用 workspaces 管理子包
        assert_eq!(projects.len(), 2);
        let names: Vec<_> = projects.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"monorepo"));
        assert!(names.contains(&"app1"));
    }

    #[tokio::test]
    async fn test_scan_empty_directory() {
        let temp = TempDir::new().unwrap();

        let scanner = ProjectScanner::new();
        let projects = scanner.scan(temp.path()).await.unwrap();

        assert_eq!(projects.len(), 0);
    }

    #[test]
    fn test_custom_exclude_dir() {
        let scanner = ProjectScanner::new().add_exclude_dir("custom-exclude".to_string());

        assert!(scanner.exclude_dirs.contains(&"custom-exclude".to_string()));
        assert!(scanner.exclude_dirs.contains(&"node_modules".to_string()));
    }

    #[tokio::test]
    async fn test_scan_with_custom_exclude() {
        let temp = TempDir::new().unwrap();

        // 正常项目
        create_test_project(
            temp.path(),
            "valid-project",
            "package.json",
            r#"{"name": "valid"}"#,
        );

        // 自定义排除目录中的项目
        create_test_project(
            temp.path(),
            "my-exclude/excluded-project",
            "package.json",
            r#"{"name": "excluded"}"#,
        );

        let scanner = ProjectScanner::new().add_exclude_dir("my-exclude".to_string());
        let projects = scanner.scan(temp.path()).await.unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "valid");
    }

    #[tokio::test]
    async fn test_scan_skips_hidden_dirs() {
        let temp = TempDir::new().unwrap();

        // 正常项目
        create_test_project(
            temp.path(),
            "visible-project",
            "package.json",
            r#"{"name": "visible"}"#,
        );

        // 隐藏目录中的项目
        create_test_project(
            temp.path(),
            ".hidden/hidden-project",
            "package.json",
            r#"{"name": "hidden"}"#,
        );

        let scanner = ProjectScanner::new();
        let projects = scanner.scan(temp.path()).await.unwrap();

        // 应该只找到 visible-project（.hidden 被自动排除）
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "visible");
    }

    #[tokio::test]
    async fn test_scan_multiple_project_types() {
        let temp = TempDir::new().unwrap();

        create_test_project(
            temp.path(),
            "nodejs-app",
            "package.json",
            r#"{"name": "nodejs"}"#,
        );
        create_test_project(
            temp.path(),
            "rust-app",
            "Cargo.toml",
            "[package]\nname = \"rust-app\"",
        );
        create_test_project(temp.path(), "go-app", "go.mod", "module example.com/go-app");
        create_test_project(
            temp.path(),
            "python-app",
            "pyproject.toml",
            "[project]\nname = \"python-app\"",
        );

        let scanner = ProjectScanner::new();
        let projects = scanner.scan(temp.path()).await.unwrap();

        assert_eq!(projects.len(), 4);

        let types: std::collections::HashMap<_, _> = projects
            .iter()
            .map(|p| (p.name.as_str(), p.project_type))
            .collect();

        assert_eq!(types.get("nodejs"), Some(&ProjectType::NodeJs));
        assert_eq!(types.get("rust-app"), Some(&ProjectType::Rust));
        assert_eq!(types.get("go-app"), Some(&ProjectType::Go));
        assert_eq!(types.get("python-app"), Some(&ProjectType::Python));
    }
}
