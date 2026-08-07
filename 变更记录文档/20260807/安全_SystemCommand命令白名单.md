# 安全：SystemCommand 命令白名单（security-audit R3 后续建议实施）

> 日期：2026-08-07
> 范围：`src-tauri/src/plugin/types.rs`、`src-tauri/src/plugin/manifest.rs`、`src-tauri/src/plugin/host_api.rs`
> 触发：security-audit.md 后续建议 #3（明确成文推荐）

## 缺陷（改动前）

`Capability::SystemCommand` 是**全量信任**能力（无命令白名单），与 `Network { allowed_domains }` 不对称。一旦插件声明即可执行任意二进制 + 任意参数——若将来接线给 WASM host imports / Tauri 命令，等于把任意代码执行暴露给不可信插件。

## 变更

### `types.rs` — 能力变体升级

```rust
// 前
SystemCommand,

// 后
SystemCommand { allowed_commands: Vec<String> },
```

对称 `Network { allowed_domains }` 模型。

### `manifest.rs` — 解析 + 安全校验

- `parse_capability("system_command")` → 空白名单（fail-closed，无命令可执行）
- `parse_capability("system_command:ls,git")` → `{ allowed_commands: ["ls", "git"] }`
- 新增 `is_safe_command_name(name)`：仅允许裸可执行名（字母数字 / `-` / `_` / `.`），**拒** `/`（路径遍历）、shell 元字符（`;` `|` `&` `$` 等）——防白名单本身成命令注入向量（白名单条目直接进 `Command::new(name)`，若有路径/元字符攻击者可借此执行任意路径或注入）。
- 含非法命令名 → 整个 capability 拒绝（None，fail-closed）
- registry `parse_capability` 复用自动传播

### `host_api.rs` — exec_command 改白名单检查

- 新增 `can_exec(command)` 纯函数 helper（便于单测）
- `exec_command` 改为 `!self.can_exec(command)` → PermissionDenied，替换原 `!has_capability(SystemCommand)`（全量信任）
- doc 更新反映白名单就位 + 接线就绪状态

## 测试（+5）

- `test_parse_capability`：扩展 system_command bare（空白名单）/ list（ls,git）/ 含路径拒 / 含 shell 元字符拒
- `test_is_safe_command_name`：裸名通过 / 绝对路径拒 / 遍历拒 / 分号拒 / 管道拒 / 空格拒 / 变量拒 / 空拒
- `test_exec_command_without_permission`：无 capability → 拒（既有，保留）
- `test_exec_command_whitelist_denies_non_listed`：有 `system_command:echo` 但执 `ls` → 拒
- `test_exec_command_empty_whitelist_denies_all`：空白名单 → 任何命令拒（fail-closed）

## 验证

- `cargo clippy --all-targets -- -D warnings`：clean
- `cargo test --lib plugin::`：**77 passed / 0 failed**（+3 新 exec 测试）
- manifest 测试：test_parse_capability + test_is_safe_command_name 均通过
