//! 项目管理模块

pub mod detector;
pub mod index;
pub mod scanner;
pub mod types;

pub use detector::ProjectDetector;
pub use index::ProjectIndex;
pub use scanner::ProjectScanner;
pub use types::{
    HealthIssue, HealthStatus, IssueCategory, IssueSeverity, ProjectHealth, ProjectMeta,
    ProjectType,
};
