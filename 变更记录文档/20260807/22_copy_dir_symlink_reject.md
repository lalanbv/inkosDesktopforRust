# 变更记录：copy_dir_all 符号链接拒绝

## 日期
2026-08-07

## 变更类型
feat(security): copy_dir_all 拒绝符号链接（防敏感文件复制进插件目录）

## 问题
`copy_dir_all`（本地插件安装路径）使用 `std::fs::copy`，它跟随符号链接读取目标内容。
含指向 `/etc/passwd` 等路径的 symlink 的插件源目录可将敏感文件复制进 plugins_dir。

## 修复
`file_type.is_symlink()` → `io::Error(InvalidInput, "插件源目录含符号链接（禁止）")`
与 `safe_extract_tar_gz` 的 tar 解压侧符号链接拒绝对称。

## 累计测试数
519 passed, 0 failed（全量）
