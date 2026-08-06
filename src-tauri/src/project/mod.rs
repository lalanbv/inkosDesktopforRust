//! 项目管理模块

pub mod index;
pub mod types;

pub use index::ProjectIndex;
pub use types::{
    HealthIssue, HealthStatus, IssueCategory, IssueSeverity, ProjectHealth, ProjectMeta,
    ProjectType,
};
