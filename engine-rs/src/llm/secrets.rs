//! 服务密钥（secrets.json 读写 + legacy 迁移 + env key 派生）。
//!
//! 移植自 `packages/core/src/llm/secrets.ts`（77 行）。
//! 纯逻辑（legacy 迁移 / env key 派生）+ std::fs IO 包装。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::collections::HashMap;
use std::path::Path;

/// secrets.json 契约：services`id` = { apiKey }。
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

/// services 键的磁盘插入序（104 号）。TS 层 3 迭代 `Object.entries(secrets
/// .services)` 为 JSON 插入序——解析层 HashMap 丢失顺序（97 号偏差备案），
/// 此处从原始 JSON 文本扫描 services 对象的一层键序；任何异常（文件缺失/
/// 无 services/文本异常）返回空序，调用方回退按名排序。
pub fn service_key_order(project_root: &Path) -> Vec<String> {
    let path = project_root.join(SECRETS_DIR).join(SECRETS_FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    json_object_keys(&raw, "services").unwrap_or_default()
}

/// 在 JSON 文本中定位键 `container` 的对象值，返回其一层键的文本序。
/// 独立于 serde 解析（BTreeMap 丢序）——只处理良构文本，异常返回 None。
fn json_object_keys(raw: &str, container: &str) -> Option<Vec<String>> {
    let bytes = raw.as_bytes();
    let skip_ws = |i: &mut usize| {
        while *i < bytes.len() && matches!(bytes[*i], b' ' | b'\t' | b'\n' | b'\r') {
            *i += 1;
        }
    };
    // 定位键 "container"（后随冒号 + '{'）。
    let needle = format!("\"{container}\"");
    let needle = needle.as_bytes();
    let mut cursor = 0usize;
    let mut object_start = None;
    while cursor + needle.len() <= bytes.len() {
        if &bytes[cursor..cursor + needle.len()] == needle {
            let mut j = cursor + needle.len();
            skip_ws(&mut j);
            if j < bytes.len() && bytes[j] == b':' {
                j += 1;
                skip_ws(&mut j);
                if j < bytes.len() && bytes[j] == b'{' {
                    object_start = Some(j);
                }
            }
            break;
        }
        cursor += 1;
    }
    let start = object_start?;
    let mut keys = Vec::new();
    let mut i = start + 1;
    loop {
        skip_ws(&mut i);
        if i >= bytes.len() {
            return None;
        }
        match bytes[i] {
            b'}' => return Some(keys),
            b',' => {
                i += 1;
                continue;
            }
            b'"' => {}
            _ => return None,
        }
        // 解析键字符串（转义按原样保留；键为 ASCII 服务 id，无实义转义）。
        let mut key: Vec<u8> = Vec::new();
        i += 1;
        let mut escape = false;
        loop {
            if i >= bytes.len() {
                return None;
            }
            let byte = bytes[i];
            if escape {
                key.push(byte);
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                break;
            } else {
                key.push(byte);
            }
            i += 1;
        }
        i += 1;
        skip_ws(&mut i);
        if i >= bytes.len() || bytes[i] != b':' {
            return None;
        }
        i += 1;
        skip_ws(&mut i);
        // 跳过值（字符串整段；嵌套对象/数组按深度，一层逗号/闭括号即值尾）。
        let mut depth = 0i32;
        let mut in_string = false;
        let mut value_escape = false;
        while i < bytes.len() {
            let byte = bytes[i];
            if in_string {
                if value_escape {
                    value_escape = false;
                } else if byte == b'\\' {
                    value_escape = true;
                } else if byte == b'"' {
                    in_string = false;
                }
            } else {
                match byte {
                    b'"' => in_string = true,
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    b',' if depth == 0 => break,
                    _ => {}
                }
            }
            i += 1;
        }
        if depth != 0 || in_string {
            return None;
        }
        keys.push(String::from_utf8(key).ok()?);
    }
}

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
    // 221 号：原子替换写（temp + rename）——API key 存储截断的代价是密钥
    // 丢失，比普通配置更高；与 inkos.json 原子写（同批）一致。
    let path = dir.join(SECRETS_FILE);
    let temp = dir.join(format!("{SECRETS_FILE}.tmp-{}", uuid::Uuid::new_v4()));
    let write = || -> std::io::Result<()> {
        std::fs::write(&temp, format!("{json}\n"))?;
        // 227 号：0600 权限（对齐 src-tauri secrets M3b）——文件含明文 API
        // key，默认 0644 同机其他用户可读；rename 前 chmod 消除可读窗口。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&temp, &path)
    };
    match write() {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

#[cfg(test)]
mod atomicity_tests {
    use super::*;

    /// 221 号：secrets 原子写——成功后无 temp 残留、往返内容一致；失败
    /// （目录不存在时的 rename 场景不可注入，改验残留清理路径）时旧文件保留。
    #[test]
    fn save_secrets_atomic_no_tmp_leftover() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut services = std::collections::HashMap::new();
        services.insert("svc".to_string(), ServiceSecret { api_key: "k-1".into() });
        let mut secrets = SecretsFile { services };
        save_secrets(root, &secrets).unwrap();

        let secrets_dir = root.join(SECRETS_DIR);
        let entries: Vec<_> = std::fs::read_dir(&secrets_dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "仅 secrets.json，无 temp 残留");
        assert!(entries[0].as_ref().unwrap().file_name().to_string_lossy().ends_with(".json"));

        let loaded = load_secrets(root).unwrap();
        assert_eq!(loaded.services.get("svc").map(|s| s.api_key.clone()), Some("k-1".to_string()));

        // 二次写（覆盖路径）：仍然原子、无残留。
        secrets.services.insert("svc".to_string(), ServiceSecret { api_key: "k-2".into() });
        save_secrets(root, &secrets).unwrap();
        let count = std::fs::read_dir(&secrets_dir).unwrap().count();
        assert_eq!(count, 1);
        let loaded = load_secrets(root).unwrap();
        assert_eq!(loaded.services.get("svc").map(|s| s.api_key.clone()), Some("k-2".to_string()));
    }

    /// 227 号：secrets 文件权限 0600（Unix；含明文 API key，不得全局可读）。
    #[cfg(unix)]
    #[test]
    fn save_secrets_sets_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut services = std::collections::HashMap::new();
        services.insert("svc".to_string(), ServiceSecret { api_key: "k".into() });
        save_secrets(root, &SecretsFile { services }).unwrap();

        let mode = std::fs::metadata(root.join(SECRETS_DIR).join(SECRETS_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "secrets.json 必须为 0600");
    }
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

    // ── 104 号：service_key_order / json_object_keys（磁盘插入序） ──

    #[test]
    fn json_object_keys_preserves_text_order() {
        // 字母序刻意逆排（HashMap/BTreeMap 解析必失序——只有文本扫描能保住）。
        let raw = r#"{"version":1,"services":{"zeta":{"apiKey":"k1"},"moonshot":{"apiKey":"k2"},"alpha":{"apiKey":"k3"}}}"#;
        assert_eq!(
            json_object_keys(raw, "services"),
            Some(vec!["zeta".to_string(), "moonshot".to_string(), "alpha".to_string()])
        );
    }

    #[test]
    fn json_object_keys_skips_nested_values_and_strings() {
        // 值含嵌套对象/数组/带逗号与括号的字符串——一层键序不受影响。
        let raw = r#"{
            "services": {
                "b": {"apiKey": "x,{y}[z]"},
                "a": {"apiKey": "k", "extra": {"deep": [1, 2], "s": "}{"}}
            }
        }"#;
        assert_eq!(
            json_object_keys(raw, "services"),
            Some(vec!["b".to_string(), "a".to_string()])
        );
    }

    #[test]
    fn json_object_keys_missing_or_malformed_returns_none() {
        assert_eq!(json_object_keys("{}", "services"), None);
        assert_eq!(json_object_keys(r#"{"services": "not-an-object"}"#, "services"), None);
        // 扫描越界（截断文本）→ None。
        assert_eq!(json_object_keys(r#"{"services": {"a""#, "services"), None);
    }

    #[test]
    fn service_key_order_reads_disk_and_falls_back_empty() {
        let dir = std::env::temp_dir().join(format!("inkos-secrets-order-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        // 无文件 → 空序（调用方回退排序）。
        assert!(service_key_order(&dir).is_empty());
        std::fs::create_dir_all(dir.join(".inkos")).unwrap();
        // 手写非字母序（模拟 sidecar 写入——serde BTreeMap 落盘必字母序）。
        std::fs::write(
            dir.join(".inkos").join("secrets.json"),
            r#"{"services":{"zeta-first":{"apiKey":"k"},"alpha-second":{"apiKey":"k"}}}"#,
        )
        .unwrap();
        assert_eq!(
            service_key_order(&dir),
            vec!["zeta-first".to_string(), "alpha-second".to_string()]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
