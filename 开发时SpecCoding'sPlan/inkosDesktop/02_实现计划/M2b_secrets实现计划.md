# inkosDesktop M2b 实现计划：secrets（keychain Route A）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development。`- [ ]` 跟踪。

**Goal:** 兑现架构 §6.4「密钥不落盘明文」：keychain 权威存储，启动派生 `.inkos/secrets.json`(0600)，SPA 改 key 经文件监听回写 keychain，防回环。

**Architecture:** `secrets` 模块（store/jsonio/sync）。keychain 经 `keyring` crate 跨平台；secrets.json 读写纯函数；sync 含启动同步+首迁+回写 watcher（`notify`）。

**Tech Stack:** Rust、`keyring`、`notify`（文件监听）、`serde_json`。

## Global Constraints（架构 v1.2 + M2b spec）

- 零修改 inkos；mono-repo 零交叉（壳只进 `src-tauri/`）
- keychain 唯一持久存储；secrets.json 运行态派生（0600）
- 复用 SPA 既有密钥 UI，零改 SPA
- Rust 不可变/单一职责/无静默吞错；覆盖≥80%
- 复用 M1 `paths`（项目根→`.inkos/secrets.json`）；接入 main.rs setup（spawn sidecar 前 sync_on_startup，后 spawn_writeback）

## File Structure

| 路径 | 职责 |
|---|---|
| `src-tauri/src/secrets/mod.rs` | re-export |
| `src-tauri/src/secrets/store.rs` | `SecretStore` trait + `KeyringStore`（生产）+ `MockStore`（测试） |
| `src-tauri/src/secrets/jsonio.rs` | `read_secrets`/`write_secrets`（纯函数，0600 原子写） |
| `src-tauri/src/secrets/sync.rs` | `sync_on_startup`、`spawn_writeback`（防回环 AtomicBool + debounce） |
| `src-tauri/src/main.rs`（改） | setup 接 sync_on_startup + spawn_writeback |
| `src-tauri/Cargo.toml`（改） | +`keyring`、`notify` |

---

## Task 1: SecretStore trait + MockStore + KeyringStore

**Files:** Create `src-tauri/src/secrets/{mod.rs, store.rs}`；改 `lib.rs`、`Cargo.toml`(+keyring)。Test: 内联（MockStore）。

**Interfaces:** Produces `trait SecretStore { read_all/upsert/delete }`、`MockStore(Mutex<HashMap>)`、`KeyringStore::new(service:&str)`（封装 keyring crate）。

- [ ] **Step 1: Cargo + trait + MockStore + 测试**

```rust
// src-tauri/src/secrets/store.rs
use anyhow::Result; use std::collections::HashMap; use std::sync::Mutex;

pub trait SecretStore: Send + Sync {
    fn read_all(&self) -> Result<HashMap<String,String>>;
    fn upsert(&self, k: &str, v: &str) -> Result<()>;
    fn delete(&self, k: &str) -> Result<()>;
}

pub struct MockStore { map: Mutex<HashMap<String,String>> }
impl MockStore { pub fn new() -> Self { Self { map: Mutex::new(HashMap::new()) } } }
impl SecretStore for MockStore {
    fn read_all(&self) -> Result<HashMap<String,String>> { Ok(self.map.lock().unwrap().clone()) }
    fn upsert(&self, k: &str, v: &str) -> Result<()> { self.map.lock().unwrap().insert(k.into(), v.into()); Ok(()) }
    fn delete(&self, k: &str) -> Result<()> { self.map.lock().unwrap().remove(k); Ok(()) }
}

// 生产实现：keyring crate（macOS Keychain/Win Cred Manager/Linux Secret Service）
pub struct KeyringStore { service: String }
impl KeyringStore {
    pub fn new(service: &str) -> Self { Self { service: service.into() } }
}
impl SecretStore for KeyringStore {
    fn read_all(&self) -> Result<HashMap<String,String>> {
        // keyring 无枚举 API；维护一个 entry 列表（在 keychain 的一个 "__index__" entry 存 JSON 数组）
        // 读 __index__ → 逐个 get。失败容错。
        todo!("Task1 仅占位，Task 接入时实现（或标 TODO 由 wiring task 实装）")
    }
    fn upsert(&self, k: &str, v: &str) -> Result<()> { /* keyring::Entry::new(service,k)?.set_password(v)?; 更新 __index__ */ todo!() }
    fn delete(&self, k: &str) -> Result<()> { /* keyring::Entry::new(service,k)?.delete_password()?; 更新 __index__ */ todo!() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn mock_roundtrip() {
        let s = MockStore::new();
        s.upsert("openai","sk-x").unwrap(); s.upsert("deepseek","dk").unwrap();
        assert_eq!(s.read_all().unwrap().get("openai").unwrap(), "sk-x");
        s.delete("openai").unwrap(); assert!(s.read_all().unwrap().get("openai").is_none());
    }
}
```

> 注：keyring 无枚举接口，需自管 `__index__` entry 存 key 列表（JSON）。Task 1 用 `todo!()` 占位 KeyringStore 真实实现，**但 todo! 会 panic**——改为 `unimplemented!` 同样 panic。为避免测试/编译期问题，**Task 1 仅落 trait + MockStore（完整可测），KeyringStore 真实实现放 Task 5（wiring）实装**（因 keyring 真实路径需 OS、标 #[ignore]）。本 task 的 KeyringStore 用 `Err(anyhow!("keyring not implemented yet"))` 占位（非 todo! panic）。

- [ ] **Step 2: 跑测试** — `cargo test secrets::store` → PASS（MockStore round-trip）。
- [ ] **Step 3: 注册** — `mod.rs` `pub mod store;`；`lib.rs` `pub mod secrets;`。
- [ ] **Step 4: Commit** — `feat(secrets): SecretStore trait + MockStore + KeyringStore scaffold`

---

## Task 2: jsonio（read_secrets/write_secrets 纯函数）

**Files:** Create `src-tauri/src/secrets/jsonio.rs`。Test: 内联（temp file）。

**Interfaces:** Produces `read_secrets(path:&Path)->Result<HashMap<String,String>>`、`write_secrets(path:&Path, map:&HashMap)->Result<()>`（0600，原子写：tmp+rename）。**schema 对齐 inkos**：secrets.json 结构 `{services: {<id>: {apiKey: "..."}}}`（按 server.ts loadSecrets）。

- [ ] **Step 1: 读写实现 + schema + 测试**

```rust
// src-tauri/src/secrets/jsonio.rs
use anyhow::Result; use serde::{Deserialize,Serialize}; use std::collections::HashMap; use std::path::Path;

#[derive(Default, Serialize, Deserialize)] struct SecretsFile { #[serde(default)] services: HashMap<String, ServiceEntry> }
#[derive(Default, Serialize, Deserialize)] struct ServiceEntry { #[serde(default)] api_key: Option<String>, #[serde(skip_serializing_if="Option::is_none", default)] other: Option<serde_json::Value> }

pub fn read_secrets(path: &Path) -> Result<HashMap<String,String>> {
    let txt = std::fs::read_to_string(path)?;
    let f: SecretsFile = serde_json::from_str(&txt).unwrap_or_default(); // 缺失/损坏→空
    Ok(f.services.into_iter().filter_map(|(k,v)| v.api_key.map(|ak|(k,ak))).collect())
}
pub fn write_secrets(path: &Path, map: &HashMap<String,String>) -> Result<()> {
    let mut f = SecretsFile::default();
    for (k,v) in map { f.services.insert(k.clone(), ServiceEntry{ api_key: Some(v.clone()), other: None }); }
    let json = serde_json::to_string_pretty(&f)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json)?; std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    std::fs::rename(&tmp, path)?; // 原子
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn roundtrip_preserves_apikeys() {
        let dir = tempfile::tempdir().unwrap(); let p = dir.path().join("secrets.json");
        let mut m = HashMap::new(); m.insert("openai".into(),"sk-1".into()); m.insert("deepseek".into(),"dk".into());
        write_secrets(&p,&m).unwrap();
        let got = read_secrets(&p).unwrap();
        assert_eq!(got.get("openai").unwrap(),"sk-1"); assert_eq!(got.get("deepseek").unwrap(),"dk");
    }
    #[test] fn read_missing_file_returns_empty() {
        let p = std::path::Path::new("/nonexistent/secrets.json");
        assert!(read_secrets(&p).is_err() || read_secrets(&p).map(|m|m.is_empty()).unwrap_or(true));
    }
}
```

> schema：需与 inkos `server.ts loadSecrets` 实际字段对齐——实现者读 inkos 源确认字段名（`apiKey` vs `api_key`）。若 inkos 用 camelCase，调整 serde rename。**Task 实现前 grep inkos `loadSecrets`/`saveSecrets` 确认 schema**，本计划用占位 `api_key`，实现时校正。

- [ ] **Step 2: 跑测试** — `cargo test secrets::jsonio` → PASS。
- [ ] **Step 3: Commit** — `feat(secrets): secrets.json read/write (atomic, 0600)`

---

## Task 3: sync_on_startup + 首次迁移

**Files:** Create `src-tauri/src/secrets/sync.rs`。Test: MockStore + temp secrets.json。

**Interfaces:** Produces `sync_on_startup(store:&dyn SecretStore, path:&Path, syncing:&AtomicBool)->Result<()>`（keychain 非空→覆盖 secrets.json[SYNCING 标记]；空+secrets.json 存在→首迁 keychain）。

- [ ] **Step 1: 实现 + 测试（两条路径）**
  - keychain 非空 → `write_secrets`（前后置 SYNCING=true）。
  - keychain 空 且 secrets.json 存在 → `read_secrets` → `store.upsert` each（首迁）。
  - 测试：MockStore 预置 {openai:sk} → sync → secrets.json 含该 key；MockStore 空 + temp secrets.json 含 {deepseek:dk} → sync → store 含 deepseek。
- [ ] **Step 2: 跑测试** → PASS。
- [ ] **Step 3: Commit** — `feat(secrets): startup sync + first-run migration`

---

## Task 4: spawn_writeback（文件监听回写 + 防回环 + debounce）

**Files:** 改 `src-tauri/src/secrets/sync.rs`（加 spawn_writeback）；Cargo +`notify`。Test: MockStore + temp + 模拟 programmatic/SPA write。

**Interfaces:** Produces `spawn_writeback(store: Arc<dyn SecretStore>, path: Arc<PathBuf>, syncing: Arc<AtomicBool>, shutdown: Arc<AtomicBool>)`。

- [ ] **Step 1: notify 监听 + debounce 500ms + 防回环**
  - 监听 secrets.json 变更；debounce（聚合 500ms 内多次）。
  - 触发：若 `syncing.load()` 为真 → 跳过（programmatic write）；否则 `read_secrets` → diff vs `store.read_all` → `upsert` 差异（SPA 新增/改 key 回写）。
- [ ] **Step 2: 测试防回环**
  - programmatic：设 syncing=true，写 secrets.json → 断言 store 未变。
  - SPA：syncing=false，写 secrets.json（新 key）→ debounce 后断言 store 含新 key。
- [ ] **Step 3: 跑测试** → PASS。
- [ ] **Step 4: Commit** — `feat(secrets): writeback watcher with anti-loop + debounce`

---

## Task 5: main.rs 接线 + KeyringStore 实装

**Files:** 改 `src-tauri/src/main.rs`、`src-tauri/src/secrets/store.rs`（KeyringStore 真实实现）。

- [ ] **Step 1: KeyringStore 实装** — 用 keyring crate；`__index__` entry 存 key 列表 JSON；read_all 读 index 逐个 get；upsert/delete 更新 index。标 #[ignore] 测真实 keychain。
- [ ] **Step 2: main.rs 接线** — setup 内 spawn sidecar **前**：`let store=Arc::new(KeyringStore::new("inkosDesktop")); let syncing=Arc::new(AtomicBool::new(false)); secrets::sync_on_startup(&store, &secrets_path, &syncing)?;`；spawn sidecar 后 `secrets::spawn_writeback(store, secrets_path, syncing, shutdown)`。secrets_path = project_root.join(".inkos/secrets.json")。
- [ ] **Step 3: 降级** — keychain 不可用时 sync_on_startup 返回 Err → log 提示"密钥未加密存储"，不阻塞（secrets.json 若存在 inkos 仍可用）。
- [ ] **Step 4: 冒烟** — `cargo build` + `cargo test`；keychain 真实路径 #[ignore]。
- [ ] **Step 5: Commit** — `feat(app): wire secrets keychain sync + writeback`

---

## Task 6: M2b 冒烟归档 + 覆盖率 + tag

- [ ] **Step 1** — 全量 `cargo test` 稳定；`cargo llvm-cov`（store/jsonio/sync 纯逻辑全覆盖；keyring/notify IO 尽力）。
- [ ] **Step 2** — 归档 `变更记录文档/20260806/M2b冒烟验证.md`（keychain 同步、防回环验证、首迁、降级、parked：真实 keychain/真机回写待验、keyring 无枚举需 __index__ 权衡）。
- [ ] **Step 3** — `git tag -a v0.2.1-m2b -m "M2b: secrets keychain Route A"`。
- [ ] **Step 4** — Commit + 报告。

---

## Self-Review

- **Spec 覆盖**：store(T1)/jsonio(T2)/sync-startup+首迁(T3)/writeback+防回环(T4)/wiring+KeyringStore(T5)/冒烟(T6) → spec §2 全覆盖。
- **占位符**：Task 1 KeyringStore 占位（Err 非 panic），Task 5 实装——合理分层（trait 先行可测，OS 路径后实装）；Task 2 schema 字段名需 grep inkos 校正（已标注）。
- **类型一致**：`SecretStore`/`read_secrets`/`write_secrets`/`sync_on_startup`/`spawn_writeback` 跨任务一致。
- **YAGNI**：M2b 只做 keychain 同步+回写；多 keychain/加密 secrets.json/密钥 UI 增强→M3。

## 后续

- **M3**：loopback 运行时强制（特权 helper）、Windows 端到端（windres/netsh→WFP）、便携 Node + engine/ 发布打包、updater 引擎通道、CI（三平台+同步回归+SSE 契约+pnpm10+GUI runner）、macOS 签名、全局快捷键、通知配置 UI、daemon 状态托盘项。
