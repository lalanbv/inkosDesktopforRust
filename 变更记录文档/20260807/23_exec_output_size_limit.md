# 变更记录：exec_command 输出大小守卫

## 日期
2026-08-07

## 变更类型
feat(security): exec_command stdout/stderr 1MiB 截断守卫

## 问题
`Command::output()` 将全 stdout/stderr 缓冲到内存，无大小限制。
进程输出 GB 级数据 → OOM 宿主。

## 修复
const MAX_EXEC_OUTPUT_BYTES=1MiB; 超限截断前 N 字节并追加 "[输出截断]" 标记，
不报错（允许插件处理部分输出）。

## 累计测试数
519 passed, 0 failed（全量）
