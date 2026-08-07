# 变更记录：可观测性 Ring Buffer + entrypoint 路径遍历修复

## 日期
2026-08-07

## 变更类型
- feat(security): entrypoint 路径遍历守卫
- feat(observability): recently_auto_disabled ring buffer

---

## 1. entrypoint 路径遍历修复（security/HIGH）

### 问题
`manifest.rs` 中 `entrypoint` 字段直接 `plugin_dir.join(&entrypoint)` 用于定位插件入口文件，但未做任何路径安全校验。恶意 manifest 可写 `entrypoint = "../../etc/passwd"` 读取插件目录外任意文件。

### 修复
新增 `is_safe_entrypoint(s: &str) -> bool`（与 `registry.rs::is_safe_relative` 对齐），在 `parse_manifest` 解析后立即校验：
- 禁止 `ParentDir`（`..`）组件（折叠后逃逸亦拒绝，如 `a/../../x`）
- 禁止绝对路径（`/usr/bin/sh`）及 Windows 前缀
- 禁止空串及 NUL 字节
- 允许相对子路径（`native/plugin.wasm`、`./plugin.sh`）

### 测试
- `test_is_safe_entrypoint`：白名单/黑名单单元测试
- `test_parse_manifest_rejects_traversal_entrypoint`：三种 traversal case + 合法值

---

## 2. recently_auto_disabled Ring Buffer（observability）

### 问题
`auto_disabled_count` 告知 session 内自动禁用的次数，但运维无法知道**哪些**插件被禁用，难以定位不稳定插件。

### 实现
- 新增 `const AUTO_DISABLED_RING_CAP: usize = 5`
- `PluginManager` 新增 `recently_auto_disabled: VecDeque<String>`（cap=5 ring buffer）
- 自动禁用触发时追加 plugin id；超过容量时 `pop_front` 滚出最旧
- `PluginMetrics` 新增 `recently_auto_disabled: Vec<String>` 字段
- `metrics()` 暴露 `.iter().cloned().collect()`
- `settings.html` loadMetrics 渲染：`🔴 最近禁用：plugin-a, plugin-b`

### 测试
- `test_metrics_initial_zero`：新增 `recently_auto_disabled.is_empty()` 断言
- `test_recently_auto_disabled_ring_buffer`：创建 AUTO_DISABLED_RING_CAP+2 个插件，验证溢出后 tail 正确、count 正确

---

## 累计测试数
511 passed, 0 failed（全量）
