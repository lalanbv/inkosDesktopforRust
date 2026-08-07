# 修复：KeyringStore __index__ 并发竞态（防静默丢凭据）

> 日期：2026-08-07
> 范围：`src-tauri/src/secrets/store.rs`
> 类型：并发正确性缺陷（自审 secrets 模块发现）

## 缺陷

`KeyringStore` 的 `__index__`（key 名单元 entry）维护是**读-改-写**序列（`read_index` → 改 → `write_index`），但**无任何同步**。

`SecretStore: Send + Sync`，调用方在 `main.rs` 用 `Arc<dyn SecretStore>` 跨多线程 tokio 任务共享（`sync_on_startup` / `process_writeback` / 命令处理）。并发 `upsert`/`delete` 时：

```
线程 A upsert(k1)：read_index → [k0]，改 → [k0,k1]，write_index → [k0,k1]
线程 B upsert(k2)：read_index → [k0]   （A 未写回前读到旧值），改 → [k0,k2]，write_index → [k0,k2]
结果：[k0,k2] —— k1 丢失！
```

index 丢失的 key → `read_all`（按 index 逐个 `get_password`）**永远读不到** → **静默丢凭据**：用户保存的 API key 在重启/同步后消失。

`MockStore` 已用 `Mutex<HashMap>` 保护（测试安全），`KeyringStore` 却无保护——实现间不一致，真实生产实现反而有缺陷。

## 修复

`KeyringStore` 加 `index_lock: Mutex<()>`，在 `read_all` / `upsert` / `delete` 入口获取锁。私有 helper（`read_index`/`write_index`/`add_to_index`/`remove_from_index`）仅被这三个方法调用，守卫入口即足够序列化 index 的 RMW。

secrets 访问低频，锁开销可忽略；与 `MockStore` 的 Mutex 模式一致。

## 测试

新增 `keyring_real_concurrent_upsert_no_lost_keys`（`#[ignore]`，需 OS keychain）：20 线程并发 `upsert` 不同 key → 断言 `read_all` 见全部 20 个（无锁时会丢失）。本地 / CI-macOS runner 可跑；Linux headless 无 keychain → 默认 ignore。

MockStore 测试（4 个，CI 常跑）覆盖 trait 行为；KeyringStore 真实测试均 ignore（OS keychain 依赖）。

## 验证

- `cargo clippy --all-targets -- -D warnings`：clean
- `cargo test --lib secrets::`：**23 passed / 0 failed**（4 ignored 含新并发测试）
