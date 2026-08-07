# 变更记录：filesystem 能力相对路径 fail-closed

## 日期
2026-08-07

## 变更类型
fix(security): filesystem 能力声明拒绝相对路径（manifest + host_api 双侧）

## 问题（权限绕过 + 知情同意失效）
`has_filesystem_capability` 对声明的 `allowed_path` 直接 `Path::new(allowed_path).canonicalize()`。
相对路径由此**相对宿主进程 CWD** 解析，而非插件 work_dir：

```
声明 filesystem:.
宿主 CWD = /Users/x/GitProject          （work_dir 的祖先）
canonical_allowed = /Users/x/GitProject
cwd.starts_with(canonical_allowed) → true  ← 恒真
```

结果：插件拿到 work_dir 全量文件读取，而安装时的敏感权限警告只显示无害的
`filesystem:.`——用户看到的授权范围与实际生效范围不一致，知情同意机制被绕过。
放开范围还随宿主启动目录浮动（同一插件在不同 CWD 下权限不同），不可预测。

## 修复（双侧，纵深防御）
1. **manifest.rs `parse_capability`**：`filesystem:` 只接受绝对路径，
   相对路径/空路径 → `None`（整条能力丢弃，fail-closed）。声明方必须写明确绝对路径。
2. **host_api.rs `has_filesystem_capability`**：`!allowed.is_absolute()` → `return false`。
   覆盖绕过 manifest 直接写入的旧 metadata（升级场景），同时让字符串回退分支
   `path.starts_with(allowed_path)` 的前提（绝对路径）成立。

## 测试（+2）
- `test_parse_capability` 扩展：`filesystem:.` / `filesystem:../../etc` /
  `filesystem:sub/dir` / `filesystem:`（空）四种相对形式均断言 `None`
- `test_filesystem_capability_ignores_relative_allowed_path`：绕过 manifest
  直接构造带相对路径声明的 metadata，遍历 4 种形式断言 `read_file` 全部 PermissionDenied

## 验证
- `cargo clippy --all-targets -- -D warnings`：clean
- `cargo test`：**521 passed / 0 failed**（+1 净增，零回归）
