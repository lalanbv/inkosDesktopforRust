# M2b 设计 spec：secrets（keychain Route A）

> inkosDesktop Phase 1 M2b。基于架构 v1.2 §6.4（Route A）+ M2a spec 要点。
> 仓库：`/Users/lalanbv/GitProject/inkosDesktopforRust`（mono-repo，master 已含 M1+M2a）。

| 项 | 值 |
|---|---|
| spec 版本 | 1.0 |
| 日期 | 2026-08-06 |
| 状态 | 设计已定，待 writing-plans |
| 前置 | M1（supervisor/lifecycle/cleanup_sidecar）+ M2a（observer，已合并 master @ v0.2.0-m2a）|

## 1. 目标

兑现架构 §6.4「密钥不落盘明文」：OS keychain 为权威存储，启动时派生 `.inkos/secrets.json`(0600) 供 inkos 运行；用户经 **Studio SPA 既有密钥 UI** 改 key（零改 SPA），桌面壳监听 secrets.json 变更回写 keychain。全程 keychain 主、secrets.json 派生。

## 2. 模块设计（`src-tauri/src/secrets/`，<800 行/文件）

### 2.1 store.rs — SecretStore（跨平台 keychain）
- 用 `keyring` crate（macOS Keychain / Win Credential Manager / Linux Secret Service，统一 API）。
- `trait SecretStore { fn read_all(&self) -> Result<HashMap<String,String>>; fn upsert(&self, k:&str, v:&str) -> Result<()>; fn delete(&self, k:&str) -> Result<()>; }`
- 服务名常量 `inkosDesktop`；key = service id（如 `openai`/`deepseek`/`custom:foo`），value = apiKey。
- 测试：`MockStore`（in-memory HashMap）实现 trait，覆盖 read/upsert/delete round-trip。

### 2.2 jsonio.rs — secrets.json 读写（纯函数可测）
- `read_secrets(path) -> Result<HashMap<String,String>>`（解析 `.inkos/secrets.json` 的 services.*.apiKey 结构——读 inkos 的实际 schema 对齐，见 server.ts loadSecrets）
- `write_secrets(path, map) -> Result<()>`（写 0600，原子写：写临时文件 + rename）
- 测试：temp file round-trip；schema 对齐（与 inkos secrets.json 结构一致）。

### 2.3 sync.rs — 启动同步 + 首次迁移 + 回写 watcher
- **启动同步** `sync_on_startup(store, path)`：
  - 若 `store.read_all()` 非空 → `write_secrets(path, map)`（keychain 主，覆盖 secrets.json）。**写入前后置 `SYNCING` AtomicBool 标记，watcher 见标记跳过回写。**
  - 若 keychain 空 且 secrets.json 存在 → **首次迁移**：`read_secrets(path)` → `store.upsert(each)`（导入 keychain）。
- **回写 watcher** `spawn_writeback(store, path)`：
  - 用 `notify` crate 监听 secrets.json 变更；debounce 500ms。
  - 触发时：若 `SYNCING` 标记为真 → 跳过（programmatic write，防回环）；否则 `read_secrets(path)` → 与 store 比对 → `upsert` 差异（SPA 新增/改 key 回写 keychain）。
  - 标记在 sync_on_startup 写入后 reset 为假。
- 测试：MockStore + temp secrets.json → 模拟 programmatic write（设标记）watcher 不回写；模拟 SPA write（不设标记）watcher 回写 store。防回环回归测。

## 3. 数据流

```
启动：
  SecretStore.read_all (keychain)
    非空 → SYNCING=true → write_secrets(secrets.json, 0600) → SYNCING=false → inkos 消费
    空 + secrets.json 存在 → 首次迁移 read_secrets → store.upsert each → 再 sync
  spawn_writeback watcher（监听 secrets.json）
运行：
  用户在 Studio SPA 改 key → SPA 写 secrets.json → watcher(debounce, SYNCING=false) → store.upsert → keychain 更新
退出：watcher 随进程结束（或 shutdown 取消）
```

## 4. 错误处理与降级

| 故障 | 处置 |
|---|---|
| keychain 不可用（无 Secret Service/权限） | 降级：secrets.json 仍可用（明文 0600），UI 提示"密钥未加密存储"，不阻塞启动 |
| secrets.json 不存在 | keychain 为准，sync 时创建（或 inkos 自建） |
| watcher 失败（notify 错误） | log + 重试（指数退避）；不影响 inkos |
| 回写 keychain 失败 | log（secrets.json 已是运行态，inkos 不受影响）；下次启动再同步 |
| schema 不匹配（inkos secrets.json 结构变） | best-effort 解析，失败 log，不崩溃 |

无静默吞错：所有 fallible 路径显式 log。

## 5. 测试

- store.rs：MockStore round-trip（read/upsert/delete）。
- jsonio.rs：temp file 读写 round-trip + schema 对齐（构造 inkos 格式 secrets.json，解析出 apiKey map）。
- sync.rs：
  - 启动同步：keychain 非空 → 覆盖 secrets.json；keychain 空 + secrets.json 存在 → 首迁。
  - **防回环**：SYNCING=true 时 programmatic write 不触发回写；SYNCING=false 时 SPA write 回写。
  - debounce：连续多次写聚合一次回写。
- 集成：MockStore + temp dir 全流程（启动→SPA改→回写）。
- keyring 真实 keychain 路径标 `#[ignore]`（需 OS keychain，CI/真机）。
- 覆盖率 ≥80%（纯逻辑全覆盖；keyring/notify IO 尽力）。

## 6. 接口契约（供 writing-plans）

- `secrets::store::{SecretStore, KeyringStore, MockStore}`（trait + 生产 + 测试实现）
- `secrets::jsonio::{read_secrets, write_secrets}`（纯函数）
- `secrets::sync::{sync_on_startup, spawn_writeback}`
- 接入点：main.rs setup 在 spawn sidecar **之前** 调 `sync_on_startup`，spawn sidecar 后 `spawn_writeback`。
- 复用 M1：`paths`（项目根 → `.inkos/secrets.json` 路径）。

## 7. 与架构对齐

- §6.4 Route A（keychain 主 + SPA 管理 + 回写同步）✓
- §6.1 密钥不落盘明文（keychain 唯一持久存储；secrets.json 为运行态派生，0600）✓
- 零修改 inkos（复用 SPA 既有密钥 UI，不另建）✓

## 8. 依赖

- `keyring`（跨平台 OS 凭据）、`notify`（文件监听）。加 `src-tauri/Cargo.toml`。
