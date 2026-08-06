# Phase 6.5：Ed25519 签名验证（engine bundle 来源认证）

> 日期：2026-08-06
> 提交：`7cbade49`

## 目标

engine 通道（自定义 updater）此前用 SHA256 防损坏，但 SHA256 不防主动篡改——攻击者可同时替换 bundle 与 `.sha256`。Ed25519 签名用发布方私钥签 bundle，客户端内嵌公钥验证，即使下载通道被 MITM 也无法伪造。

## 交付物

### `updater/sig.rs`
```rust
pub fn verify(data: &[u8], sig: &[u8], pubkey: &[u8]) -> Result<()>
```
- Ed25519 验证（`ed25519-dalek` 2.x，纯 Rust）
- 长度不符提前 bail（明确错误，非底层 panic）
- `SIG_LEN=64` / `PUBKEY_LEN=32` 常量

### 与 shell 通道对称
shell 通道（应用自身更新）已用 `tauri-plugin-updater` 的 Ed25519（pubkey 在 `tauri.conf.json`）。本模块给 engine bundle 提供对称能力。

## 测试（5，全过）
- 合法签名通过
- 篡改数据拒绝
- 错误公钥拒绝
- 长度校验（公钥/签名长度错）
- 空数据边界

## 当前状态 / 后续
- **已交付**：可独立测试的 `verify` 纯函数（325 lib test / 0 failed）
- **后续接入**（需运维约定）：release 附 `.sig` 资产 + 公钥内嵌 `engine.rs`，在 `EngineChannel::apply` 的 SHA256 校验后加 `sig::verify` 步骤

## 依赖
`ed25519-dalek = "2"`（features: rand_core）；dev-dep `rand = "0.8"`（测试生成密钥对）

---

## Phase 6 整体完成度（截至本次）

| 子项 | 状态 | 交付 |
|---|---|---|
| 6.1 增量更新（客户端 delta） | ✅ | `updater/delta.rs`（bsdiff apply + SHA256 校验，5 测试） |
| 6.2 遥测 | ✅ | 插件 `PluginMetrics` + 项目 `ProjectMetrics`（对称），前端面板展示 |
| 6.4 前端管理面板 | ✅ | `settings.html` + picker 入口 + 托盘入口（全生命周期可达） |
| 6.5 签名验证 | ✅ | `updater/sig.rs`（Ed25519 verify，5 测试） |
| 6.3 WASM Component Model 完整执行 | ⏳ | wit + bindgen + 链接器，周级工程，进程隔离已提供可执行路径 |

Phase 6 的 5 个候选项已完成 4 个（6.1/6.2/6.4/6.5），仅 6.3（WASM 完整执行）为剩余大工程。进程隔离插件路径已端到端验证可执行，6.3 是安全隔离的增强，非功能阻塞项。
