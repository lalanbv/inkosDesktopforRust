# 安全审计报告 — 2026-08-07

> 范围：inkosDesktop Rust/Tauri 后端不可信边界与安全关键路径
> 方法：聚焦 OWASP 相关 Rust/Tauri 攻击面（路径穿越、任意执行、签名降级、不可信反序列化、密钥处理），结合源码审查 + 回归单测
> 目标：服务「最安全可靠」硬要求——把 fail-open / 校验-写入路径不一致等潜在缺陷收敛为 fail-closed + 单测钉死

## 攻击面勘察

| 类别 | 勘察结论 |
|------|----------|
| `unsafe` 块 | 仅 4 处 `libc::kill`（进程组信号，unix），边界清晰、参数受控 |
| `std::process::Command` | 全部用显式二进制 + `.arg()`，**无 `.shell(true)`**——无 shell 注入面 |
| 不可信反序列化 | 插件输出经 `serde_json::from_str`，错误传播显式，无 `unwrap` 于不可信数据 |
| Webview 边界 | 自定义命令经 Tauri 派发；picker/settings 为 `withGlobalTauri` 原生 JS |
| WASM 边界 | Host trait 仅暴露 read_file/write_file/list_dir/http_get/log——**不含 exec** |

## 发现与处置

### S1 — engine bundle 签名门 fail-open（MEDIUM，**已修**）

**位置**：`src/updater/engine.rs` `apply_inner` 签名验证矩阵

**缺陷**：原 `match (sig_url, pubkey_hex)` 的 `_ =>` 兜底臂对 3/4 情形 warn + 放行。其中**「公钥已配置（`INKOS_ENGINE_PUBKEY=Some`，表达验证意图）+ release 未提供 `.sig`（`sig_url=None`）」**也放行。这是降级攻击向量：篡改发布 feed 的攻击者只需剥离 `.sig` asset，即可让「已配置验证」的客户端接受未签名（被篡改）bundle。

**修复**：把策略抽成纯函数 `decide_sig_action(sig_present, pubkey_configured) -> SigAction`（单一来源、可单测、0GC），矩阵改为 fail-closed：

| release 含 .sig | 配置了公钥 | 动作 |
|-----------------|-----------|------|
| 有 | 有 | 强制验证，失败拒绝 |
| 有 | 无 | 放行 + warn（无法验证；过渡期）|
| **无** | **有** | **拒绝**（缺签名=可疑/篡改/发布误配）|
| 无 | 无 | 放行 + warn（无签名基础设施；过渡期）|

`apply_inner` 委托该函数；拒绝路径 `anyhow::bail!` 并给出运维指引（补签 / 回退公钥配置）。

**回归单测**：`sig_action_verify_when_both_present` / `sig_action_warnpass_when_cannot_or_not_armed` / `sig_action_reject_when_armed_but_unsigned`（钉死 fail-closed）。

### S2 — `write_file` 校验路径 ≠ 写入路径（MEDIUM，**已修**）

**位置**：`src/plugin/host_api.rs` `HostContext::write_file`

**缺陷**：函数手工解析 `normalized`（拒绝 `..`/绝对/前缀组件并 `starts_with(work_dir)` 校验），却把内容写入 `full_path = work_dir.join(path)`（**保留原始 `..`/符号链接组件**）。校验一条路径、写入另一条，是经典的路径校验缺陷——任何使二者分离的边界条件即成逃逸口。

**修复**：删除未净化的 `full_path`，校验与写入统一指向 `normalized`。保留 `normalized` 解析逻辑（对新文件不能 `canonicalize`，故走组件解析）。

**回归单测**：`test_write_file_path_traversal`——`../evil.txt`、`a/../../evil.txt`、`/etc/evil`（RootDir）均拒绝；合法 `ok.txt` 仍可写。注：`C:\\evil` 在 unix 下是合法相对文件名（反斜杠非分隔符），仅在 Windows 是 `Prefix`——不纳入跨平台拒绝集，避免误报。

### S3 — `exec_command` 不可达性确认（INFO，**已加契约注释**）

**位置**：`src/plugin/host_api.rs` `exec_command`

**结论**：`exec_command` **当前不可达自不可信边界**——WIT（`wit/inkos.wit`）的 `invoke(command, args)` 是插件自身命令分发，非系统命令；WASM Host trait 不暴露 exec。即 WASM 插件**无法执行系统命令**，强隔离边界任意执行面为零。全仓 grep 确认 `exec_command` 仅被自身单测调用（运行期无外部调用方）。

**处置**：为 `exec_command` 补充安全契约 doc-comment——声明 (1) 当前不可达自 WASM；(2) `Capability::SystemCommand` 是全量信任能力（无命令白名单，与 `Network::allowed_domains` 不对称）；(3) **未来若接入 WASI host import 或 Tauri 命令，必须先引入 `allowed_commands` 白名单**，否则等于把任意代码执行暴露给不可信插件。当前在安装期 UX 告警（settings.html `installFromDir`）。

## 已知残留（记录 + 缓解，本次不修）

| 编号 | 描述 | 缓解 |
|------|------|------|
| R1 | ~~`write_file` 字面解析不抵御 **work_dir 内预置符号链接**~~ | ✅ **已闭环**（`c849bfac`）：`safe_extract_tar_gz` 现拒绝 symlink/hardlink 条目，安装期阻断恶意符号链接落地；`test_safe_extract_rejects_symlink` 钉死 |
| R2 | 进程隔离插件 OS 级网络沙箱无 root 跨平台不可行（unshare 需 CAP_SYS_ADMIN） | 已以 WASM host_api 域名白名单（强隔离）+ 进程隔离插件安装期 UX 警告替代，见 `docs/plugin-system.md`、`docs/i18n-a11y.md` |
| R3 | `Capability::SystemCommand` 无命令白名单 | 当前不可达（见 S3）；若将来接线见 S3 契约 |

> **2026-08-07 后续审查补充闭环**（见 `变更记录文档/20260807/安全_*.md`）：tar-bomb DoS
> 解压上限、6to4/NAT64 SSRF 旁路、`extract_host` userinfo/方括号 IPv6 解析、注册表
> 重放降级（单调新鲜性）均已修。本表 R1 亦于此次闭环。

## 验证

- `cargo test`：全 binary **0 失败**（lib 单测 338 含新增 4：3 × `sig_action_*` + `test_write_file_path_traversal`）
- `cargo clippy --all-targets -- -D warnings`：**clean**
- 签名门矩阵由纯函数钉死，未来改动会被 3 个单测拦截

## 后续建议（按价值）

1. ~~**R1 安装期拒符号链接**~~：✅ 已闭环（`c849bfac`，见上表 R1）
2. **签名密钥采购落地**：按 `docs/signing-procurement.md` 生成 Ed25519 密钥，CI 设置 `INKOS_ENGINE_PUBKEY`/`INKOS_ENGINE_SIGNING_KEY`，使 S1 的 Verify 分支在生产生效
3. **SystemCommand 白名单**（若计划暴露给 WASM）：把 `Capability::SystemCommand` 升级为 `{ allowed_commands: Vec<String> }`，对称 Network 模型
