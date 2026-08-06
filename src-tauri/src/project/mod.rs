//! 项目管理模块
//!
//! 分层结构（下层不依赖上层）：
//! - `types`    — 核心数据类型（ProjectMeta / ProjectHealth）
//! - `index`    — SQLite 持久化层（CRUD + 查询）
//! - `detector` — 项目类型识别（特征文件 → ProjectType + 名称）
//! - `scanner`  — 文件系统遍历（发现项目根）
//! - `health`   — 健康检查（依赖 / 配置 / 环境 / 权限 / 磁盘）
//! - `manager`  — 生命周期管理（整合以上各层 + 内存缓存）
//! - `commands` — Tauri 命令层（前端 API）

pub mod commands;
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
