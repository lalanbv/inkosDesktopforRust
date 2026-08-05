//! Secret store abstraction: trait + MockStore (测试用) + KeyringStore (OS keychain 占位)。
//!
//! ## 背景
//! M2b Route A：把 API key 从 `.env` 文件迁移到 OS keychain（macOS Keychain /
//! Windows Credential Manager / Linux Secret Service），避免明文落盘。
//!
//! ## 设计
//! - `SecretStore` trait：`read_all` / `upsert` / `delete`，`Send + Sync`。
//! - `MockStore`：基于 `Mutex<HashMap>`，完整实现，供测试与开发期使用。
//! - `KeyringStore`：封装 `keyring` crate，本 task 仅占位（返回 `Err`），
//!   真实路径（含 `__index__` entry 管理 key 列表）由 Task 5（wiring）实装。

use anyhow::Result;
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

/// 生产实现：封装 `keyring` crate（macOS Keychain / Win Cred Manager / Linux Secret Service）。
///
/// **本 task 仅占位**：三个方法返回 `Err(anyhow!(...))`。
/// `keyring` crate 无枚举 API，真实实现需在 keychain 中维护一个 `__index__` entry
/// 存 key 列表（JSON 数组），由 Task 5（wiring）实装。
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }
}

impl SecretStore for KeyringStore {
    fn read_all(&self) -> Result<HashMap<String, String>> {
        Err(anyhow::anyhow!(
            "keyring read_all not implemented yet (service={}); deferred to wiring task",
            self.service
        ))
    }

    fn upsert(&self, key: &str, _value: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "keyring upsert not implemented yet (service={}, key={}); deferred to wiring task",
            self.service,
            key
        ))
    }

    fn delete(&self, key: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "keyring delete not implemented yet (service={}, key={}); deferred to wiring task",
            self.service,
            key
        ))
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
        assert!(after.get("openai").is_none());
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
}
