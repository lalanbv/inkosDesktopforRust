//! 启动同步：keychain ↔ secrets.json 双向对齐。
//!
//! ## 职责
//! - `sync_on_startup`：启动时一次性同步，保证 keychain 与 secrets.json 一致。
//! - 两条路径（互斥）：
//!   1. keychain 非空 → keychain 主，覆盖 secrets.json（写文件前后置 SYNCING 标记，
//!      防 watcher 回写）。
//!   2. keychain 空 且 secrets.json 非空 → 首次迁移：secrets.json → keychain。
//! - 两者都空 → 无操作（Ok）。
//!
//! ## 错误处理
//! - `read_all` / `read_secrets` / `write_secrets` / `upsert` 失败 → 显式向上传播
//!   （不静默吞错）。
//! - SYNCING 标记在 path 1（keychain → json）下保证复位：即便 `write_secrets` 失败
//!   也复位为 false，避免 watcher 永久禁用。
//! - path 2（首迁）中途 `upsert` 失败 → 已迁移的 key 保留，函数返回 Err；
//!   调用方可在排错后重试（幂等：再次 `upsert` 同 key 只是覆盖）。

use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::secrets::jsonio::{read_secrets, write_secrets};
use crate::secrets::store::SecretStore;

/// 启动时同步 keychain 与 secrets.json。
///
/// 行为契约（与 [`crate::secrets`] 模块文档一致）：
/// - **keychain 非空** → 用 keychain 内容覆盖 secrets.json。
///   写文件前设 `syncing=true`，写完后设 `syncing=false`（防 watcher 回写）。
///   即便 `write_secrets` 失败也保证 `syncing` 复位（避免 watcher 永久禁用）。
/// - **keychain 空 且 secrets.json 非空** → 首次迁移：把 secrets.json 中每个
///   `(serviceId, apiKey)` 调用 `store.upsert` 写入 keychain。
///   中途 `upsert` 失败 → 已写入的 key 保留，返回 Err（调用方可重试，幂等）。
/// - **两者都空** → 无操作（Ok）。
///
/// `path` 指向 `.inkos/secrets.json`（由调用方传入，通常是
/// `project_root.join(".inkos/secrets.json")`）。
pub fn sync_on_startup(store: &dyn SecretStore, path: &Path, syncing: &AtomicBool) -> Result<()> {
    let from_keychain = store.read_all()?;

    if !from_keychain.is_empty() {
        // path 1：keychain → secrets.json（keychain 主，覆盖文件）
        syncing.store(true, Ordering::SeqCst);
        let write_result = write_secrets(path, &from_keychain);
        // 不论成功失败都复位 SYNCING，避免 watcher 永久禁用（不静默吞错：
        // write 错误仍向下传播，只是先复位标记再返回 Err）
        syncing.store(false, Ordering::SeqCst);
        write_result?;
        return Ok(());
    }

    // keychain 空 → 尝试从 secrets.json 首迁
    let from_json = read_secrets(path)?;
    if from_json.is_empty() {
        // 两者都空：无操作
        return Ok(());
    }

    // path 2：secrets.json → keychain（首次迁移）
    for (service_id, api_key) in from_json {
        store.upsert(&service_id, &api_key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::store::MockStore;
    use std::fs;

    /// 辅助：把原始字符串写入指定路径（用于构造既有 secrets.json 测试夹具）。
    fn put_raw(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
    }

    /// 辅助：读 secrets.json 原始内容（断言用）。
    fn read_raw(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    // ---------- path 1：keychain 非空 → 覆盖 secrets.json ----------

    #[test]
    fn keychain_nonempty_writes_to_secrets_json_and_resets_syncing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        // 注意：secrets.json 初始不存在——write_secrets 会创建

        let store = MockStore::new();
        store.upsert("openai", "sk-x").unwrap();

        let syncing = AtomicBool::new(false);
        sync_on_startup(&store, &path, &syncing).unwrap();

        // secrets.json 含 openai
        let got = read_secrets(&path).unwrap();
        assert_eq!(got.get("openai").unwrap(), "sk-x");
        assert_eq!(got.len(), 1);

        // SYNCING 在 write 后复位为 false
        assert!(
            !syncing.load(Ordering::SeqCst),
            "syncing must reset to false after write"
        );

        // keychain 未被修改（source 不变）
        let chain = store.read_all().unwrap();
        assert_eq!(chain.get("openai").unwrap(), "sk-x");
    }

    #[test]
    fn keychain_nonempty_overwrites_existing_json_with_keychain_as_source_of_truth() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        // 既有 secrets.json 含 deepseek（将不出现于结果，因 keychain 主、覆盖语义由
        // write_secrets 的 merge-upsert 实现：keychain 中不存在的 key 会保留——
        // 但本测试 keychain 只放 openai，write_secrets 会保留 deepseek；
        // 真正"覆盖"语义指 keychain 中有的 key 以 keychain 为准）
        put_raw(
            &path,
            r#"{"services": {"deepseek": { "apiKey": "dk-old" }}}"#,
        );

        let store = MockStore::new();
        store.upsert("openai", "sk-x").unwrap();

        let syncing = AtomicBool::new(false);
        sync_on_startup(&store, &path, &syncing).unwrap();

        let got = read_secrets(&path).unwrap();
        // keychain 中的 openai 写入
        assert_eq!(got.get("openai").unwrap(), "sk-x");
        // 既有 deepseek 因 write_secrets merge-upsert 语义保留（jsonio 层契约）
        assert_eq!(got.get("deepseek").unwrap(), "dk-old");
    }

    #[test]
    fn keychain_nonempty_multiple_keys_all_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");

        let store = MockStore::new();
        store.upsert("openai", "sk-1").unwrap();
        store.upsert("deepseek", "dk-2").unwrap();
        store.upsert("anthropic", "ak-3").unwrap();

        let syncing = AtomicBool::new(false);
        sync_on_startup(&store, &path, &syncing).unwrap();

        let got = read_secrets(&path).unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got.get("openai").unwrap(), "sk-1");
        assert_eq!(got.get("deepseek").unwrap(), "dk-2");
        assert_eq!(got.get("anthropic").unwrap(), "ak-3");
    }

    // ---------- path 2：keychain 空 + secrets.json 非空 → 首迁 ----------

    #[test]
    fn keychain_empty_migrates_from_existing_secrets_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(
            &path,
            r#"{"services": {"deepseek": { "apiKey": "dk-x" }}}"#,
        );

        let store = MockStore::new();
        let syncing = AtomicBool::new(false);
        sync_on_startup(&store, &path, &syncing).unwrap();

        // MockStore 含 deepseek（首迁成功）
        let chain = store.read_all().unwrap();
        assert_eq!(chain.get("deepseek").unwrap(), "dk-x");
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn first_migration_multiple_keys_all_upserted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(
            &path,
            r#"{"services": {
                "openai": { "apiKey": "sk-1" },
                "deepseek": { "apiKey": "dk-2" },
                "anthropic": { "apiKey": "ak-3" }
            }}"#,
        );

        let store = MockStore::new();
        sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();

        let chain = store.read_all().unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain.get("openai").unwrap(), "sk-1");
        assert_eq!(chain.get("deepseek").unwrap(), "dk-2");
        assert_eq!(chain.get("anthropic").unwrap(), "ak-3");
    }

    #[test]
    fn first_migration_corrupt_json_is_noop_keychain_stays_empty() {
        // 损坏 JSON → read_secrets 返回 Ok(empty)（jsonio 契约）→ 两者都空 → 无操作
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(&path, "{ not valid json }}}}");

        let store = MockStore::new();
        sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();

        // keychain 仍空
        assert!(store.read_all().unwrap().is_empty());
    }

    // ---------- path 3：两者都空 → 无操作 ----------

    #[test]
    fn both_empty_is_noop_file_not_created_store_stays_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        // secrets.json 不存在；keychain 空

        let store = MockStore::new();
        sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();

        // secrets.json 未被创建
        assert!(
            !path.exists(),
            "both-empty case must not create secrets.json"
        );
        // keychain 仍空
        assert!(store.read_all().unwrap().is_empty());
    }

    #[test]
    fn both_empty_with_existing_empty_json_is_noop() {
        // secrets.json 存在但 services 为空 → read_secrets 返回 empty → 无操作
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(&path, r#"{"services": {}}"#);

        let store = MockStore::new();
        sync_on_startup(&store, &path, &AtomicBool::new(false)).unwrap();

        // keychain 仍空
        assert!(store.read_all().unwrap().is_empty());
        // 文件内容不变（无 write 调用）
        let raw = read_raw(&path);
        assert!(raw.contains("\"services\""));
    }

    // ---------- 错误路径 ----------

    #[test]
    fn write_failure_propagates_error_and_resets_syncing() {
        // 用一个父目录不存在的路径触发 write_secrets 失败（atomic_write 写 tmp 失败）
        let path = std::path::Path::new("/nonexistent/dir/secrets.json");

        let store = MockStore::new();
        store.upsert("openai", "sk-x").unwrap();

        let syncing = AtomicBool::new(false);
        let result = sync_on_startup(&store, &path, &syncing);

        // 错误显式传播（不静默吞错）
        assert!(result.is_err(), "write failure must propagate as Err");
        // SYNCING 即便失败也复位（避免 watcher 永久禁用）
        assert!(
            !syncing.load(Ordering::SeqCst),
            "syncing must reset to false even when write fails"
        );
    }

    #[test]
    fn syncing_flag_not_touched_in_first_migration_path() {
        // path 2 不操作 syncing（无文件写入，watcher 不会触发）
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        put_raw(
            &path,
            r#"{"services": {"deepseek": { "apiKey": "dk-x" }}}"#,
        );

        let store = MockStore::new();
        let syncing = AtomicBool::new(true); // 初始故意设 true
        sync_on_startup(&store, &path, &syncing).unwrap();

        // syncing 不被 path 2 触碰——仍是 true（调用方语义：仅 path 1 管理 syncing）
        assert!(
            syncing.load(Ordering::SeqCst),
            "syncing flag must not be touched by first-migration path"
        );
    }

    // ---------- 集成：与 jsonio round-trip 一致 ----------

    #[test]
    fn sync_roundtrip_keychain_to_json_and_back() {
        // 端到端：keychain → secrets.json；清空 MockStore；从 secrets.json 首迁回 keychain。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");

        // stage 1：keychain → json
        let store_src = MockStore::new();
        store_src.upsert("openai", "sk-rt").unwrap();
        store_src.upsert("deepseek", "dk-rt").unwrap();
        sync_on_startup(&store_src, &path, &AtomicBool::new(false)).unwrap();

        // stage 2：清空 keychain 后从 json 首迁
        let store_dst = MockStore::new();
        sync_on_startup(&store_dst, &path, &AtomicBool::new(false)).unwrap();

        let chain = store_dst.read_all().unwrap();
        assert_eq!(chain.get("openai").unwrap(), "sk-rt");
        assert_eq!(chain.get("deepseek").unwrap(), "dk-rt");
        assert_eq!(chain.len(), 2);
    }
}
