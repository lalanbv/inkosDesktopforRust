# engine bundle 签名密钥采购指引（M4c）

> 作用：给 engine 通道（自定义 updater）配置端到端 Ed25519 签名，防 MITM 替换 bundle。
> 与 shell 通道（`tauri-plugin-updater` 的 `TAURI_SIGNING_PRIVATE_KEY`）独立，可共用或分用密钥对。

## 一、生成密钥对（发布方，一次性）

在 `src-tauri` 下运行：

```bash
cargo run --example gen-keypair
```

输出（示例）：

```
私钥 → GitHub secret INKOS_ENGINE_SIGNING_KEY（保密，切勿入仓）:
a1b2c3...（64 hex 字符）

公钥 → 编译期 INKOS_ENGINE_PUBKEY（公开，内嵌客户端验证用）:
d4e5f6...（64 hex 字符）
```

**私钥绝对不入仓**——只配进 GitHub secret。公钥可公开（内嵌客户端二进制）。

## 二、配置 CI（签名发布）

### 2.1 设置 GitHub secret
仓库 Settings → Secrets and variables → Actions → New repository secret：
- Name: `INKOS_ENGINE_SIGNING_KEY`
- Value: 上一步的**私钥** hex

### 2.2 CI 自动签名（已配置）
`.github/workflows/desktop-build.yml` 的 `Sign engine bundle` 步骤已 gated：
- `INKOS_ENGINE_SIGNING_KEY` 存在 → `cargo build --bin sign-bundle` + 对 `engine-*.tar.gz` 签名 → 产出 `engine-*.tar.gz.sig` → 上传到 release
- secret 未配置 → `::warning::` 告警 + 跳过（客户端降级为仅 SHA256 校验）

## 三、配置客户端（强制验证）

构建客户端时注入公钥：

```bash
INKOS_ENGINE_PUBKEY=d4e5f6... cargo build --release
```

`updater/engine.rs` 的 `apply_inner` 用 `option_env!("INKOS_ENGINE_PUBKEY")` 读取：
- 公钥已配置 + release 有 `.sig` → **强制验证**，签名不匹配拒绝更新
- 公钥未配置 或 release 无 `.sig` → 过渡期跳过 + `tracing::warn`（不阻断既有更新）

部署侧配置公钥后即从「过渡期」转为「强制模式」。

## 四、密钥轮换

1. `cargo run --example gen-keypair` 生成新密钥对
2. 更新 GitHub secret `INKOS_ENGINE_SIGNING_KEY`（新私钥）
3. 更新客户端构建的 `INKOS_ENGINE_PUBKEY`（新公钥）→ 发新版客户端
4. 新版客户端发布后，旧 release 的 `.sig`（旧私钥签）对新客户端（新公钥）失效——
   故轮换需**先发新版客户端**，再切换 secret。过渡期可让新客户端同时接受新旧公钥
   （未来增强：`INKOS_ENGINE_PUBKEY` 支持逗号分隔多公钥）。

## 五、验证链路

```
发布方：gen-keypair → 私钥(secret) + 公钥(pubkey)
CI：    sign-bundle(engine.tar.gz, 私钥) → engine.tar.gz.sig → release
客户端：下载 bundle + .sha256 + .sig
        SHA256 校验 → 完整性（防损坏）
        Ed25519 verify(bundle, .sig, 公钥) → 来源认证（防篡改）
        通过 → 原子替换；失败 → 拒绝该 bundle
```

`tests/secure_update_integration.rs` 已端到端验证此链路（delta + 签名组合）。

## 六、工具清单

| 工具 | 位置 | 用途 |
|---|---|---|
| `gen-keypair` | `examples/gen-keypair.rs` | 生成密钥对（发布方本地） |
| `sign-bundle` | `src/bin/sign-bundle.rs` | 签 bundle（CI 用） |
| `sig::verify` | `updater/sig.rs` | 客户端验证（运行时） |
