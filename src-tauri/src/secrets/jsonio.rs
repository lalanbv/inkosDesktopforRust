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
//! ## C7 tri-state 区分（M3 批次 3）
//! 旧版 [`read_secrets`] 把"文件缺失"、"JSON 损坏"、"合法但 services 空"三者折叠为
//! 同一个 `Ok(empty)`，让 watcher 的删除传播兜底无法区分"用户真删全部 key"与
//! "磁盘瞬时损坏"——用户删唯一 service 写出合法 `{"services":{}}` 被误判为"损坏"
//! 跳过删除，keychain 残留导致重启 `sync_on_startup` path1 复活已删 key。
//!
//! 新 API [`read_secrets_state`] 在保持 [`read_secrets`] 行为兼容的同时，额外返回
//! [`SecretsFileState`]（Absent / Empty / Populated / Corrupt），让 watcher 兜底
//! 仅对 `Corrupt` 跳过删除（保守不删），对 `Empty` / `Absent` 正常传播 deletes
//! （视为用户合法意图：清空 / 删文件）。
//!
//! ## write 策略：merge-保留 + 0600 + 原子替换
//! - 先读既有文件得到完整 `serde_json::Value`，只覆盖 `services[*].apiKey`，
//!   保留 inkos 未来可能添加的其他字段（forward-compat）。
//! - pretty 2-space 缩进（对齐 inkos `JSON.stringify(secrets, null, 2)`）。
//! - `tempfile::NamedTempFile::new_in(parent)` + `.persist(path)` 原子替换：
//!   - 随机文件名（消除固定 `secrets.json.tmp` 的符号链接预创建攻击面）；
//!   - 创建即 0600（Unix）：消除 `fs::write` 默认权限的短暂可读窗口；
//!   - `.persist` 是原子 rename（同 filesystem 保证）。
//!
//! ## 错误处理
//! - 文件缺失 / JSON 损坏（read）→ `Ok(empty)`，不报错（对齐 inkos）。
//! - IO 失败 / 序列化失败（write）→ `Err`，显式向上传播（不静默吞错）。

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::Path;

/// 文件状态的 tri-state 分类，配合 [`read_secrets_state`] 使用。
///
/// **C7 修复背景**：旧版 [`read_secrets`] 把"文件缺失"、"JSON 损坏"、"合法但
/// services 空"折叠为同一个 `Ok(empty)`。这导致 [`process_writeback`]
/// 的删除传播兜底（`from_json.is_empty() && !from_store.is_empty()`）无法区分
/// "用户真删全部 key"与"磁盘瞬时损坏"——用户删唯一 service 后写出合法
/// `{"services":{}}` 被误判为"损坏"跳过删除，keychain 残留，下次启动
/// `sync_on_startup` path1 复活已删 key。
///
/// tri-state 让 watcher 兜底**仅**对 [`Corrupt`](Self::Corrupt) 跳过删除
/// （保守不删），对 [`Empty`](Self::Empty) / [`Absent`](Self::Absent) 正常传播
/// deletes（视为用户合法意图：清空 / 删文件）。
///
/// [`process_writeback`]: crate::secrets::sync::process_writeback
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretsFileState {
    /// 文件不存在（用户从未创建 / 显式删除）。
    ///
    /// watcher 视为"用户意图清空"→ 正常传播 deletes。
    Absent,
    /// 文件存在且 JSON 合法，但没有任何 `services[*].apiKey` 对（如
    /// `{"services": {}}`、`{"version": 42}`、或所有 entry 都缺 `apiKey`）。
    ///
    /// 视为**用户意图清空全部 secret** → 正常传播 deletes。
    Empty,
    /// 文件存在且至少有一个 `serviceId -> apiKey` 对。
    Populated,
    /// 文件存在但 JSON 不可解析（并发写截断、磁盘错乱）。
    ///
    /// watcher 兜底：保守跳过 deletes（防瞬时损坏导致 keychain 全部丢失）。
    Corrupt,
}

/// tri-state 版本的 [`read_secrets`]：返回解析得到的 map **和**文件状态，
/// 让调用方区分"合法清空"（[`Empty`](SecretsFileState::Empty)）与"瞬时损坏"
/// （[`Corrupt`](SecretsFileState::Corrupt)）。
///
/// 行为契约：
/// - 文件缺失 → `(empty, [`Absent`](SecretsFileState::Absent))`，不报错。
/// - JSON 解析失败 → `(empty, [`Corrupt`](SecretsFileState::Corrupt))`，不报错。
/// - 合法 JSON 但无 apiKey 对 → `(empty, [`Empty`](SecretsFileState::Empty))`。
/// - 合法 JSON 且 ≥1 apiKey 对 → `(map, [`Populated`](SecretsFileState::Populated))`。
/// - 底层 IO 失败（权限、磁盘错误） → `Err`（显式向上传播，不静默吞错）。
///
/// service entry 缺 `apiKey` 或值非 string → 该 entry 跳过（不影响其他 entry）。
///
/// 注：本层不区分"空字符串 apiKey"与"缺失 apiKey"——空串会被读出。
/// inkos `getServiceApiKey` 把空串视为未设置（`if (entry?.apiKey) return entry.apiKey`），
/// 该语义由上层 store 负责；jsonio 仅做 schema 解析。
pub fn read_secrets_state(path: &Path) -> Result<(HashMap<String, String>, SecretsFileState)> {
    let txt = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((HashMap::new(), SecretsFileState::Absent));
        }
        Err(e) => {
            return Err(e).with_context(|| format!("read secrets: {}", path.display()))
        }
    };
    let root: Value = match serde_json::from_str(&txt) {
        Ok(v) => v,
        Err(_) => return Ok((HashMap::new(), SecretsFileState::Corrupt)),
    };
    let map = collect_api_keys(&root);
    let state = if map.is_empty() {
        SecretsFileState::Empty
    } else {
        SecretsFileState::Populated
    };
    Ok((map, state))
}

/// 从 `secrets.json` 读取全部 `serviceId -> apiKey` 映射。
///
/// **向后兼容包装**：委托 [`read_secrets_state`] 并丢弃状态。等价于旧版语义
/// （文件缺失 / JSON 损坏 / services 空都返回 `Ok(empty)`）。
///
/// 需要区分"合法清空"与"瞬时损坏"的调用方（如 [`process_writeback`]
/// 的删除传播兜底）应改用 [`read_secrets_state`]。
///
/// [`process_writeback`]: crate::secrets::sync::process_writeback
pub fn read_secrets(path: &Path) -> Result<HashMap<String, String>> {
    let (map, _state) = read_secrets_state(path)?;
    Ok(map)
}

/// 从已解析的 JSON root 收集 `services[*].apiKey`。
/// 缺 `services` 字段 / 非 object services / entry 缺 apiKey → 空 map。
fn collect_api_keys(root: &Value) -> HashMap<String, String> {
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

/// 原子写：`tempfile::NamedTempFile::new_in(parent)` + `.persist(path)`。
///
/// **安全设计**（M2b 加固）：
/// - 随机文件名：消除固定 `secrets.json.tmp` 的符号链接预创建攻击面
///   （攻击者在父目录预创建同名符号链接指向敏感文件，旧实现 fs::write 会跟随符号链接写）。
/// - 创建即 0600（Unix）：`tempfile::Builder::permissions(0o600)` 在创建时即设置权限，
///   消除 `fs::write` 默认权限（受 umask 影响，通常 0644）的"短暂可读窗口"——
///   secret 内容从未以非 0600 状态落盘。Windows 无 0600 概念，跳过（NTFS ACL 另论）。
/// - `.persist(path)`：原子 rename（同 filesystem 保证；跨 filesystem 会 fall back 到
///   非原子拷贝，本场景 secrets.json 与其 tmp 同在 `.inkos/` 目录，不会跨 fs）。
///
/// **失败语义**：`NamedTempFile::new_in` / `write_all` / `persist` 任一失败 → 显式 `Err`，
/// `NamedTempFile` 即便 drop 未 persist，也会自动清理 tmp 文件（无垃圾残留）。
fn atomic_write_0600(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().with_context(|| {
        format!("atomic_write: path={} 无父目录", path.display())
    })?;

    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(PermissionsExt::from_mode(0o600));
    }
    let mut tmp = builder
        .tempfile_in(parent)
        .with_context(|| format!("创建 NamedTempFile 失败: {}", parent.display()))?;

    use std::io::Write;
    tmp.write_all(data)
        .with_context(|| format!("write tmp 失败: {}", tmp.path().display()))?;
    tmp.flush()
        .with_context(|| format!("flush tmp 失败: {}", tmp.path().display()))?;

    // persist 是 tempfile 3.x NamedTempFile 的 inherent 方法（同 fs 原子 rename）。
    // 跨 fs 会 fallback 到非原子拷贝，但本场景同目录不会跨。
    if let Err(e) = tmp.persist(path) {
        return Err(anyhow::anyhow!(
            "persist tmp -> {} 失败: {}",
            path.display(),
            e.error
        ));
    }
    Ok(())
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

        let got = read_secrets(p.as_path()).unwrap();
        assert_eq!(got.get("openai").unwrap(), "sk-1");
        assert_eq!(got.get("deepseek").unwrap(), "dk-1");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn read_missing_file_returns_empty_ok() {
        let p = Path::new("/nonexistent/path/secrets.json");
        let got = read_secrets(p).unwrap();
        assert!(got.is_empty(), "missing file should yield Ok(empty), not Err");
    }

    #[test]
    fn read_corrupt_json_returns_empty_ok() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(&p, "{ not valid json }}}}");
        let got = read_secrets(p.as_path()).unwrap();
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
        let got = read_secrets(p.as_path()).unwrap();
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
        let got = read_secrets(p.as_path()).unwrap();
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

        let got = read_secrets(p.as_path()).unwrap();
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

        let got = read_secrets(p.as_path()).unwrap();
        assert_eq!(got.get("broken").unwrap(), "sk-fixed");
    }

    #[test]
    fn write_empty_map_to_fresh_file_produces_empty_services() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        let m = HashMap::new();
        write_secrets(&p, &m).unwrap();

        let got = read_secrets(p.as_path()).unwrap();
        assert!(got.is_empty());

        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(raw.contains("\"services\""));
    }

    // ===================================================================
    // C7 修复：read_secrets_state tri-state（Absent / Empty / Populated / Corrupt）
    // 区分"用户合法清空"与"瞬时损坏"，让 watcher 兜底仅对 Corrupt 跳过删除。
    // ===================================================================

    #[test]
    fn read_state_missing_file_is_absent() {
        let p = Path::new("/nonexistent/path/secrets.json");
        let (map, state) = read_secrets_state(p).unwrap();
        assert!(map.is_empty());
        assert_eq!(state, SecretsFileState::Absent);
    }

    #[test]
    fn read_state_corrupt_json_is_corrupt() {
        // C7 核心：JSON 不可解析 → Corrupt（watcher 兜底将跳过删除）
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(&p, "{ broken not json }}}}");
        let (map, state) = read_secrets_state(p.as_path()).unwrap();
        assert!(map.is_empty());
        assert_eq!(state, SecretsFileState::Corrupt, "不可解析 JSON 必须分类为 Corrupt");
    }

    #[test]
    fn read_state_legal_empty_services_is_empty_not_corrupt() {
        // C7 回归核心：合法 {"services":{}} 必须分类为 Empty（用户意图清空），
        // 而非 Corrupt——否则 watcher 兜底误跳过删除导致 keychain 残留。
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(&p, r#"{"services":{}}"#);
        let (map, state) = read_secrets_state(p.as_path()).unwrap();
        assert!(map.is_empty());
        assert_eq!(state, SecretsFileState::Empty, "合法空 services 必须是 Empty，不能误判为 Corrupt");
    }

    #[test]
    fn read_state_missing_services_field_is_empty() {
        // 合法 JSON 但无 services 字段 → Empty（无 apiKey 对）
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(&p, r#"{"version": 42}"#);
        let (map, state) = read_secrets_state(p.as_path()).unwrap();
        assert!(map.is_empty());
        assert_eq!(state, SecretsFileState::Empty);
    }

    #[test]
    fn read_state_entries_all_missing_apikey_is_empty() {
        // services 存在但所有 entry 都缺 apiKey → Empty（无 apiKey 对）
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(&p, r#"{"services": {"broken": {}, "alsobroken": {}}}"#);
        let (map, state) = read_secrets_state(p.as_path()).unwrap();
        assert!(map.is_empty());
        assert_eq!(state, SecretsFileState::Empty);
    }

    #[test]
    fn read_state_with_apikey_is_populated() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.json");
        write_raw(&p, r#"{"services": {"openai": {"apiKey": "sk-x"}}}"#);
        let (map, state) = read_secrets_state(p.as_path()).unwrap();
        assert_eq!(map.get("openai").unwrap(), "sk-x");
        assert_eq!(state, SecretsFileState::Populated);
    }
}
