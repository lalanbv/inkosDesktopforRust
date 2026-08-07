# 变更记录：write_file 内容大小守卫 + list_dir 条目数上限

## 日期
2026-08-07

## 变更类型
feat(security): write_file 8MiB 内容守卫 + list_dir 4096 条目上限

## 1. write_file 内容大小守卫
MAX_WRITE_FILE_BYTES=8MiB; content.len()超限→PermissionDenied。
防插件写入超大文件耗尽磁盘。

## 2. list_dir 条目数上限
MAX_DIR_ENTRIES=4096; 用 .take(4097) 检测超限→ExecutionFailed。
防海量目录项（每条~String alloc）OOM 宿主。

## 累计测试数
519 passed, 0 failed（全量）
