# 变更记录：安全拒绝事件结构化 target

## 日期
2026-08-07

## 变更类型
feat(observability/security): host_api 所有能力拒绝统一 target:"inkos.plugin.security"

## 问题
- `list_dir` 能力拒绝无任何 warn 日志
- `write_file` 路径逃逸两处（非法组件 / 沙箱边界）无 warn 日志
- 现有 warn! 无统一 target，无法被 ELK/Loki 按安全事件过滤聚合

## 修复
所有 PermissionDenied 返回点均加/更新 `warn!(target:"inkos.plugin.security", ...)`：
- `read_file`: capability deny + filesystem deny（已有→加 target）
- `write_file`: capability deny + 非法组件 + 沙箱边界（后两处新增）
- `list_dir`: capability deny（新增）
- `http_get`: network cap + domain whitelist + SSRF IP literal（加 target）
- `exec_command`: capability deny + 执行审计（加 target）

统一字段：`plugin_id`、`action`、`target: "inkos.plugin.security"`

## 累计测试数
511 passed, 0 failed（全量）
