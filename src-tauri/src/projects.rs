//! 最近项目持久化（app_data/projects.json）—— 解 C2 的数据层。
//!
//! 启动时 `read` → 若 `last_opened` 有效则自动复用（免选择，快启）；
//! 否则前端 picker 显示 `recent` 列表 + 目录选择对话框。选定后 `record_open` 持久化。
//!
//! 纯逻辑（无 Tauri 依赖）：路径校验、原子写、最近列表去重/截断/置顶全在此可单测。
//! main.rs / picker 负责调用与 UI。

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::util::atomic_write_0600;

/// 最近项目列表上限（置顶 + 去重后截断）。
pub const MAX_RECENT: usize = 10;

/// 单条最近项目（path 绝对路径；name 展示名，缺省时取目录名）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentProject {
    pub path: String,
    pub name: String,
}

/// 最近项目集合（projects.json schema）。
///
/// `recent` 顺序即展示顺序（最近打开在前，由 [`Self::record_open`] 维护）。
/// `last_opened` = 上次打开的项目 path，启动时自动复用（快启）；None 表示首次/未选。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentProjects {
    #[serde(default)]
    pub recent: Vec<RecentProject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened: Option<String>,
}

impl RecentProjects {
    /// 读 projects.json；文件缺失 → 空（首次运行），不报错。
    /// 损坏 JSON → Err（不静默吞，让上层提示用户）。
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let p: Self = serde_json::from_slice(&bytes)
                    .with_context(|| format!("解析 projects.json 失败: {}", path.display()))?;
                Ok(p)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e)
                .with_context(|| format!("读取 projects.json 失败: {}", path.display())),
        }
    }

    /// 原子写（0600）：projects.json 含用户项目路径（隐私相关），0600 防短暂可读窗口。
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self).context("序列化 projects.json 失败")?;
        atomic_write_0600(path, json.as_bytes())
    }

    /// 不可变：返回打开 `path` 后的新快照（置顶 + 去重 + 截断 + last_opened 更新）。
    ///
    /// - `name` 若为空，落库时改为 `path` 的文件名（便于 UI 展示）。
    /// - 同 path 旧条目移除后重新置顶（顺序=最近在前）。
    /// - 超过 [`MAX_RECENT`] 截断尾部。
    pub fn record_open(&self, path: &str, name: &str) -> Self {
        let resolved_name = if name.trim().is_empty() {
            std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            name.to_string()
        };
        let entry = RecentProject {
            path: path.to_string(),
            name: resolved_name,
        };
        let mut recent = vec![entry];
        for r in &self.recent {
            if r.path == path {
                continue; // 去重：旧条目跳过（已置顶新条目）
            }
            recent.push(r.clone());
            if recent.len() >= MAX_RECENT {
                break;
            }
        }
        recent.truncate(MAX_RECENT);
        Self {
            recent,
            last_opened: Some(path.to_string()),
        }
    }

    /// 按 path 查找最近项目（供启动校验 last_opened 是否仍在列表/有效）。
    pub fn find(&self, path: &str) -> Option<&RecentProject> {
        self.recent.iter().find(|r| r.path == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_missing_file_returns_default() {
        let p = RecentProjects::read(std::path::Path::new("/nonexistent/projects.json")).unwrap();
        assert!(p.recent.is_empty());
        assert_eq!(p.last_opened, None);
    }

    #[test]
    fn read_corrupt_json_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("projects.json");
        std::fs::write(&path, b"not json").unwrap();
        assert!(RecentProjects::read(&path).is_err());
    }

    #[test]
    fn write_read_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("projects.json");
        let original =
            RecentProjects::default().record_open("/tmp/proj1", "Proj1");
        original.write(&path).unwrap();
        let loaded = RecentProjects::read(&path).unwrap();
        assert_eq!(loaded, original);
    }

    #[cfg(unix)]
    #[test]
    fn write_produces_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("projects.json");
        RecentProjects::default()
            .record_open("/tmp/proj1", "Proj1")
            .write(&path)
            .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn record_open_puts_new_path_at_front_and_sets_last_opened() {
        let base = RecentProjects {
            recent: vec![RecentProject {
                path: "/tmp/old".to_string(),
                name: "Old".to_string(),
            }],
            last_opened: Some("/tmp/old".to_string()),
        };
        let next = base.record_open("/tmp/new", "New");
        assert_eq!(next.recent[0].path, "/tmp/new");
        assert_eq!(next.last_opened.as_deref(), Some("/tmp/new"));
        // 旧项目保留在尾部。
        assert!(next.recent.iter().any(|r| r.path == "/tmp/old"));
    }

    #[test]
    fn record_open_dedups_existing_path_to_front() {
        let base = RecentProjects {
            recent: vec![
                RecentProject { path: "/tmp/a".into(), name: "A".into() },
                RecentProject { path: "/tmp/b".into(), name: "B".into() },
                RecentProject { path: "/tmp/c".into(), name: "C".into() },
            ],
            last_opened: Some("/tmp/a".into()),
        };
        // 重开 /tmp/b：应置顶，旧 b 条目移除，顺序 [b, a, c]。
        let next = base.record_open("/tmp/b", "B");
        assert_eq!(
            next.recent.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            vec!["/tmp/b", "/tmp/a", "/tmp/c"]
        );
    }

    #[test]
    fn record_open_caps_at_max_recent() {
        let mut base = RecentProjects::default();
        for i in 0..(MAX_RECENT + 3) {
            base = base.record_open(&format!("/tmp/p{i}"), &format!("P{i}"));
        }
        assert_eq!(base.recent.len(), MAX_RECENT);
        // 最新置顶（倒序写入，最后写的在最前）。
        assert_eq!(base.recent[0].path, format!("/tmp/p{}", MAX_RECENT + 2));
    }

    #[test]
    fn record_open_fills_empty_name_from_basename() {
        let next = RecentProjects::default().record_open("/tmp/myproj", "");
        assert_eq!(next.recent[0].name, "myproj");
        // 恶意/异常 path 无 basename 时退化为 path 本身（不 panic）。
        let next2 = RecentProjects::default().record_open("/", "");
        assert!(!next2.recent[0].name.is_empty());
    }

    #[test]
    fn find_locates_by_path() {
        let base = RecentProjects::default().record_open("/tmp/x", "X");
        assert!(base.find("/tmp/x").is_some());
        assert!(base.find("/tmp/missing").is_none());
    }
}
