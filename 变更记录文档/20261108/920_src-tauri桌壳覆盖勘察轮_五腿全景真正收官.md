# 920 号 · src-tauri 桌壳覆盖勘察轮：21 目标精准覆盖 + 文档契约测试亮点——五腿覆盖全景真正收官

> 919 方法学（引擎面勘察）对最后一块未察拼图的收尾：src-tauri 桌壳
> （第三腿，25 目标/584 用例）的 tests 域分布与 src 模块对照。结论：
> **21 目标精准覆盖桌壳全部用户可感知域，无缺口**；亮点=文档契约测试
> 形态（DESKTOP_WALKTHROUGH 人工走查清单有自动守卫）。零代码改动轮。
> 验证：gate:ts 十步全绿（当日 914-920 七轮连绿）。

## 一、21 目标域分布（src 模块对照）

| 域 | 目标 |
|---|---|
| 配置 | config_integration / config_reload / sync_unit / sync_watcher_io |
| 密钥 | e2e_secrets（keychain） |
| 启动链 | e2e_startup / rust_backend_launch（164 直启）/ supervisor_integration（监督者） |
| 更新器 | e2e_updater / secure_update_integration（166 双通道） |
| 插件 | plugin_execution / wasm_plugin_execution / marketplace_e2e / plugin_doc_contract（文档契约）/ perf_plugin_wasm |
| 性能 | perf_plugin_wasm / perf_scanner（**桌壳自有 perf 目标两件**） |
| 通信 | sse_contract / observer_integration / project_commands |
| 文档契约 | **desktop_walkthrough_doc_contract**（DESKTOP_WALKTHROUGH 人工走查清单的自动守卫——文档对齐的自动化先例） |

src 模块（bin/commands/config/engine[manifest|node|rustbin]/isolation/
lifecycle/observability/observer）全部有对应测试面。

## 二、净结论

- **五腿覆盖全景真正收官**：core（185 文件）/studio（141 文件矩阵）/
  cli（23 文件）/engine-rs（51 目标含 43 golden）/src-tauri（21 目标
  精准域覆盖）——五腿全部勘察闭合，无已知真空白。
- 亮点沉淀：文档契约测试形态（desktop_walkthrough_doc_contract）=
  「文档对齐」的自动化终局形态，后续新人工清单可同款建守卫。

## 三、验证

- gate:ts 十步全绿（当日 914-920 七轮连绿；work-alternation 5.2s/
  check-routes 0.1s 稳定）。

**下一号自 921 起；出口清空回归常备；用户侧待办=Fork 推送 54 个提交。**
