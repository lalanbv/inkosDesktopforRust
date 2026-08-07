# 变更记录：read_file 大小守卫 + 插件安装数量上限

## 日期
2026-08-07

## 变更类型
feat(security): read_file 8MiB 大小守卫 + MAX_INSTALLED_PLUGINS=256

## 1. read_file 文件大小守卫
`const MAX_READ_FILE_BYTES: u64 = 8 * 1024 * 1024`（8 MiB）。
`fs::metadata().len()` 前置检查，超限 → `PermissionDenied`（防 OOM）。
顺序调整：大小检查先于 filesystem 白名单检查（fail-fast）。
测试：`test_read_file_rejects_oversized_file`（稀疏文件模拟 8MiB+1）

## 2. 插件安装数量上限
`const MAX_INSTALLED_PLUGINS: usize = 256`。
`install_plugin` 在"已安装"检查后加数量守卫，防 DoS 耗尽磁盘/拖慢启动扫描。
错误：`InstallFailed("已安装插件数达上限 256，无法安装更多")`

## 累计测试数
519 passed, 0 failed（全量）
