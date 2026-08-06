//! 项目管理核心类型定义

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 项目类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectType {
    /// Node.js 项目（package.json）
    NodeJs,
    /// Python 项目（requirements.txt / pyproject.toml / setup.py）
    Python,
    /// Rust 项目（Cargo.toml）
    Rust,
    /// Go 项目（go.mod）
    Go,
    /// Java 项目（pom.xml / build.gradle）
    Java,
    /// .NET 项目（*.csproj / *.sln）
    Dotnet,
    /// 通用项目（无特征文件）
    Generic,
}

impl ProjectType {
    /// 转换为字符串（用于数据库存储）
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NodeJs => "nodejs",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Go => "go",
            Self::Java => "java",
            Self::Dotnet => "dotnet",
            Self::Generic => "generic",
        }
    }

    /// 从字符串解析（用于数据库读取）
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "nodejs" => Some(Self::NodeJs),
            "python" => Some(Self::Python),
            "rust" => Some(Self::Rust),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "dotnet" => Some(Self::Dotnet),
            "generic" => Some(Self::Generic),
            _ => None,
        }
    }
}

/// 项目元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    /// 项目 ID（UUID v4）
    pub id: String,
    /// 项目名称（目录名或自定义）
    pub name: String,
    /// 项目根目录绝对路径
    pub path: PathBuf,
    /// 项目类型
    pub project_type: ProjectType,
    /// 所属工作区 ID
    pub workspace_id: Option<String>,
    /// 创建时间（Unix 时间戳，秒）
    pub created_at: i64,
    /// 最后打开时间（Unix 时间戳，秒）
    pub last_opened_at: Option<i64>,
    /// 最后扫描时间（Unix 时间戳，秒）
    pub last_scanned_at: Option<i64>,
    /// 是否收藏
    pub is_favorite: bool,
    /// 标签（用户自定义）
    pub tags: Vec<String>,
    /// 项目描述
    pub description: Option<String>,
}

impl ProjectMeta {
    /// 创建新项目元数据
    pub fn new(name: String, path: PathBuf, project_type: ProjectType) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            path,
            project_type,
            workspace_id: None,
            created_at: now,
            last_opened_at: None,
            last_scanned_at: Some(now),
            is_favorite: false,
            tags: Vec::new(),
            description: None,
        }
    }

    /// 标记为已打开（更新 last_opened_at）
    pub fn mark_opened(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        self.last_opened_at = Some(now);
    }

    /// 切换收藏状态
    pub fn toggle_favorite(&mut self) {
        self.is_favorite = !self.is_favorite;
    }
}

/// 健康检查状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// 健康（无问题）
    Healthy,
    /// 警告（有轻微问题）
    Warning,
    /// 严重（有严重问题）
    Critical,
    /// 未知（未检查）
    Unknown,
}

impl HealthStatus {
    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Warning => "warning",
            Self::Critical => "critical",
            Self::Unknown => "unknown",
        }
    }

    /// 从字符串解析
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "healthy" => Some(Self::Healthy),
            "warning" => Some(Self::Warning),
            "critical" => Some(Self::Critical),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// 问题严重程度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    /// 信息
    Info,
    /// 警告
    Warning,
    /// 严重
    Critical,
}

/// 问题类别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueCategory {
    /// 依赖缺失
    DependencyMissing,
    /// 配置无效
    ConfigInvalid,
    /// 环境缺失
    EnvironmentMissing,
    /// 文件权限问题
    FilePermission,
    /// 磁盘空间不足
    DiskSpace,
}

/// 健康问题
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthIssue {
    /// 严重程度
    pub severity: IssueSeverity,
    /// 问题类别
    pub category: IssueCategory,
    /// 问题描述
    pub message: String,
    /// 修复建议
    pub suggestion: Option<String>,
}

/// 项目健康检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectHealth {
    /// 项目 ID
    pub project_id: String,
    /// 检查时间（Unix 时间戳，秒）
    pub checked_at: i64,
    /// 整体健康状态
    pub status: HealthStatus,
    /// 问题列表
    pub issues: Vec<HealthIssue>,
    /// 依赖总数
    pub dependency_count: usize,
    /// 缺失依赖列表
    pub missing_dependencies: Vec<String>,
}

impl ProjectHealth {
    /// 创建新的健康检查结果
    pub fn new(project_id: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        Self {
            project_id,
            checked_at: now,
            status: HealthStatus::Unknown,
            issues: Vec::new(),
            dependency_count: 0,
            missing_dependencies: Vec::new(),
        }
    }

    /// 添加问题
    pub fn add_issue(&mut self, issue: HealthIssue) {
        self.issues.push(issue);
        self.update_status();
    }

    /// 根据问题更新整体状态
    fn update_status(&mut self) {
        if self.issues.is_empty() {
            self.status = HealthStatus::Healthy;
            return;
        }

        let has_critical = self
            .issues
            .iter()
            .any(|i| matches!(i.severity, IssueSeverity::Critical));

        if has_critical {
            self.status = HealthStatus::Critical;
        } else {
            self.status = HealthStatus::Warning;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_type_serialization() {
        assert_eq!(ProjectType::NodeJs.as_str(), "nodejs");
        assert_eq!(ProjectType::Python.as_str(), "python");
        assert_eq!(ProjectType::Rust.as_str(), "rust");

        assert_eq!(ProjectType::from_str("nodejs"), Some(ProjectType::NodeJs));
        assert_eq!(ProjectType::from_str("python"), Some(ProjectType::Python));
        assert_eq!(ProjectType::from_str("invalid"), None);
    }

    #[test]
    fn test_project_meta_creation() {
        let meta = ProjectMeta::new(
            "test-project".to_string(),
            PathBuf::from("/path/to/project"),
            ProjectType::NodeJs,
        );

        assert!(!meta.id.is_empty());
        assert_eq!(meta.name, "test-project");
        assert_eq!(meta.path, PathBuf::from("/path/to/project"));
        assert_eq!(meta.project_type, ProjectType::NodeJs);
        assert!(meta.workspace_id.is_none());
        assert!(meta.last_opened_at.is_none());
        assert!(meta.last_scanned_at.is_some());
        assert!(!meta.is_favorite);
        assert!(meta.tags.is_empty());
    }

    #[test]
    fn test_project_meta_mark_opened() {
        let mut meta = ProjectMeta::new(
            "test".to_string(),
            PathBuf::from("/test"),
            ProjectType::Generic,
        );

        assert!(meta.last_opened_at.is_none());
        meta.mark_opened();
        assert!(meta.last_opened_at.is_some());
    }

    #[test]
    fn test_project_meta_toggle_favorite() {
        let mut meta = ProjectMeta::new(
            "test".to_string(),
            PathBuf::from("/test"),
            ProjectType::Generic,
        );

        assert!(!meta.is_favorite);
        meta.toggle_favorite();
        assert!(meta.is_favorite);
        meta.toggle_favorite();
        assert!(!meta.is_favorite);
    }

    #[test]
    fn test_health_status_serialization() {
        assert_eq!(HealthStatus::Healthy.as_str(), "healthy");
        assert_eq!(HealthStatus::Warning.as_str(), "warning");
        assert_eq!(HealthStatus::Critical.as_str(), "critical");

        assert_eq!(
            HealthStatus::from_str("healthy"),
            Some(HealthStatus::Healthy)
        );
        assert_eq!(
            HealthStatus::from_str("warning"),
            Some(HealthStatus::Warning)
        );
        assert_eq!(HealthStatus::from_str("invalid"), None);
    }

    #[test]
    fn test_project_health_creation() {
        let health = ProjectHealth::new("proj-123".to_string());

        assert_eq!(health.project_id, "proj-123");
        assert_eq!(health.status, HealthStatus::Unknown);
        assert!(health.issues.is_empty());
        assert_eq!(health.dependency_count, 0);
        assert!(health.missing_dependencies.is_empty());
    }

    #[test]
    fn test_project_health_add_issue() {
        let mut health = ProjectHealth::new("proj-123".to_string());

        health.add_issue(HealthIssue {
            severity: IssueSeverity::Warning,
            category: IssueCategory::DependencyMissing,
            message: "Missing dependency: lodash".to_string(),
            suggestion: Some("Run npm install lodash".to_string()),
        });

        assert_eq!(health.status, HealthStatus::Warning);
        assert_eq!(health.issues.len(), 1);

        health.add_issue(HealthIssue {
            severity: IssueSeverity::Critical,
            category: IssueCategory::ConfigInvalid,
            message: "Invalid package.json".to_string(),
            suggestion: None,
        });

        assert_eq!(health.status, HealthStatus::Critical);
        assert_eq!(health.issues.len(), 2);
    }
}
