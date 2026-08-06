//! projects.json（已发布的最近项目）与 M6 索引之间的桥接层。
//!
//! ## 为什么需要这一层
//!
//! 应用实际发布的前端是 `picker/index.html`，它的数据源是 `projects.json`
//! （[`RecentProject`]：只有 path + name）。M6 引入的 SQLite 索引
//! （[`ProjectMeta`]：类型 / 收藏 / 标签 / 健康）是独立的第二份存储。
//!
//! 两者都保留、不做二选一：
//! - `projects.json` 继续作为「最近打开」的权威顺序（既有用户零迁移，
//!   且索引打不开时 picker 仍能正常工作）；
//! - M6 索引作为「元数据增强」的来源（类型徽章、收藏标记）。
//!
//! 本模块只做合并，不做写入决策——写入由 `main.rs` 的 `cmd_choose_project`
//! 在记录 projects.json 之后顺带同步索引（失败不阻断启动）。
//!
//! 合并逻辑写成纯函数 [`enrich_recents`]（索引查询以闭包注入），因此可以
//! 不依赖数据库、不依赖 Tauri 直接单测。

use crate::project::ProjectMeta;
use crate::projects::RecentProject;
use serde::{Deserialize, Serialize};

/// picker 展示用的最近项目：`projects.json` 基础字段 + M6 索引增强字段。
///
/// 增强字段全部可缺省——索引里查不到这个路径时（老用户的历史 recents、
/// 或索引打开失败）退化成与增强前完全一致的展示，不影响可用性。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichedRecent {
    /// 项目绝对路径（与 projects.json 一致，前端据此调 cmd_choose_project）
    pub path: String,
    /// 展示名（与 projects.json 一致）
    pub name: String,
    /// M6 索引中的项目类型字符串（"nodejs" / "rust" / …）；索引无此项时 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_type: Option<String>,
    /// M6 索引中的项目 ID（前端调 toggle_favorite 等命令需要）；索引无此项时 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// 是否收藏；索引无此项时为 false
    pub is_favorite: bool,
}

impl EnrichedRecent {
    /// 未增强的退化形态（索引查不到时使用）。
    fn plain(recent: &RecentProject) -> Self {
        Self {
            path: recent.path.clone(),
            name: recent.name.clone(),
            project_type: None,
            project_id: None,
            is_favorite: false,
        }
    }
}

/// 把 `projects.json` 的最近列表与 M6 索引数据合并。
///
/// `lookup` 按路径查索引，返回 `None` 表示索引里没有这个项目。传闭包而不是
/// 直接收 `&ProjectIndex`，是为了让本函数在测试里无需真实数据库。
///
/// 顺序严格保持 `recents` 的顺序（即「最近打开在前」，由
/// [`crate::projects::RecentProjects::record_open`] 维护），索引不参与排序——
/// 排序权威只有一个，避免两份存储不一致时顺序抖动。
///
/// 名称以 `projects.json` 为准：用户在 picker 里看到的名字不应该因为后台
/// 扫描重新识别出 package.json 的 name 而突然变化。
pub fn enrich_recents<F>(recents: &[RecentProject], lookup: F) -> Vec<EnrichedRecent>
where
    F: Fn(&str) -> Option<ProjectMeta>,
{
    recents
        .iter()
        .map(|recent| match lookup(&recent.path) {
            Some(meta) => EnrichedRecent {
                path: recent.path.clone(),
                name: recent.name.clone(),
                project_type: Some(meta.project_type.as_str().to_string()),
                project_id: Some(meta.id),
                is_favorite: meta.is_favorite,
            },
            None => EnrichedRecent::plain(recent),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectType;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn recent(path: &str, name: &str) -> RecentProject {
        RecentProject {
            path: path.to_string(),
            name: name.to_string(),
        }
    }

    fn meta(path: &str, name: &str, ty: ProjectType) -> ProjectMeta {
        ProjectMeta::new(name.to_string(), PathBuf::from(path), ty)
    }

    /// 用 HashMap 造一个假索引，避免测试依赖真实 SQLite。
    fn lookup_from(pairs: Vec<ProjectMeta>) -> impl Fn(&str) -> Option<ProjectMeta> {
        let map: HashMap<String, ProjectMeta> = pairs
            .into_iter()
            .map(|m| (m.path.to_string_lossy().into_owned(), m))
            .collect();
        move |path: &str| map.get(path).cloned()
    }

    #[test]
    fn test_enrich_empty_recents() {
        let out = enrich_recents(&[], |_| None);
        assert!(out.is_empty());
    }

    #[test]
    fn test_enrich_index_miss_degrades_gracefully() {
        // 索引里什么都没有（例如索引打开失败）→ 退化为纯 projects.json 展示
        let recents = vec![recent("/a", "alpha"), recent("/b", "beta")];
        let out = enrich_recents(&recents, |_| None);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "alpha");
        assert_eq!(out[0].path, "/a");
        assert_eq!(out[0].project_type, None);
        assert_eq!(out[0].project_id, None);
        assert!(!out[0].is_favorite);
    }

    #[test]
    fn test_enrich_index_hit_attaches_type_and_id() {
        let recents = vec![recent("/a", "alpha")];
        let indexed = meta("/a", "alpha", ProjectType::Rust);
        let expected_id = indexed.id.clone();

        let out = enrich_recents(&recents, lookup_from(vec![indexed]));

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].project_type.as_deref(), Some("rust"));
        assert_eq!(out[0].project_id.as_deref(), Some(expected_id.as_str()));
    }

    #[test]
    fn test_enrich_propagates_favorite() {
        let recents = vec![recent("/a", "alpha")];
        let mut indexed = meta("/a", "alpha", ProjectType::NodeJs);
        indexed.is_favorite = true;

        let out = enrich_recents(&recents, lookup_from(vec![indexed]));
        assert!(out[0].is_favorite);
    }

    #[test]
    fn test_enrich_partial_hit() {
        // 一半在索引里、一半不在（老用户历史 recents 尚未被扫描收录）
        let recents = vec![recent("/a", "alpha"), recent("/b", "beta")];
        let out = enrich_recents(
            &recents,
            lookup_from(vec![meta("/a", "alpha", ProjectType::Go)]),
        );

        assert_eq!(out[0].project_type.as_deref(), Some("go"));
        assert_eq!(out[1].project_type, None);
    }

    #[test]
    fn test_enrich_preserves_recents_order() {
        // 顺序权威只有 projects.json；索引的插入顺序不得影响展示顺序
        let recents = vec![recent("/c", "c"), recent("/a", "a"), recent("/b", "b")];
        let out = enrich_recents(
            &recents,
            lookup_from(vec![
                meta("/a", "a", ProjectType::Rust),
                meta("/b", "b", ProjectType::NodeJs),
                meta("/c", "c", ProjectType::Go),
            ]),
        );

        let paths: Vec<&str> = out.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["/c", "/a", "/b"]);
    }

    #[test]
    fn test_enrich_prefers_recents_name_over_index_name() {
        // 索引里叫 "package-json-name"，projects.json 里叫 "用户看到的名字"
        // → 展示名以 projects.json 为准，避免名字无预期地跳变
        let recents = vec![recent("/a", "用户看到的名字")];
        let out = enrich_recents(
            &recents,
            lookup_from(vec![meta("/a", "package-json-name", ProjectType::NodeJs)]),
        );

        assert_eq!(out[0].name, "用户看到的名字");
    }

    #[test]
    fn test_enriched_recent_serializes_camel_free_snake_keys() {
        // picker 是手写 JS，直接读 snake_case 字段；固化该契约防止无意改名
        let json = serde_json::to_string(&EnrichedRecent {
            path: "/a".into(),
            name: "alpha".into(),
            project_type: Some("rust".into()),
            project_id: Some("id-1".into()),
            is_favorite: true,
        })
        .unwrap();

        assert!(json.contains("\"project_type\":\"rust\""));
        assert!(json.contains("\"project_id\":\"id-1\""));
        assert!(json.contains("\"is_favorite\":true"));
    }

    #[test]
    fn test_enriched_recent_omits_absent_optionals() {
        // 索引未命中时不输出 project_type / project_id 键，减少前端判空分支
        let json = serde_json::to_string(&EnrichedRecent::plain(&recent("/a", "alpha"))).unwrap();

        assert!(!json.contains("project_type"));
        assert!(!json.contains("project_id"));
        assert!(json.contains("\"is_favorite\":false"));
    }
}
