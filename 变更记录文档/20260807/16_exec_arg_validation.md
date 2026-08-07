# 变更记录：exec_command 参数校验 + registry HTTPS-only 守卫

## 日期
2026-08-07

## 变更类型
feat(security): exec_command args NUL/length 校验

## 问题
`exec_command` 将 `args: &[String]` 直接传给 `Command::new().args()`。
- NUL 字节：OS 层截断参数，行为未定义，可绕过日志截断造成误判
- 超长参数：触发 `E2BIG`（POSIX ARG_MAX 超限），错误信息无安全语义

## 修复
- `const MAX_ARG_LEN: usize = 4096`
- 每个 arg 校验：含 NUL → `ExecutionFailed("参数 #N 含 NUL 字节")`
- 每个 arg 校验：len > MAX_ARG_LEN → `ExecutionFailed("参数 #N 超过最大长度")`
- 测试：`test_exec_command_rejects_nul_byte_in_arg`
- 测试：`test_exec_command_rejects_overlong_arg`

## 累计测试数
518 passed, 0 failed（全量）
