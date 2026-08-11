//! 服务密钥（secrets.json 读写 + legacy 迁移 + env key 派生）。
//!
//! 移植自 `packages/core/src/llm/secrets.ts`（77 行）。
//! 纯逻辑（legacy 迁移 / env key 派生）+ std::fs IO 包装。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::collections::HashMap;
use std::path::Path;

/// secrets.json 契约：services[id] = { apiKey }。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct SecretsFile {
    #[serde(default)]
    pub services: HashMap<String, ServiceSecret>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ServiceSecret {
    #[serde(rename = "apiKey")]
    pub api_key: String,
}

/// legacy service id 重映射（siliconflow → siliconcloud）。
const LEGACY_SERVICE_ID_REMAP: &[(&str, &str)] = &[("siliconflow", "siliconcloud")];

/// 迁移 legacy service id（pure）。仅当 new id 不存在时复制 old→new 并删 old（对齐 TS）。
pub fn migrate_legacy_service_ids(mut secrets: SecretsFile) -> (SecretsFile, bool) {
    let mut changed = false;
    for (old_id, new_id) in LEGACY_SERVICE_ID_REMAP {
        if !secrets.services.contains_key(*new_id) {
            if let Some(entry) = secrets.services.get(*old_id).cloned() {
                secrets.services.insert((*new_id).to_string(), entry);
                secrets.services.remove(*old_id);
                changed = true;
            }
        }
    }
    (secrets, changed)
}

/// 由 service id 派生 env var 名：非字母数字→_，大写，加 _API_KEY 后缀。
/// 对齐 TS `${service.replace(/[^a-zA-Z0-9]/g, "_").toUpperCase()}_API_KEY`。
pub fn service_env_key(service: &str) -> String {
    let cleaned: String = service
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{}_API_KEY", cleaned.to_uppercase())
}

/// 从 service id 解析 API key：先查 secrets，再查 env var。
/// 返回 (key, 来源) 便于调试；None 表示未找到。
pub fn resolve_service_api_key(secrets: &SecretsFile, service: &str, env_lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(entry) = secrets.services.get(service) {
        if !entry.api_key.is_empty() {
            return Some(entry.api_key.clone());
        }
    }
    let env_key = service_env_key(service);
    env_lookup(&env_key)
}

// ── IO 包装（std::fs）────────────────────────────────────────────

const SECRETS_DIR: &str = ".inkos";
const SECRETS_FILE: &str = "secrets.json";

/// 读取 secrets.json（缺失/损坏 → 空）。返回原始（未迁移）。
pub fn read_secrets_raw(project_root: &Path) -> SecretsFile {
    let path = project_root.join(SECRETS_DIR).join(SECRETS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<SecretsFile>(&raw) {
            Ok(s) => {
                if s.services.is_empty() && !raw.contains("\"services\"") {
                    return SecretsFile::default();
                }
                // 确保有 services 字段（旧格式兜底）
                s
            }
            Err(_) => SecretsFile::default(),
        },
        Err(_) => SecretsFile::default(),
    }
}

/// 加载 + 迁移 legacy id（若有变更自动保存）。
pub fn load_secrets(project_root: &Path) -> std::io::Result<SecretsFile> {
    let raw = read_secrets_raw(project_root);
    let (data, changed) = migrate_legacy_service_ids(raw);
    if changed {
        save_secrets(project_root, &data)?;
    }
    Ok(data)
}

/// 保存 secrets.json（创建 .inkos/ 目录）。
pub fn save_secrets(project_root: &Path, secrets: &SecretsFile) -> std::io::Result<()> {
    let dir = project_root.join(SECRETS_DIR);
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(secrets).unwrap_or_else(|_| "{}".into());
    std::fs::write(dir.join(SECRETS_FILE), format!("{json}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn secret(key: &str) -> ServiceSecret {
        ServiceSecret { api_key: key.into() }
    }

    #[test]
    fn migrate_legacy_remaps_siliconflow() {
        let mut services = HashMap::new();
        services.insert("siliconflow".into(), secret("k1"));
        let (out, changed) = migrate_legacy_service_ids(SecretsFile { services });
        assert!(changed);
        assert!(out.services.contains_key("siliconcloud"));
        assert!(!out.services.contains_key("siliconflow"));
    }

    #[test]
    fn migrate_no_change_when_already_new() {
        let mut services = HashMap::new();
        services.insert("siliconcloud".into(), secret("k1"));
        let (out, changed) = migrate_legacy_service_ids(SecretsFile { services });
        assert!(!changed);
        assert_eq!(out.services.len(), 1);
    }

    #[test]
    fn migrate_preserves_existing_new_id() {
        // 既有 siliconflow 又有 siliconcloud → TS 不迁移（new 已存在），两者都保留
        let mut services = HashMap::new();
        services.insert("siliconflow".into(), secret("old"));
        services.insert("siliconcloud".into(), secret("new"));
        let (out, changed) = migrate_legacy_service_ids(SecretsFile { services });
        assert!(!changed); // 不迁移
        assert_eq!(out.services.get("siliconcloud").unwrap().api_key, "new");
        assert!(out.services.contains_key("siliconflow")); // 旧 id 保留
    }

    #[test]
    fn env_key_derivation() {
        assert_eq!(service_env_key("moonshot"), "MOONSHOT_API_KEY");
        assert_eq!(service_env_key("deepseek"), "DEEPSEEK_API_KEY");
        assert_eq!(service_env_key("custom:My-Service"), "CUSTOM_MY_SERVICE_API_KEY");
    }

    #[test]
    fn resolve_key_prefers_secrets_then_env() {
        let mut services = HashMap::new();
        services.insert("deepseek".into(), secret("sk-from-file"));
        let s = SecretsFile { services };
        // secrets 命中
        assert_eq!(
            resolve_service_api_key(&s, "deepseek", |_| None),
            Some("sk-from-file".into())
        );
        // secrets 空 → env
        let empty = SecretsFile::default();
        assert_eq!(
            resolve_service_api_key(&empty, "deepseek", |k| if k == "DEEPSEEK_API_KEY" { Some("sk-env".into()) } else { None }),
            Some("sk-env".into())
        );
        // 都无 → None
        assert_eq!(resolve_service_api_key(&empty, "missing", |_| None), None);
    }

    #[test]
    fn secrets_roundtrip() {
        let dir = std::env::temp_dir().join(format!("inkos-secrets-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let mut services = HashMap::new();
        services.insert("openai".into(), secret("sk-123"));
        let s = SecretsFile { services };
        save_secrets(&dir, &s).unwrap();
        let loaded = load_secrets(&dir).unwrap();
        assert_eq!(loaded.services.get("openai").unwrap().api_key, "sk-123");
        std::fs::remove_dir_all(&dir).ok();
    }
}
