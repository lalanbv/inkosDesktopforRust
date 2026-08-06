# 安全审计：签名门 fail-closed + write_file 路径校验

> 日期：2026-08-07
> 范围：`src-tauri/src/updater/engine.rs`、`src-tauri/src/plugin/host_api.rs`、`docs/security-audit.md`
> 目标：服务「最安全可靠」硬要求，收敛 fail-open / 校验-写入路径不一致等潜在缺陷为 fail-closed + 单测钉死

## 背景

对不可信边界与安全关键路径做聚焦安全审计（OWASP 相关 Rust/Tauri 攻击面）。勘察确认：无 `.shell(true)`（无 shell 注入面）、`Command::new` 全用显式二进制 + `.arg()`、`unsafe` 仅 4 处受控 `libc::kill`、WASM Host trait 不暴露 exec。发现两处可修缺陷 + 一处可达性结论。

## 变更

### S1 — engine bundle 签名门 fail-open → fail-closed（MEDIUM，已修）

`updater/engine.rs` 原 `match (sig_url, pubkey_hex)` 的 `_ =>` 兜底对 3/4 情形 warn+放行，其中**「公钥已配置 + release 无 `.sig`」**也放行——降级攻击向量（剥离 `.sig` 即可让已配置验证的客户端接受未签名 bundle）。

- 抽出纯函数 `decide_sig_action(sig_present, pubkey_configured) -> SigAction { Verify, WarnPass, Reject }`（单一来源、0GC、可单测）
- 矩阵：公钥已配置且无签名 → `Reject`（`anyhow::bail!`，附运维指引）；其余维持验证/过渡放行
- `apply_inner` 委托该函数

### S2 — `write_file` 校验路径 ≠ 写入路径（MEDIUM，已修）

`plugin/host_api.rs` `write_file` 手工解析 `normalized` 并校验 `starts_with(work_dir)`，却写入未净化的 `full_path = work_dir.join(path)`（保留原始 `..`/符号链接）。删除 `full_path`，校验与写入统一指向 `normalized`。

### S3 — `exec_command` 不可达性（INFO，已加契约注释）

确认 WIT 的 `invoke` 是插件命令分发（非系统命令），WASM Host trait 不暴露 exec → **WASM 插件无法执行系统命令**。为 `exec_command` 补安全契约 doc-comment：声明不可达 + `SystemCommand` 为全量信任能力 + 未来接线须先加 `allowed_commands` 白名单。

## 验证

- `cargo test`：全 binary **0 失败**（lib 单测 338，含新增 4：3 × `sig_action_*` + `test_write_file_path_traversal`）
- `cargo clippy --all-targets -- -D warnings`：clean

## 已知残留（见 docs/security-audit.md）

- R1：write_file 不抵御 work_dir 内预置符号链接（需安装期拒 symlink 条目闭环）
- R2：进程隔离插件 OS 级网络沙箱不可行（已以 WASM 白名单 + UX 警告替代）
- R3：`SystemCommand` 无命令白名单（当前不可达；接线前须升级）

## 文档

- 新增 `docs/security-audit.md`（`docs/*` 按 .gitignore 不入库，作本地参考）
