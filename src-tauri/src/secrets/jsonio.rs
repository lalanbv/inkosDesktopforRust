//! `.inkos/secrets.json` 纯函数读写：与 `SecretStore` trait 解耦，
//! 供上层 store 实现（MockStore / KeyringStore / 未来迁移层）复用。
//!
//! ## Schema（对齐 inkos `packages/core/src/llm/secrets.ts:loadSecrets`）
//! ```json
//! { "services": { "<serviceId>": { "apiKey": "<key>" } } }
//! ```
//! - 顶层 `services` 字段；每个 service entry 至少含 `apiKey`（**camelCase**）。
//! - 损坏 / 缺失文件 → 视为空（与 inkos `loadSecrets` catch-returns-default 语义一致），不 panic。
//!
//! ## write 策略：merge-保留 + 0600 + 原子 rename
//! - 先读既有文件得到完整 `serde_json::Value`，只覆盖 `services[*].apiKey`，
//!   保留 inkos 未来可能添加的其他字段（forward-compat）。
//! - pretty 2-space 缩进（对齐 inkos `JSON.stringify(secrets, null, 2)`）。
//! - tmp 文件 + `rename` 原子替换；Unix 设置 0600 权限（Windows cfg-gate 跳过）。
//!
//! ## 错误处理
//! - 文件缺失 / JSON 损坏（read）→ `Ok(empty)`，不报错（对齐 inkos）。
//! - IO 失败 / 序列化失败（write）→ `Err`，显式向上传播（不静默吞错）。

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 从 `secrets.json` 读取全部 `serviceId -> apiKey` 映射。
///
/// 行为契约（对齐 inkos `loadSecrets` 容错语义）：
/// - 文件缺失 → `Ok(empty)`，不报错。
/// - JSON 损坏 / 非 object / 缺 `services` → `Ok(empty)`，不 panic。
/// - service entry 缺 `apiKey` 或值非 string → 该 entry 跳过（不影响其他 entry）。
///
/// 仅底层 IO 失败（权限、磁盘错误等）返回 `Err`。
///
/// 注：本层不区分"空字符串 apiKey"与"缺失 apiKey"——空串会被读出。
/// inkos `getServiceApiKey` 把空串视为未设置（`if (entry?.apiKey) return entry.apiKey`），
/// 该语义由上层 store 负责；jsonio 仅做 schema 解析。
pub fn read_secrets(path: &Path) -> Result<HashMap<String, String>> {
    let txt = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => {
            return Err(e).with_context(|| format!("read secrets: {}", path.display()))
        }
    };
    Ok(parse_api_keys(&txt))
}

/// 把 `services.*.apiKey` 解析为 `HashMap`；损坏 / 缺字段 → 空。
fn parse_api_keys(txt: &str) -> HashMap<String, String> {
    let root: Value = match serde_json::from_str(txt) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let services = match root.get("services").and_then(Value::as_object) {
        Some(m) => m,
        None => return HashMap::new(),
    };
    services
        .iter()
        .filter_map(|(id, entry)| {
            entry
                .get("apiKey")
                .and_then(Value::as_str)
                .map(|k| (id.clone(), k.to_string()))
        })
        .collect()
}

/// 把 `map` 合并写入 `secrets.json`（merge-保留 + 0600 + 原子 rename）。
///
/// 行为契约：
/// - 先读既有文件（若存在且 JSON 合法）：保留所有非 `apiKey` 字段与其他 service。
/// - 对 `map` 中每个 `(serviceId, apiKey)`：
///   - service entry 已存在且为 object → 只覆盖 `apiKey`（保留同 service 其他字段）。
///   - service entry 不存在 → 新建 `{ "apiKey": value }`。
///   - service entry 存在但非 object（schema 不符）→ 整体替换为合法 entry（修坏数据）。
/// - 写入格式：pretty 2-space 缩进（对齐 inkos）。
/// - 原子写：tmp 文件 + rename；Unix 设置 0600 权限。
pub fn write_secrets(path: &Path, map: &HashMap<String, String>) -> Result<()> {
    let mut root = load_existing_root(path);
    ensure_services_object(&mut root);
    let services = root
        .get_mut("services")
        .and_then(Value::as_object_mut)
        .expect("services object ensured above");

    for (id, key) in map {
        let entry = services
            .entry(id.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("apiKey".to_string(), Value::String(key.clone()));
        } else {
            // 既有 entry 非 object（schema 不符）：替换为合法 entry，不静默保留坏数据。
            *entry = json!({ "apiKey": key });
        }
    }

    let json = serde_json::to_string_pretty(&root)
        .with_context(|| "serialize secrets.json")?;
    atomic_write_0600(path, json.as_bytes())
        .with_context(|| format!("atomic write secrets: {}", path.display()))?;
    Ok(())
}

/// 读既有文件 → `Value`；缺失 / 损坏 → 空 Object。
fn load_existing_root(path: &Path) -> Value {
    match std::fs::read_to_string(path) {
        Ok(txt) => serde_json::from_str(&txt).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    }
}

/// 确保 `root` 是 Object 且 `services` 是 Object。
fn ensure_services_object(root: &mut Value) {
    if !root.is_object() {
        *root = Value::Object(Map::new());
    }
    let obj = root.as_object_mut().expect("root ensured as object");
    let services_is_object = obj
        .get("services")
        .map(Value::is_object)
        .unwrap_or(false);
    if !services_is_object {
        obj.insert("services".to_string(), Value::Object(Map::new()));
    }
}

/// tmp + rename 原子写；Unix 设置 0600 权限。
fn atomic_write_0600(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = tmp_path_for(path);
    std::fs::write(&tmp, data)
        .with_context(|| format!("write tmp: {}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600: {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

/// 在与 `path` 同目录下构造 `.tmp` 后缀临时路径。
fn tmp_path_for(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("secrets.json"));
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_raw(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn roundtrip_preserves_apikeys() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        let mut m = HashMap::new();
        m.insert("openai".into(), "sk-1".into());
        m.insert("deepseek".into(), "dk-1".into());
        write_secrets(&p, &m).unwrap();

        let got = read_secrets(&p).unwrap();
        assert_eq!(got.get("openai").unwrap(), "sk-1");
        assert_eq!(got.get("deepseek").unwrap(), "dk-1");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn read_missing_file_returns_empty_ok() {
        let p = Path::new("/nonexistent/path/secrets.json");
        let got = read_secrets(&p).unwrap();
        assert!(got.is_empty(), "missing file should yield Ok(empty), not Err");
    }

    #[test]
    fn read_corrupt_json_returns_empty_ok() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(&p, "{ not valid json }}}}");
        let got = read_secrets(&p).unwrap();
        assert!(
            got.is_empty(),
            "corrupt JSON should yield Ok(empty), not panic"
        );
    }

    #[test]
    fn read_missing_services_field_returns_empty_ok() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(&p, r#"{"version": 42}"#);
        let got = read_secrets(&p).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn read_entry_missing_apikey_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(
            &p,
            r#"{"services": {"openai": {"apiKey": "sk-x"}, "broken": {}}}"#,
        );
        let got = read_secrets(&p).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got.get("openai").unwrap(), "sk-x");
    }

    #[test]
    fn write_preserves_other_services_and_fields() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        // 既有：含 anthropic + service 额外字段 + 顶层额外字段
        write_raw(
            &p,
            r#"{
                "services": {
                    "anthropic": { "apiKey": "ak-old", "note": "keep me" }
                },
                "schemaVersion": 3
            }"#,
        );

        let mut m = HashMap::new();
        m.insert("openai".into(), "sk-new".into());
        // 同时更新 anthropic：应只覆盖 apiKey，保留 note
        m.insert("anthropic".into(), "ak-new".into());
        write_secrets(&p, &m).unwrap();

        let raw = std::fs::read_to_string(&p).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        // 新 service 正确写入
        assert_eq!(v["services"]["openai"]["apiKey"], "sk-new");
        // 已有 service：apiKey 更新，note 保留
        assert_eq!(v["services"]["anthropic"]["apiKey"], "ak-new");
        assert_eq!(v["services"]["anthropic"]["note"], "keep me");
        // 顶层其他字段保留
        assert_eq!(v["schemaVersion"], 3);
    }

    #[test]
    fn write_creates_structure_when_file_absent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        let mut m = HashMap::new();
        m.insert("openai".into(), "sk-1".into());
        write_secrets(&p, &m).unwrap();

        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(raw.contains("\"services\""));
        assert!(raw.contains("\"apiKey\""));
        assert!(raw.contains("\"openai\""));
    }

    #[test]
    fn write_output_is_pretty_2_space_indented() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        let mut m = HashMap::new();
        m.insert("openai".into(), "sk-1".into());
        write_secrets(&p, &m).unwrap();
        let raw = std::fs::read_to_string(&p).unwrap();
        // 对齐 inkos JSON.stringify(secrets, null, 2)
        assert!(
            raw.contains("\n  \"services\""),
            "expected 2-space indent, got: {raw}"
        );
    }

    #[test]
    fn write_uses_camelcase_apikey_field() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        let mut m = HashMap::new();
        m.insert("openai".into(), "sk-1".into());
        write_secrets(&p, &m).unwrap();
        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(raw.contains("\"apiKey\""), "must use camelCase apiKey");
        assert!(
            !raw.contains("api_key"),
            "must NOT use snake_case api_key"
        );
    }

    #[test]
    #[cfg(unix)]
    fn write_sets_0600_permissions_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        let mut m = HashMap::new();
        m.insert("openai".into(), "sk-1".into());
        write_secrets(&p, &m).unwrap();
        let mode = std::fs::metadata(&p)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "secrets.json must be 0600 on Unix");
    }

    #[test]
    fn write_merges_with_existing_does_not_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        let mut m1 = HashMap::new();
        m1.insert("a".into(), "1".into());
        m1.insert("b".into(), "2".into());
        write_secrets(&p, &m1).unwrap();

        // 二次 write 只更新 a：b 应保留（merge-upsert 语义）
        let mut m2 = HashMap::new();
        m2.insert("a".into(), "1-updated".into());
        write_secrets(&p, &m2).unwrap();

        let got = read_secrets(&p).unwrap();
        assert_eq!(got.get("a").unwrap(), "1-updated");
        assert_eq!(got.get("b").unwrap(), "2");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn write_replaces_non_object_service_entry() {
        // schema 不符的既有 entry（非 object）：write 应替换为合法 entry，而非崩溃。
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(&p, r#"{"services": {"broken": "not-an-object"}}"#);

        let mut m = HashMap::new();
        m.insert("broken".into(), "sk-fixed".into());
        write_secrets(&p, &m).unwrap();

        let got = read_secrets(&p).unwrap();
        assert_eq!(got.get("broken").unwrap(), "sk-fixed");
    }

    #[test]
    fn write_empty_map_to_fresh_file_produces_empty_services() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        let m = HashMap::new();
        write_secrets(&p, &m).unwrap();

        let got = read_secrets(&p).unwrap();
        assert!(got.is_empty());

        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(raw.contains("\"services\""));
    }
}
