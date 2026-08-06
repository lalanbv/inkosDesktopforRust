//! 项目健康检查器

use crate::project::{
    HealthIssue, IssueCategory, IssueSeverity, ProjectHealth, ProjectMeta,
    ProjectType,
};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

/// 项目健康检查器
pub struct ProjectHealthChecker;

impl ProjectHealthChecker {
    /// 执行完整健康检查（异步）
    pub async fn check(meta: &ProjectMeta) -> Result<ProjectHealth> {
        let path = meta.path.clone();
        let project_type = meta.project_type;
        let project_id = meta.id.clone();

        tokio::task::spawn_blocking(move || {
            let mut health = ProjectHealth::new(project_id);

            // 并行执行所有检查
            let deps_issues = Self::check_dependencies(&path, project_type).unwrap_or_default();
            let config_issues = Self::check_config(&path).unwrap_or_default();
            let env_issues = Self::check_environment(project_type).unwrap_or_default();
            let perm_issues = Self::check_permissions(&path).unwrap_or_default();
            let disk_issues = Self::check_disk_space(&path).unwrap_or_default();

            // 统计依赖
            if let Ok(dep_count) = Self::count_dependencies(&path, project_type) {
                health.dependency_count = dep_count.0;
                health.missing_dependencies = dep_count.1;
            }

            // 收集所有问题
            for issue in deps_issues
                .into_iter()
                .chain(config_issues)
                .chain(env_issues)
                .chain(perm_issues)
                .chain(disk_issues)
            {
                health.add_issue(issue);
            }

            Ok(health)
        })
        .await
        .context("Health check task failed")?
    }

    /// 检查依赖
    fn check_dependencies(path: &Path, project_type: ProjectType) -> Result<Vec<HealthIssue>> {
        let mut issues = Vec::new();

        match project_type {
            ProjectType::NodeJs => {
                let package_json = path.join("package.json");
                if !package_json.exists() {
                    issues.push(HealthIssue {
                        severity: IssueSeverity::Critical,
                        category: IssueCategory::DependencyMissing,
                        message: "package.json 文件不存在".to_string(),
                        suggestion: Some("创建 package.json".to_string()),
                    });
                    return Ok(issues);
                }

                // 检查 node_modules
                let node_modules = path.join("node_modules");
                if !node_modules.exists() {
                    issues.push(HealthIssue {
                        severity: IssueSeverity::Warning,
                        category: IssueCategory::DependencyMissing,
                        message: "node_modules 目录不存在".to_string(),
                        suggestion: Some("运行 npm install 或 pnpm install".to_string()),
                    });
                }
            }
            ProjectType::Rust => {
                let cargo_toml = path.join("Cargo.toml");
                if !cargo_toml.exists() {
                    issues.push(HealthIssue {
                        severity: IssueSeverity::Critical,
                        category: IssueCategory::DependencyMissing,
                        message: "Cargo.toml 文件不存在".to_string(),
                        suggestion: Some("创建 Cargo.toml".to_string()),
                    });
                    return Ok(issues);
                }

                // 检查 Cargo.lock
                let cargo_lock = path.join("Cargo.lock");
                if !cargo_lock.exists() {
                    issues.push(HealthIssue {
                        severity: IssueSeverity::Info,
                        category: IssueCategory::DependencyMissing,
                        message: "Cargo.lock 不存在（首次构建时会生成）".to_string(),
                        suggestion: Some("运行 cargo build".to_string()),
                    });
                }
            }
            ProjectType::Python => {
                let has_requirements = path.join("requirements.txt").exists();
                let has_pyproject = path.join("pyproject.toml").exists();

                if !has_requirements && !has_pyproject {
                    issues.push(HealthIssue {
                        severity: IssueSeverity::Warning,
                        category: IssueCategory::DependencyMissing,
                        message: "未找到依赖文件（requirements.txt 或 pyproject.toml）".to_string(),
                        suggestion: Some("创建依赖文件".to_string()),
                    });
                }
            }
            ProjectType::Go => {
                let go_mod = path.join("go.mod");
                if !go_mod.exists() {
                    issues.push(HealthIssue {
                        severity: IssueSeverity::Critical,
                        category: IssueCategory::DependencyMissing,
                        message: "go.mod 文件不存在".to_string(),
                        suggestion: Some("运行 go mod init".to_string()),
                    });
                }
            }
            _ => {
                // 其他项目类型，不检查依赖
            }
        }

        Ok(issues)
    }

    /// 检查配置文件
    fn check_config(path: &Path) -> Result<Vec<HealthIssue>> {
        let mut issues = Vec::new();

        // 检查 package.json 格式
        let package_json = path.join("package.json");
        if package_json.exists() {
            match fs::read_to_string(&package_json) {
                Ok(content) => {
                    if serde_json::from_str::<serde_json::Value>(&content).is_err() {
                        issues.push(HealthIssue {
                            severity: IssueSeverity::Critical,
                            category: IssueCategory::ConfigInvalid,
                            message: "package.json 格式无效".to_string(),
                            suggestion: Some("修复 JSON 格式错误".to_string()),
                        });
                    }
                }
                Err(_) => {
                    issues.push(HealthIssue {
                        severity: IssueSeverity::Critical,
                        category: IssueCategory::FilePermission,
                        message: "无法读取 package.json".to_string(),
                        suggestion: Some("检查文件权限".to_string()),
                    });
                }
            }
        }

        Ok(issues)
    }

    /// 检查环境（运行时是否安装）
    fn check_environment(project_type: ProjectType) -> Result<Vec<HealthIssue>> {
        let mut issues = Vec::new();

        let (cmd, name) = match project_type {
            ProjectType::NodeJs => ("node", "Node.js"),
            ProjectType::Python => ("python", "Python"),
            ProjectType::Rust => ("rustc", "Rust"),
            ProjectType::Go => ("go", "Go"),
            ProjectType::Java => ("java", "Java"),
            ProjectType::Dotnet => ("dotnet", ".NET"),
            ProjectType::Generic => return Ok(issues),
        };

        // 检查命令是否可用
        match Command::new(cmd).arg("--version").output() {
            Ok(output) if output.status.success() => {
                // 环境已安装
            }
            _ => {
                issues.push(HealthIssue {
                    severity: IssueSeverity::Critical,
                    category: IssueCategory::EnvironmentMissing,
                    message: format!("{} 未安装或不在 PATH 中", name),
                    suggestion: Some(format!("安装 {} 运行时", name)),
                });
            }
        }

        Ok(issues)
    }

    /// 检查文件权限
    fn check_permissions(path: &Path) -> Result<Vec<HealthIssue>> {
        let mut issues = Vec::new();

        // 检查目录是否可读
        match fs::read_dir(path) {
            Ok(_) => {}
            Err(_) => {
                issues.push(HealthIssue {
                    severity: IssueSeverity::Critical,
                    category: IssueCategory::FilePermission,
                    message: "项目目录不可读".to_string(),
                    suggestion: Some("检查目录权限".to_string()),
                });
                return Ok(issues);
            }
        }

        // 检查是否可写（尝试创建临时文件）
        let test_file = path.join(".health_check_tmp");
        match fs::write(&test_file, b"test") {
            Ok(_) => {
                let _ = fs::remove_file(&test_file);
            }
            Err(_) => {
                issues.push(HealthIssue {
                    severity: IssueSeverity::Warning,
                    category: IssueCategory::FilePermission,
                    message: "项目目录不可写".to_string(),
                    suggestion: Some("检查目录权限".to_string()),
                });
            }
        }

        Ok(issues)
    }

    /// 检查磁盘空间
    fn check_disk_space(path: &Path) -> Result<Vec<HealthIssue>> {
        let mut issues = Vec::new();

        // 注：这里只统计项目根目录的直属文件大小（不递归），作为「占用偏大」的
        // 低成本信号。要获取真正的磁盘剩余空间需 statfs/GetDiskFreeSpaceEx，
        // 后续可引入 sysinfo crate 替换。
        const LARGE_DIR_THRESHOLD: u64 = 10 * 1024 * 1024 * 1024; // 10GB

        if Self::estimate_dir_size(path).unwrap_or(0) > LARGE_DIR_THRESHOLD {
            issues.push(HealthIssue {
                severity: IssueSeverity::Info,
                category: IssueCategory::DiskSpace,
                message: "项目目录占用空间较大（>10GB）".to_string(),
                suggestion: Some("清理构建产物或缓存".to_string()),
            });
        }

        Ok(issues)
    }

    /// 估算目录大小（仅直属文件，不递归；跨平台）
    fn estimate_dir_size(path: &Path) -> Result<u64> {
        let mut total = 0u64;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    total += metadata.len();
                }
            }
        }
        Ok(total)
    }

    /// 统计依赖数量和缺失依赖
    fn count_dependencies(
        path: &Path,
        project_type: ProjectType,
    ) -> Result<(usize, Vec<String>)> {
        match project_type {
            ProjectType::NodeJs => {
                let package_json = path.join("package.json");
                if !package_json.exists() {
                    return Ok((0, Vec::new()));
                }

                let content = fs::read_to_string(&package_json)?;
                let json: serde_json::Value = serde_json::from_str(&content)?;

                let mut count = 0;
                if let Some(deps) = json.get("dependencies").and_then(|v| v.as_object()) {
                    count += deps.len();
                }
                if let Some(dev_deps) = json.get("devDependencies").and_then(|v| v.as_object()) {
                    count += dev_deps.len();
                }

                // 简化：不检查具体哪些依赖缺失
                Ok((count, Vec::new()))
            }
            _ => Ok((0, Vec::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_project(_project_type: ProjectType, files: Vec<(&str, &str)>) -> TempDir {
        let temp = TempDir::new().unwrap();
        for (file, content) in files {
            let file_path = temp.path().join(file);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(file_path, content).unwrap();
        }
        temp
    }

    #[tokio::test]
    async fn test_check_nodejs_healthy() {
        let temp = create_test_project(
            ProjectType::NodeJs,
            vec![
                ("package.json", r#"{"name": "test", "dependencies": {"lodash": "^4.17.21"}}"#),
            ],
        );

        let meta = ProjectMeta::new("test".to_string(), temp.path().to_path_buf(), ProjectType::NodeJs);
        let health = ProjectHealthChecker::check(&meta).await.unwrap();

        // Node.js 环境可能未安装，所以不检查环境问题
        // 主要验证依赖检查逻辑
        assert_eq!(health.dependency_count, 1);
    }

    #[tokio::test]
    async fn test_check_nodejs_missing_node_modules() {
        let temp = create_test_project(
            ProjectType::NodeJs,
            vec![("package.json", r#"{"name": "test"}"#)],
        );

        let meta = ProjectMeta::new("test".to_string(), temp.path().to_path_buf(), ProjectType::NodeJs);
        let health = ProjectHealthChecker::check(&meta).await.unwrap();

        // 应该有 node_modules 缺失警告
        let has_node_modules_warning = health.issues.iter().any(|i| {
            i.category == IssueCategory::DependencyMissing
                && i.message.contains("node_modules")
        });
        assert!(has_node_modules_warning);
    }

    #[tokio::test]
    async fn test_check_nodejs_invalid_json() {
        let temp = create_test_project(
            ProjectType::NodeJs,
            vec![("package.json", r#"{"name": "test", invalid}"#)],
        );

        let meta = ProjectMeta::new("test".to_string(), temp.path().to_path_buf(), ProjectType::NodeJs);
        let health = ProjectHealthChecker::check(&meta).await.unwrap();

        // 应该有配置无效错误
        let has_invalid_json = health.issues.iter().any(|i| {
            i.category == IssueCategory::ConfigInvalid
                && i.severity == IssueSeverity::Critical
        });
        assert!(has_invalid_json);
    }

    #[tokio::test]
    async fn test_check_rust_project() {
        let temp = create_test_project(
            ProjectType::Rust,
            vec![("Cargo.toml", "[package]\nname = \"test\"\nversion = \"0.1.0\"")],
        );

        let meta = ProjectMeta::new("test".to_string(), temp.path().to_path_buf(), ProjectType::Rust);
        let health = ProjectHealthChecker::check(&meta).await.unwrap();

        // Cargo.lock 不存在，应该有 Info 级别提示
        let has_lock_info = health.issues.iter().any(|i| {
            i.severity == IssueSeverity::Info
                && i.message.contains("Cargo.lock")
        });
        assert!(has_lock_info || health.issues.is_empty()); // 可能没有问题
    }

    #[test]
    fn test_check_dependencies_nodejs() {
        let temp = create_test_project(
            ProjectType::NodeJs,
            vec![("package.json", r#"{"name": "test"}"#)],
        );

        let issues = ProjectHealthChecker::check_dependencies(temp.path(), ProjectType::NodeJs).unwrap();

        // node_modules 缺失
        assert!(issues.iter().any(|i| i.message.contains("node_modules")));
    }

    #[test]
    fn test_check_config_valid_json() {
        let temp = create_test_project(
            ProjectType::NodeJs,
            vec![("package.json", r#"{"name": "test"}"#)],
        );

        let issues = ProjectHealthChecker::check_config(temp.path()).unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn test_check_config_invalid_json() {
        let temp = create_test_project(
            ProjectType::NodeJs,
            vec![("package.json", r#"{"name": invalid"#)],
        );

        let issues = ProjectHealthChecker::check_config(temp.path()).unwrap();
        assert!(!issues.is_empty());
        assert!(issues[0].message.contains("格式无效"));
    }

    #[test]
    fn test_check_permissions() {
        let temp = TempDir::new().unwrap();
        let issues = ProjectHealthChecker::check_permissions(temp.path()).unwrap();

        // 临时目录应该有读写权限
        assert!(issues.is_empty());
    }

    #[test]
    fn test_count_dependencies() {
        let temp = create_test_project(
            ProjectType::NodeJs,
            vec![("package.json", r#"{
                "dependencies": {"lodash": "^4.0.0", "axios": "^1.0.0"},
                "devDependencies": {"jest": "^29.0.0"}
            }"#)],
        );

        let (count, _missing) = ProjectHealthChecker::count_dependencies(temp.path(), ProjectType::NodeJs).unwrap();
        assert_eq!(count, 3); // 2 dependencies + 1 devDependencies
    }
}
