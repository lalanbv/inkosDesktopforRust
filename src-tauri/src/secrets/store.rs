//! Secret store abstraction: trait + MockStore (测试用) + KeyringStore (OS keychain 真实实现)。
//!
//! ## 背景
//! M2b Route A：把 API key 从 `.env` 文件迁移到 OS keychain（macOS Keychain /
//! Windows Credential Manager / Linux Secret Service），避免明文落盘。
//!
//! ## 设计
//! - `SecretStore` trait：`read_all` / `upsert` / `delete`，`Send + Sync`。
//! - `MockStore`：基于 `Mutex<HashMap>`，完整实现，供测试与开发期使用。
//! - `KeyringStore`：封装 `keyring` crate（v3）。
//!
//! ## KeyringStore 的 `__index__` 设计
//! `keyring` crate **无枚举 API**（不能列出某 service 下的所有 entry）。
//! 为支持 `read_all`，本实现在同一 service 下维护一个名为 `__index__` 的
//! "元 entry"，其 value 是 JSON 数组（`["openai","deepseek"]`），记录所有
//! 已写入的 key 列表。`read_all` 先读 index、再按 index 逐个 `get_password`。
//!
//! 这引入两个权衡：
//! 1. **index 与 entry 可能不一致**（外部工具删 keychain 项、写入中途失败等）。
//!    `read_all` 对 index 中存在但 entry 缺失的 key 容错跳过（log），不报错。
//! 2. **`__index__` 自身是明文 key 名单**（不是 secret 值）。secret 值仍各自独立
//!    加密存储。攻击者拿到 keychain 只能知道"有哪些 service id"，不能拿到 key。
//!
//! ## 错误处理
//! keyring 调用失败（PlatformFailure / NoStorageAccess 等）→ 显式 `Err`，
//! 由调用层（`sync_on_startup` / `process_writeback` / main.rs setup）决定降级策略。
//! `NoEntry` 在语义允许的位置（`delete` / `read_all` 逐项 / index 缺失）视为成功或跳过。

use anyhow::Result;
use keyring::{Entry, Error as KeyringError};
use std::collections::HashMap;
use std::sync::Mutex;

/// 跨 OS 的 secret 存储抽象。
///
/// 所有方法返回 `anyhow::Result`，错误在调用层显式处理（不静默吞错）。
pub trait SecretStore: Send + Sync {
    /// 读取全部 secret（返回快照副本，调用方可自由修改）。
    fn read_all(&self) -> Result<HashMap<String, String>>;
    /// 插入或更新一个 key（key 已存在则覆盖）。
    fn upsert(&self, key: &str, value: &str) -> Result<()>;
    /// 删除一个 key（不存在视为成功，不报错）。
    fn delete(&self, key: &str) -> Result<()>;
}

/// 内存 mock 实现，供单元测试与开发期使用。
///
/// 用 `Mutex<HashMap>` 保护内部状态；所有方法返回克隆或副本，保持外部不可变语义。
pub struct MockStore {
    map: Mutex<HashMap<String, String>>,
}

impl MockStore {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MockStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for MockStore {
    fn read_all(&self) -> Result<HashMap<String, String>> {
        Ok(self.map.lock().expect("MockStore mutex poisoned").clone())
    }

    fn upsert(&self, key: &str, value: &str) -> Result<()> {
        self.map
            .lock()
            .expect("MockStore mutex poisoned")
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.map
            .lock()
            .expect("MockStore mutex poisoned")
            .remove(key);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// KeyringStore：keyring v3 封装（macOS Keychain / Win Cred Manager / Linux Secret Service）
// ---------------------------------------------------------------------------

/// keychain 中存储 key 列表的元 entry 名（不可作为业务 service id 使用）。
///
/// value 是 JSON 数组（`Vec<String>`），记录所有已写入的业务 key 名单。
/// 选择 `__index__` 而非 `_index_` / `index`：双下划线前缀降低与业务 key 撞名概率
/// （inkos 的 service id 来自 `llms.config.ts`，不会以 `__` 开头）。
const INDEX_KEY: &str = "__index__";

/// 生产实现：封装 `keyring` crate（macOS Keychain / Win Cred Manager / Linux Secret Service）。
///
/// `keyring` crate 无枚举 API，本实现在 keychain 中维护 `__index__` entry
/// 存 key 列表（JSON 数组），详见模块级文档的"`__index__` 设计"。
///
/// 所有 keyring 错误（`PlatformFailure` / `NoStorageAccess` 等）显式向上传播；
/// `NoEntry` 在语义允许的位置（`delete` / 逐项 `read_all`）视为成功或跳过。
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }

    /// 构造 keyring Entry（业务 key 或 `__index__` 元 entry）。
    ///
    /// `Entry::new` 仅在 platform backend 初始化失败时返回 Err
    /// （Linux 无 Secret Service、macOS Keychain 锁死等）。
    fn entry_for(&self, key: &str) -> Result<Entry> {
        Entry::new(&self.service, key).map_err(|e| {
            anyhow::anyhow!(
                "keyring Entry::new(service={}, key={key}) 失败: {e}",
                self.service
            )
        })
    }

    /// 读取 `__index__` entry 的 key 列表。
    ///
    /// - `__index__` 不存在（首次使用 / 已清空）→ 返回空 `Vec`（非 Err）。
    /// - `__index__` 存在但 JSON 损坏 → `Err`（数据完整性问题，不应静默吞错）。
    fn read_index(&self) -> Result<Vec<String>> {
        match self.entry_for(INDEX_KEY)?.get_password() {
            Ok(json) => serde_json::from_str::<Vec<String>>(&json)
                .map_err(|e| anyhow::anyhow!("keyring __index__ JSON 解析失败: {e}（原始: {json:?}）")),
            Err(KeyringError::NoEntry) => Ok(Vec::new()),
            Err(e) => Err(anyhow::anyhow!(
                "keyring __index__ get_password(service={}) 失败: {e}",
                self.service
            )),
        }
    }

    /// 全量覆盖写入 `__index__` entry。
    fn write_index(&self, keys: &[String]) -> Result<()> {
        let json = serde_json::to_string(keys)
            .map_err(|e| anyhow::anyhow!("keyring __index__ JSON 序列化失败: {e}"))?;
        self.entry_for(INDEX_KEY)?
            .set_password(&json)
            .map_err(|e| anyhow::anyhow!(
                "keyring __index__ set_password(service={}) 失败: {e}",
                self.service
            ))
    }

    /// 把 `key` 加入 `__index__`（已存在则幂等 no-op，保持顺序不变）。
    fn add_to_index(&self, key: &str) -> Result<()> {
        let mut keys = self.read_index()?;
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_string());
            self.write_index(&keys)?;
        }
        Ok(())
    }

    /// 把 `key` 从 `__index__` 移除（不存在则幂等 no-op）。
    fn remove_from_index(&self, key: &str) -> Result<()> {
        let mut keys = self.read_index()?;
        let before = keys.len();
        keys.retain(|k| k != key);
        if keys.len() != before {
            self.write_index(&keys)?;
        }
        Ok(())
    }
}

impl SecretStore for KeyringStore {
    fn read_all(&self) -> Result<HashMap<String, String>> {
        let keys = self.read_index()?;
        let mut out = HashMap::with_capacity(keys.len());
        for key in keys {
            match self.entry_for(&key)?.get_password() {
                Ok(value) => {
                    out.insert(key, value);
                }
                Err(KeyringError::NoEntry) => {
                    // 容错：index 标记 key 存在但 entry 缺失（手工删除 / 写入中途失败）。
                    // 跳过该 key，不返回 Err——保证 read_all 整体成功，让 sync 流程继续。
                    // 日志帮助排错；下次 upsert 该 key 会重建 entry。
                    eprintln!(
                        "[secrets] keyring read_all: key={key} 在 __index__ 但 entry 缺失，跳过"
                    );
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "keyring get_password(service={}, key={key}) 失败: {e}",
                        self.service
                    ));
                }
            }
        }
        Ok(out)
    }

    fn upsert(&self, key: &str, value: &str) -> Result<()> {
        // 1. 写 entry（即便后续 index 更新失败，secret 已落 keychain——下次 upsert 同 key 幂等覆盖）
        self.entry_for(key)?
            .set_password(value)
            .map_err(|e| anyhow::anyhow!(
                "keyring set_password(service={}, key={key}) 失败: {e}",
                self.service
            ))?;

        // 2. 更新 __index__（key 已存在则幂等 no-op）
        // index 更新失败时返回 Err——keychain 中 secret 已存在但 index 缺失会导致下次
        // read_all 漏读该 key。调用方可重试 add_to_index（幂等），不破坏已写入的 secret。
        self.add_to_index(key)?;
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        // 1. 删 entry（不存在视为成功，与 MockStore::delete 语义一致）
        // 注：keyring v3 的方法名是 `delete_credential`（v2 是 `delete_password`，v3 改名）。
        match self.entry_for(key)?.delete_credential() {
            Ok(()) => {}
            Err(KeyringError::NoEntry) => {
                // key 不存在：仍走 remove_from_index 修正 index（可能 index 残留了已删 key）
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "keyring delete_credential(service={}, key={key}) 失败: {e}",
                    self.service
                ));
            }
        }

        // 2. 从 __index__ 移除（不存在则幂等 no-op）
        self.remove_from_index(key)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_roundtrip() {
        let store = MockStore::new();
        store.upsert("openai", "sk-x").unwrap();
        store.upsert("deepseek", "dk").unwrap();

        let all = store.read_all().unwrap();
        assert_eq!(all.get("openai").unwrap(), "sk-x");
        assert_eq!(all.get("deepseek").unwrap(), "dk");
        assert_eq!(all.len(), 2);

        store.delete("openai").unwrap();
        let after = store.read_all().unwrap();
        assert!(!after.contains_key("openai"));
        assert_eq!(after.len(), 1);
    }

    #[test]
    fn mock_upsert_overwrites() {
        let store = MockStore::new();
        store.upsert("k", "v1").unwrap();
        store.upsert("k", "v2").unwrap();
        assert_eq!(store.read_all().unwrap().get("k").unwrap(), "v2");
    }

    #[test]
    fn mock_delete_missing_is_noop() {
        let store = MockStore::new();
        store.delete("nonexistent").unwrap();
        assert!(store.read_all().unwrap().is_empty());
    }

    #[test]
    fn mock_default_is_empty() {
        let store = MockStore::default();
        assert!(store.read_all().unwrap().is_empty());
    }

    // -----------------------------------------------------------------------
    // KeyringStore 真实 keychain 测试（#[ignore]：需 OS keychain 可达；
    // CI / headless 环境常无 keychain，默认不跑）
    // -----------------------------------------------------------------------
    // 跑法：`cargo test -- --ignored keyring_real`
    // macOS：首次会弹钥匙串授权；GitHub Actions macOS runner 可用，Linux 需 D-Bus +
    // gnome-keyring（CI 跑不动 → 默认 ignore）。
    //
    // service 名带 `__test__` 前缀 + 测试名后缀，避免与生产数据撞键。

    /// 生成唯一 service 名（每次测试独立 keychain namespace，避免互相污染）。
    fn unique_service(label: &str) -> String {
        // 用 PID + 时间戳保证并发与重跑唯一：
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("__test__inkosDesktop_{label}_{pid}_{nanos}")
    }

    /// 清理 service 下的所有 entry（按 __index__ 列表 + 兜底删 index 自身）。
    /// 测试结束（含失败）后调用，避免 keychain 残留测试垃圾。
    fn cleanup(store: &KeyringStore) {
        if let Ok(keys) = store.read_index() {
            for k in keys {
                let _ = store.delete(&k);
            }
        }
        // 兜底：__index__ 自身可能残留（delete 已含此步，失败再保险一次）
        // keyring v3：`delete_credential`（v2 是 `delete_password`，v3 改名）
        if let Ok(entry) = store.entry_for(INDEX_KEY) {
            let _ = entry.delete_credential();
        }
    }

    #[test]
    #[ignore]
    fn keyring_real_roundtrip_upsert_read_delete() {
        let service = unique_service("roundtrip");
        let store = KeyringStore::new(&service);

        // 初始空
        assert!(store.read_all().unwrap().is_empty(), "新 service 应为空");

        // upsert 两个 key
        store.upsert("openai", "sk-test-1").unwrap();
        store.upsert("deepseek", "dk-test-2").unwrap();

        // read_all 看到 2 条
        let all = store.read_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("openai").unwrap(), "sk-test-1");
        assert_eq!(all.get("deepseek").unwrap(), "dk-test-2");

        // upsert 覆盖
        store.upsert("openai", "sk-test-1-v2").unwrap();
        let all = store.read_all().unwrap();
        assert_eq!(all.get("openai").unwrap(), "sk-test-1-v2");
        assert_eq!(all.len(), 2, "覆盖不应增加 key 数");

        // delete 一个
        store.delete("openai").unwrap();
        let all = store.read_all().unwrap();
        assert!(!all.contains_key("openai"));
        assert_eq!(all.len(), 1);

        cleanup(&store);
    }

    #[test]
    #[ignore]
    fn keyring_real_delete_missing_is_noop() {
        let service = unique_service("delmissing");
        let store = KeyringStore::new(&service);

        // 空 service 下删除不存在的 key：不应 Err（与 MockStore::delete 语义一致）
        store.delete("never-existed").unwrap();
        assert!(store.read_all().unwrap().is_empty());

        cleanup(&store);
    }

    #[test]
    #[ignore]
    fn keyring_real_index_stale_entry_tolerated() {
        // 模拟 index 残留：手工往 __index__ 加一个不存在的 key，
        // read_all 应跳过、不报错。
        let service = unique_service("stale");
        let store = KeyringStore::new(&service);

        // 写一个真 key + 往 index 塞一个假 key
        store.upsert("real_key", "real_val").unwrap();
        {
            let mut keys = store.read_index().unwrap();
            keys.push("ghost_key".to_string());
            store.write_index(&keys).unwrap();
        }

        let all = store.read_all().unwrap();
        // ghost_key 被跳过（log），real_key 正常返回
        assert_eq!(all.len(), 1);
        assert_eq!(all.get("real_key").unwrap(), "real_val");
        assert!(!all.contains_key("ghost_key"));

        cleanup(&store);
    }
}
