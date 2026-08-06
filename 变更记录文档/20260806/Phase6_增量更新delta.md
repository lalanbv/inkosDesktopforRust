# Phase 6.1：增量更新——客户端 bsdiff delta 应用

> 日期：2026-08-06
> 提交：`4c4fad1e`
> 对应 Phase 6 规划 6.1 的客户端侧

## 目标

减少 engine bundle 更新下载体积。相邻版本高度相似时，下载 delta（差量）远小于全量 bundle。

## 交付物

### `updater/delta.rs`
```rust
pub fn apply_delta(old: &[u8], patch: &[u8], expected_sha256: &[u8]) -> Result<Vec<u8>>
```
- 应用 bsdiff delta 重建 new bundle
- **SHA256 校验通过才返回**，失败绝不返回部分结果（防篡改/防损坏）
- 纯计算无副作用，写盘时机交上层原子控制

### 完整更新流程（上层组合）
下载 delta →（解压）→ `apply_delta` → SHA256 校验 → 签名验证 → 原子替换。
本模块只做「应用 + 校验」这一步，其余由 updater 上层组合，职责单一。

## 依赖

`bsdiff = "0.2"`（space-wizards 维护）：
- 纯 **safe Rust**，无 C 依赖，跨平台行为一致
- 成熟算法（bsdiff/bspatch 的 Rust 移植）
- BSD-2-License

## 设计决策

### 为什么客户端只做 apply，不做 diff
- diff 计算密集，应由服务端预生成（每次发版生成 old→new delta，存 release 资产）
- 客户端只需 patch（轻量），减少终端 CPU/内存

### 为什么 SHA256 校验
- delta 可能下载损坏/被篡改
- apply 后校验 SHA256 == 发布约定的 expected，不匹配绝不上写
- 与 engine 通道既有的 SHA256 校验（M3a）一致

### 关于 delta 大小
bsdiff crate 不内置压缩（经典 bsdiff 推荐配合 bzip2）。delta 是否更小取决于：
1. 数据相邻版本的相似度
2. 是否对 delta 做压缩（服务端发布时可 zstd/bzip2 压缩 delta，客户端解压后 apply）

`apply_delta` 的契约是「正确还原 + 校验通过」，非「delta 更小」——后者是服务端 delta 管道的职责。

## 测试（5 个，全过）
- `test_apply_delta_reconstructs_new`：基本 roundtrip
- `test_apply_delta_rejects_wrong_hash`：哈希不匹配必拒绝（错误信息含 SHA256）
- `test_apply_delta_empty_old`：old 为空（首装/全量降级）
- `test_apply_delta_identical_old_new`：old==new（无变化发版）
- `test_apply_delta_realistic_bundle_roundtrip`：真实 bundle 形态数据 roundtrip

## 验证
- `cargo test --lib`：320 passed / 0 failed（含 5 delta 测试）
- `cargo clippy --all-targets -- -D warnings`：零告警

## 后续（服务端侧，独立工程）
- CI 在发版时为每对相邻版本生成 delta 资产（`engine-v1.7.2-to-v1.7.3.delta`）
- release 元数据声明可用的 delta（current → latest）
- 客户端检查更新时优先尝试 delta，无可用 delta 或校验失败则降级全量
