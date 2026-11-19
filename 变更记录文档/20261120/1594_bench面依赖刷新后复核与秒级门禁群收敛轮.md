# 1594 bench 面依赖刷新后复核 + 秒级门禁群收敛轮（2026-11-20）

## 本轮增量靶面：bench:gate 自 1586 新基线后未复跑，而 1588+1589 触碰潜在漂移面

1586 收集基线（collectedAt 2026-11-19T19:34Z）之后：1588 改 src-tauri/rustbin.rs（生产代码）、1589 刷 Cargo.lock（engine-rs 10 项 patch/minor：cranelift 0.137.3=wasmtime JIT 后端、bytes 1.12.2=网络缓冲——bench 采样面底座）。bench 门禁在此后三轮变更上从未复跑。「变更不在采样面」的 1448 引证前提不成立（1589 直接动采样面依赖），需真跑——凌晨 5:23 静默时段守卫放行。

## 实跑结果（当前 HEAD=33cbdef5）

1. **bench:gate 30 场景全带内通过**（阈值 +30%）：最大正向漂移 sensitive_words/scan_chapter +11.6%（未超阈）；多数场景 ±3% 内。**无真回退信号**。
2. audit:semantic-patterns findings 0（candidates 9 report-only + whitelisted 31，与 1590 收官形态一致）。
3. verify:engine-bindings 186 导出零漂移（内部 205 测试 ok）。
4. verify:publish-manifests 3 包 OK。
5. audit:rust/npm 双 0——1593 轮（同日同 HEAD 代码面）已跑，日扫级不重复。

## 漂移归因（两处点名）

- **sensitive_words/scan_chapter +11.6%**：同场景 1055 先例已有 +14.8% 漂移倾向（后定性环境噪声）；同族变体 builtin_only +0.9% 不同步=环境噪声特征；基准面代码 1586 后零变更。带内不强制归因闭环，备案观察。
- **sse_broadcast/dispatch/1 -67.4%**（363.09→118.44ns）：基线值形态异常——1 订阅者慢于 32 订阅者 3 倍违反单调性；当前值与 dispatch/8（117.06）/dispatch/32（117.06）齐平=合理形态回归；1586 后 sse 面代码零变更→定性**基线采样噪声被当前正常采样修复现实**（非回退非本轮改善）。门禁已过不刷基线（-67% 改善余量留存更严；550 号手工改基线 Faces 追加场景先例不适用于改既有值）。

## 收敛面更新

bench 面闭环：22+ 场景在 1588/1589 变更后复核通过——至此十面验证面在当前 HEAD 全部有当周证据（testgate 2731=1589/1590 两轮+sweep 23=1593+gate:ts 十腿=1590+vitest 2088=1592+bench 30 场景=本轮+audit 双 0/semantic/bindings/manifests=1593/1590/本轮+release bin=1587/1589 保鲜）。

## 本轮零生产代码变更（纯验证轮）

## 后续

- 下号自 1595 起；用户侧待办不变：① Fork 图形端推送（955-1594）② 正式打包三脚本重烤+真实 GUI 手测。
