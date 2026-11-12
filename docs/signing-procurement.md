# engine bundle 签名密钥采购指引（M4c 立项；994 号对齐本地发版口径）

> 作用：给 engine 通道（自定义 updater）配置端到端 Ed25519 签名，防 MITM 替换 bundle。
> 与 shell 通道（`tauri-plugin-updater` 的 `TAURI_SIGNING_PRIVATE_KEY`）独立，可共用或分用密钥对。
>
> **994 号勘误**：本指引原为 GitHub Actions CI 自动签名而写（secret 配置 +
> `desktop-build.yml` gated 步骤）。本仓 2026-08-23 起全量移除 workflows
> （发版走本地脚本），签名相应改为**发布方本地手动签名**——工具链不变
> （gen-keypair / sign-bundle / sig::verify / secure_update_integration 端到端
> 测试全部健在），仅「谁在何时调用 sign-bundle」从 CI 变为发布者本人。

## 一、生成密钥对（发布方，一次性）

在 `src-tauri` 下运行：

```bash
cargo run --example gen-keypair
```

输出（示例）：

```
私钥（保密，切勿入仓）:
a1b2c3...（64 hex 字符）

公钥 → 编译期 INKOS_ENGINE_PUBKEY（公开，内嵌客户端验证用）:
d4e5f6...（64 hex 字符）
```

**私钥保管约束（安全红线）**：
- 私钥**绝对不入仓**、不进 shell 历史（避免明文落盘到 `~/.bash_history`）；
- 从**环境变量或本机密钥服务**读取传入 sign-bundle（一次性 `read -s` 或
  密钥管理工具注入），不要写进任何脚本/配置文件；
- 公钥可公开（内嵌客户端二进制）。

## 二、本地签名发布流程（994 号口径）

发版打包产出 `engine-*.tar.gz` 后，发布方本机手动签名：

```bash
cd src-tauri
cargo build --release --bin sign-bundle

# 私钥经 env 注入（勿写进命令行字面量——防 shell 历史泄漏）
read -rs INKOS_SIGNING_KEY && export INKOS_SIGNING_KEY
target/release/sign-bundle /path/to/engine-2.x.y.tar.gz "$INKOS_SIGNING_KEY"
unset INKOS_SIGNING_KEY
```

产出 `engine-2.x.y.tar.gz.sig`（签名 hex 文本，与 `.sha256` 资产格式一致）。
发布时 **bundle + `.sha256` + `.sig` 三件同传 release 附件**。

不带 `.sig` 发布 = 客户端降级为仅 SHA256 校验（过渡期语义，见下）——正式
release 应始终签名。

## 三、配置客户端（强制验证）

构建客户端时注入公钥：

```bash
INKOS_ENGINE_PUBKEY=d4e5f6... cargo build --release
```

`updater/engine.rs` 的 `apply_inner` 用 `option_env!("INKOS_ENGINE_PUBKEY")` 读取：
- 公钥已配置 + release 有 `.sig` → **强制验证**，签名不匹配拒绝更新
- 公钥未配置 或 release 无 `.sig` → 过渡期跳过 + `tracing::warn`（不阻断既有更新）

部署侧配置公钥后即从「过渡期」转为「强制模式」（fail-closed：配置公钥即
表达「只接受签名 bundle」）。

## 四、密钥轮换

1. `cargo run --example gen-keypair` 生成新密钥对
2. 发布方本机保管新私钥（替换旧私钥保管位；旧私钥销毁）
3. 更新客户端构建的 `INKOS_ENGINE_PUBKEY`（新公钥）→ 发新版客户端
4. 新版客户端发布后，旧 release 的 `.sig`（旧私钥签）对新客户端（新公钥）失效——
   故轮换需**先发新版客户端**，再切换签名私钥。过渡期可让新客户端同时接受新旧公钥
   （未来增强：`INKOS_ENGINE_PUBKEY` 支持逗号分隔多公钥）。

## 五、验证链路

```
发布方：gen-keypair → 私钥(本地保管) + 公钥(pubkey)
发版：  sign-bundle(engine.tar.gz, 私钥) → engine.tar.gz.sig → release 附件
客户端：下载 bundle + .sha256 + .sig
        SHA256 校验 → 完整性（防损坏）
        Ed25519 verify(bundle, .sig, 公钥) → 来源认证（防篡改）
        通过 → 原子替换；失败 → 拒绝该 bundle
```

`tests/secure_update_integration.rs` 已端到端验证此链路（delta + 签名组合）。

## 六、工具清单

| 工具 | 位置 | 用途 |
|---|---|---|
| `gen-keypair` | `src-tauri/examples/gen-keypair.rs` | 生成密钥对（发布方本地） |
| `sign-bundle` | `src-tauri/src/bin/sign-bundle.rs` | 签 bundle（发布方本地发版时手动调用） |
| `sig::verify` | `src-tauri/src/updater/sig.rs` | 客户端验证（运行时） |
