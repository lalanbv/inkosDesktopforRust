//! 项目管理模块

pub mod detector;
pub mod health;
pub mod index;
pub mod manager;
pub mod scanner;
pub mod types;

pub use detector::ProjectDetector;
pub use health::ProjectHealthChecker;
pub use index::ProjectIndex;
pub use manager::ProjectManager;
pub use scanner::ProjectScanner;
pub use types::{
    HealthIssue, HealthStatus, IssueCategory, IssueSeverity, ProjectHealth, ProjectMeta,
    ProjectType,
};
