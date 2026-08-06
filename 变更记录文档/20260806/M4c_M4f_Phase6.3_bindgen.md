# M4c + M4f + Phase 6.3 进展

> 日期：2026-08-06
> 提交：`21574572`(M4c)、`49818ba3`(M4f)、`7e7aa92d`(6.3 bindgen)

## M4c：engine bundle 端到端 Ed25519 签名 ✅

engine 通道（自定义 updater）此前仅 SHA256（防损坏），补 Ed25519 来源认证（防 MITM），与 shell 通道（tauri-plugin-updater）对称。

### 交付
- `updater/sig.rs` 加 `sign()` + `encode_hex/decode_hex`（发布方+客户端共用）+ 4 新测试
- `src/bin/sign-bundle.rs`：签名 CLI（读私钥 hex + bundle → `.sig` hex），端到端测试
- `examples/gen-keypair.rs`：密钥对生成（发布方本地，不进 release）
- `updater/engine.rs apply_inner`：SHA256 后加签名验证（`option_env!("INKOS_ENGINE_PUBKEY")` 编译期注入；公钥+sig 齐备→强制，否则过渡期 warn 放行）
- `.github/workflows/desktop-build.yml`：`Sign engine bundle` 步骤（gated by `INKOS_ENGINE_SIGNING_KEY`）+ 上传 `.sig`
- `docs/signing-procurement.md`：密钥生成/CI secret/客户端注入/轮换指引

### 安全语义
SHA256（完整性，已有）+ Ed25519（来源认证，本次）= 防损坏 + 防篡改。过渡期容错，部署侧配置公钥后转强制。

## M4f：Node SEA 调研 + 决策 ✅（不采用）

### 调研结论
**不采用 SEA**，维持 tar.gz bundle（M3）+ bsdiff delta（6.1）。

核心论据：
- SEA 不减体积（node binary ~40MB 占大头，SEA 不压缩）
- 更新体积痛点已被 delta 解决（SEA 全量替换 vs delta 大幅减小）
- 采用成本高（pnpm monorepo + workspace + overrides 需先 esbuild bundle；native 模块；跨平台 sea-config + codesign）

### 交付
- `docs/sea-feasibility.md`：原理/现状/体积分析/障碍/对比/决策/fallback/重评估触发条件
- `scripts/measure-sea-vs-bundle.sh`：体积对比工具骨架（前置 Node 22 + esbuild）

### fallback（已就位）
首装 tar.gz + 更新 delta + SHA256 + Ed25519。

## Phase 6.3：WASM Component Model（推进到 bindgen，2/5 步）

`runtime.rs` 加 `wasmtime::component::bindgen!({ path:"wit", world:"inkos-plugin" })`：
- **编译期解析 wit/inkos.wit，生成 InkosPlugin 实例类型 + Host trait**
- wit 语法正确性 + wasmtime 27 bindgen 兼容性已验证（clippy -D warnings 零告警）
- 这是契约（wit）→ 类型化绑定的关键里程碑

### 5 步执行路径进度
1. 契约定义（wit/inkos.wit）✅
2. bindgen 生成绑定 ✅（本次）
3. 实现 Host trait（委托 host_api::HostContext）⏳
4. Component 链接器 + 实例化 + 类型化 invoke ⏳
5. 真实 component 测试（wit-bindgen + wasm 编译）⏳

bindgen 通过降低了剩余步骤风险（最大不确定性——wit 能否编译期生成绑定——已验证）。进程隔离插件已端到端可执行，6.3 剩余 3 步是强隔离增强，非功能阻塞。

## 验证
- `cargo test --lib`：328 passed / 0 failed
- `cargo clippy --all-targets -- -D warnings`：零告警
- `cargo build --example gen-keypair` / `--bin sign-bundle`：通过
