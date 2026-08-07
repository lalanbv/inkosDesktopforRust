# 变更记录：write_file 符号链接父目录逃逸拦截

## 日期
2026-08-07

## 变更类型
fix(security): write_file 拦截「父目录是指向沙箱外符号链接」的逃逸路径

## 问题（真实沙箱逃逸）
`write_file` 的路径校验是**字符串层**的组件解析（只接受 Normal/CurDir/ParentDir，
折叠 `..`），随后 `normalized.starts_with(work_dir)`。

该校验无法发现父目录是符号链接：
- work_dir 为**用户项目目录**，项目内符号链接是合法且常见的
- `work_dir/escape -> /tmp/outside` 时，`write_file("escape/pwned.txt")` 得到
  `normalized = work_dir/escape/pwned.txt`，`starts_with(work_dir)` **通过**
- 实际 `fs::write` 跟随符号链接 → 落到 `/tmp/outside/pwned.txt`（沙箱外）

对比：`read_file`/`list_dir` 走 `normalize_path`，用 `canonicalize()`（解析真实
符号链接目标）后校验，本就不受影响。缺口只在 write 路径——因为新文件不存在
无法 canonicalize，此前退化为纯字符串校验。

## 修复
父目录（已存在，故可 canonicalize）解析真实符号链接目标后重新校验：

```rust
if let Some(parent) = normalized.parent() {
    if let (Ok(canonical_parent), Ok(canonical_work_dir)) =
        (parent.canonicalize(), self.work_dir.canonicalize())
    {
        if !canonical_parent.starts_with(&canonical_work_dir) {
            return Err(PluginError::PermissionDenied(
                "路径逃逸: 父目录经符号链接指向工作目录外".to_string()));
        }
    }
}
```

父目录不存在时跳过——`create_dir_all` 只在沙箱内新建目录，无既有链接可跟随。
审计日志走统一的 `target: "inkos.plugin.security"`。

## 测试（+1）
`test_write_file_rejects_symlinked_parent_dir`（#[cfg(unix)]）：
work_dir 内建 `escape -> outside_tmpdir` 符号链接 → `write_file("escape/pwned.txt")`
断言 PermissionDenied **且** 沙箱外无文件产生（双重断言：拒绝 + 无副作用）。

## 验证
- `cargo clippy --all-targets -- -D warnings`：clean
- `cargo test`：**520 passed / 0 failed**（+1，零回归）
