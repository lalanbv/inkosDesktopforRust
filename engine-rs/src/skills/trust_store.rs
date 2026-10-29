//! 外部技能来源信任库（768 号，决策①A）——TS `skills/trust-store.ts` 逐字镜像。
//!
//! 仅 external（INKOS_SKILL_DIRS 显式）来源需确认；user/project 默认信任。
//! 状态落 `{project_root}/.inkos/trusted-sources.json`
//!（[`crate::utils::atomic_file_set::write_file_atomic`]，767 号起持久性语义）。
//! 路径按 env 配置串归一（trim + 去尾分隔符）原样比对——双端同规，不做
//! 符号链接归一（跨端 /tmp 形态分歧）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::utils::atomic_file_set::write_file_atomic;

const TRUST_FILE_RELATIVE: &str = ".inkos/trusted-sources.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedSourceEntry {
    pub path: String,
    /// camelCase 对齐 TS 写面（`confirmed_at` 蛇形会让写读键名不对称、
    /// 回读丢条目——首轮测试实锤后修正）。
    #[serde(rename = "confirmedAt")]
    pub confirmed_at: String,
}

#[derive(Debug, Serialize)]
struct TrustedSourceFile<'a> {
    version: u8,
    entries: &'a [TrustedSourceEntry],
}

/// env 配置串归一：trim + 去尾分隔符（根路径 "/" 除外）。对齐 TS
/// `normalizeTrustedSourcePath`。
pub fn normalize_trusted_source_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.chars().count() > 1 {
        trimmed.trim_end_matches(['/', '\\']).to_string()
    } else {
        trimmed.to_string()
    }
}

fn trust_file(project_root: &Path) -> PathBuf {
    project_root.join(TRUST_FILE_RELATIVE)
}

/// 读信任条目；缺文件/坏 JSON → 空（信任库损坏不阻断技能装载——与 TS
/// parseEntries 容错同构，最多回退到"全部 external 待确认"保守面）。
pub async fn read_trusted_sources(project_root: &Path) -> Vec<TrustedSourceEntry> {
    let Ok(raw) = tokio::fs::read_to_string(trust_file(project_root)).await else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let Some(entries) = value.get("entries").and_then(|entries| entries.as_array()) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let path = entry.get("path")?.as_str()?.to_string();
            let confirmed_at = entry.get("confirmedAt")?.as_str()?.to_string();
            Some(TrustedSourceEntry { path, confirmed_at })
        })
        .collect()
}

pub async fn is_trusted_source(project_root: &Path, path: &str) -> bool {
    let normalized = normalize_trusted_source_path(path);
    read_trusted_sources(project_root)
        .await
        .iter()
        .any(|entry| entry.path == normalized)
}

async fn write_entries(project_root: &Path, entries: &[TrustedSourceEntry]) -> std::io::Result<()> {
    // write_file_atomic 不建父目录（TS 侧 writeEntries 先 mkdir recursive 同构）。
    tokio::fs::create_dir_all(project_root.join(".inkos")).await?;
    let serialized = serde_json::to_string_pretty(&TrustedSourceFile {
        version: 1,
        entries,
    })
    .unwrap_or_default();
    write_file_atomic(&trust_file(project_root), &format!("{serialized}\n")).await
}

/// 新增信任来源（幂等；已存在不刷新 confirmed_at）。返回写后全量条目。
pub async fn add_trusted_source(
    project_root: &Path,
    path: &str,
) -> std::io::Result<Vec<TrustedSourceEntry>> {
    let normalized = normalize_trusted_source_path(path);
    if normalized.is_empty() {
        return Err(std::io::Error::other("trusted source path is required."));
    }
    let mut entries = read_trusted_sources(project_root).await;
    if !entries.iter().any(|entry| entry.path == normalized) {
        entries.push(TrustedSourceEntry {
            path: normalized,
            confirmed_at: crate::utils::utc_time::utc_now_iso(),
        });
        write_entries(project_root, &entries).await?;
    }
    Ok(entries)
}

/// 移除信任来源（幂等）。返回写后全量条目。
pub async fn remove_trusted_source(
    project_root: &Path,
    path: &str,
) -> std::io::Result<Vec<TrustedSourceEntry>> {
    let normalized = normalize_trusted_source_path(path);
    let entries: Vec<TrustedSourceEntry> = read_trusted_sources(project_root)
        .await
        .into_iter()
        .filter(|entry| entry.path != normalized)
        .collect();
    write_entries(project_root, &entries).await?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn trust_store_roundtrip_and_gate() {
        let dir = tempfile::tempdir().expect("临时目录");
        let root = dir.path();
        assert!(!is_trusted_source(root, "/ext/skills").await);
        assert!(read_trusted_sources(root).await.is_empty());

        let entries = add_trusted_source(root, "/ext/skills/").await.expect("添加");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "/ext/skills", "尾分隔符归一");
        assert!(is_trusted_source(root, "/ext/skills").await);

        // 幂等：重复添加不刷新 confirmedAt、不增条目。
        let again = add_trusted_source(root, "/ext/skills").await.expect("重复添加");
        assert_eq!(again.len(), 1);

        let after_remove = remove_trusted_source(root, "/ext/skills").await.expect("移除");
        assert!(after_remove.is_empty());
        assert!(!is_trusted_source(root, "/ext/skills").await);
    }

    #[tokio::test]
    async fn trust_store_tolerates_broken_json() {
        let dir = tempfile::tempdir().expect("临时目录");
        let file = trust_file(dir.path());
        tokio::fs::create_dir_all(file.parent().unwrap()).await.unwrap();
        tokio::fs::write(&file, "{ broken").await.unwrap();
        assert!(read_trusted_sources(dir.path()).await.is_empty());
        assert!(!is_trusted_source(dir.path(), "/x").await);
    }

    // 768 号决策①A：载入闸镜像——external 未信任 → 技能不入列仅诊断；
    // 信任后重载入列；user/project 默认信任不受闸影响（对齐 TS
    // external-skill-loader.test "withholds untrusted external skill sources"）。
    #[tokio::test]
    async fn load_configured_gates_untrusted_external_source() {
        let dir = tempfile::tempdir().expect("临时目录");
        let root = dir.path();
        let external_root = root.join("gated-external");
        let skill_dir = external_root.join("gated-romance");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Gated Romance\ndescription: Gated external skill.\n---\nBody.",
        )
        .await
        .unwrap();
        let project_root = root.join("trust-project");
        let home_dir = root.join("trust-home");
        tokio::fs::create_dir_all(&project_root).await.unwrap();

        let env_dirs = vec![external_root.to_string_lossy().to_string()];
        let untrusted =
            crate::skills::external_loader::load_configured_agent_skills(&project_root, &env_dirs, Some(&home_dir))
                .await;
        assert!(untrusted.skills.iter().all(|skill| skill.id != "gated-romance"));
        assert!(untrusted.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("pending trust confirmation")
                && diagnostic.path == external_root.to_string_lossy()
        }));

        add_trusted_source(&project_root, &external_root.to_string_lossy())
            .await
            .expect("添加信任");
        let trusted =
            crate::skills::external_loader::load_configured_agent_skills(&project_root, &env_dirs, Some(&home_dir))
                .await;
        assert!(trusted.skills.iter().any(|skill| skill.id == "gated-romance"));
        assert!(!trusted.diagnostics.iter().any(|diagnostic| diagnostic.message.contains("pending trust confirmation")));
    }
}
