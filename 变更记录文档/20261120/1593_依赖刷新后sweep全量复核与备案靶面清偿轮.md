# 1593 依赖刷新后 sweep 全量复核 + 备案靶面清偿轮（2026-11-20）

## 本轮增量靶面：1589 依赖刷新后 sweep 未在当前 HEAD 复跑

1592 收官表格中 sweep 23 枚全绿记录时点=1588 轮（warn_if_stale 后），**早于 1589 轮 Cargo.lock 刷新**（wasmtime 50.0.2 patch 面+cranelift 0.137.3/bytes 1.12.2/zip 9.7.3 等全量 patch/minor）——备案待办「e2e_startup 插件面复核（wasmtime 50 后）」的可会话内部分正是这一面。本轮闭合。

## 实跑结果（当前 HEAD=376f04e8）

1. **audit:rust 双 crate 0 漏洞**（advisory-db 最新提交 0 天前）+ 必红自检活（9 漏洞夹具检出 9 + 1 漏洞夹具检出 1——检测器非恒绿）。
2. **audit:npm advisory 合计 4：白名单放行 4（已定性 dev 工具链），拦截 0** + 4 夹具自检全绿。
3. **sweep 23 枚六 scope 全绿（当前 HEAD 首跑）**：
   - lib 7/7（pfctl bail + keychain 真实四枚 + updater 双通道网络面）
   - sync_watcher_io 3/3（探针自同步 watcher 端到端）
   - e2e_updater 1/1（is_newer 纯逻辑）
   - supervisor_integration 1/1（真实 inkos sidecar 拉起→SPA 可达→kill_tree 端口释放；engine 组装幂等重走，node_min=22.19.0）
   - **e2e_startup 2/2（真实 release binary 启动冒烟×2——wasmtime 50+依赖刷新后启动链活体；release bin 1589 gate:ts rust-bin 腿编译保鲜，编译增量近零）**
   - e2e_secrets 9/9（真实 keychain，unique_service 无 ACL 弹框）
4. **清场终态净**：4567 空闲+无 inkos 进程残留；尾清回收 1 枚引擎孤儿（87247，PPID=1+本仓路径特征——1435 自愈闭环按设计兜底）。

## 插件面证据链完整性说明

e2e_startup 本体=启动冒烟+日志版本（1431 备案两形态皆绿）；wasmtime 50 下插件**运行时**行为另有 testgate 常规链内 wasm_plugin_execution/plugin_execution/perf_plugin_wasm 证据（1583 升级轮+1589 依赖刷新轮 testgate 2731 两轮绿背书；1590 后 Rust 面零变更故证据链延伸至当前 HEAD）。备案的复核语义已闭合。

## 本轮零生产代码变更（纯验证轮）

工作树净，基线=上轮终态全绿，无新 BUG 浮出。

## 后续

- 下号自 1594 起；用户侧待办收窄：① Fork 图形端推送（955-1593）② 正式打包三脚本重烤 + 真实 GUI 形态手测（e2e_startup 复核已覆盖 CLI 形态）。
