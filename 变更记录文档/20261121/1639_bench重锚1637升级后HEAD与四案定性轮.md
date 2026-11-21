# 1639 bench 重锚@1637 升级后 HEAD 与四案定性轮（2026-11-21）

1637 依赖升级后唯一未钉最新 HEAD 的面 = bench 性能/0GC 面（1636 锚于 813b9d25，1637 动了 TS 依赖）。本轮重锚该面并做四案定性。零源码变更。

## 本轮证据（@35641122）

1. **bench:gate**：负载门禁三轮重试制实践（第一/二轮拒跑 loadavg 21%/核>20% 阈——负载源=WindowServer 43.8%+ZCode UI 37%/17%，1448 号「会话工具链自身即负载源」定性同形；间隔等待后第三轮 loadavg 2.98/18核=16.5% 放行）→ **30 基准全过阈（阈 +30%）EXIT 0**。
2. **漂移分布**（criterion 终态汇总，有基线 30 枚）：正漂 21 / 零漂或负漂 9。正漂 Top：read_chain/read_chapter +21.8%、export_artifact/build_epub_12x3000 +10.8%（1636 时 +6.7% 持续正漂倾向）、read_chain/list_books +9.0%。
3. **read_chapter 隔离复跑噪声定性**：批跑 +21.8% → `cargo bench --bench hot_paths read_chain/read_chapter` 隔离跑 53.9µs，criterion change **-12.5%（p<0.05，Performance has improved）**——面状上浮（21 枚正漂聚集=read 族+整体右移）非点状回归 + Rust 源零变更 → **负载残留噪声定性成立**（257 号实测满载漂移 +190%~+520%，本轮 +21.8% 远小于满载形态=轻负载残留级）。门禁结论维持，基线不洗（1448 纪律）。
4. **sse_broadcast/dispatch/1 -66.0%**：1594 号已备案基线异常值修复现实（118ns 与 dispatch/8/32 齐平=合理形态），零漂移延续。

## 四案定性（改进候选扫描，均零改动）

1. **1.5.2 残留目录**：node_modules/.pnpm 三个 1.5.2 目录零引用（1.6.0 包依赖面 grep 零命中），顶层 symlink 实测全钉 1.6.0（node 解析 pi-ai=1.6.0）——pnpm 惰性回收常态，零功能影响，不动。
2. **./compat 子路径**：1.6.0 package.json exports 仍含 "./compat" 且 compat.js 在场——pi-env-keys.ts 镜像头注释「上游 deprecated ./compat」陈述仍准确，自持副本决策继续成立。
3. **supportsStrictMode**（1.6.0 新增 compat 布尔）：包内 API 层自动消费（各 API 模块 `model.compat?.supportsStrictMode ?? 默认`），本仓零接线需求；仓内 strict 断言面零命中。
4. **developer role 收紧**（openai-completions #10741）：仓内 developer role 测试断言面零命中，行为变化零测试面触碰；gate:ts test 腿全绿背书。

**TODO/FIXME 债务扫描**：engine-rs/src + src-tauri/src 零命中（1489 号「TODO 双栈零」延续）。

## 验证转移面

- Rust 全量测试/audit:rust/clippy/fmt：1637 同日证据（Rust 源零变更，1635 testgate 2732 + 当日双 0）转移成立。
- gate:ts 十腿/audit:npm：1637 当日已跑全绿（35641122 提交含十腿证据）。
- 上游：1637 当日 fetch 后 0 behind。

## 后续

- 下号自 1640 起；出口 = 下一巡检周期（2026-12 中旬到期，对照包以 1637 刷新后为新基线）。
- 用户侧待办：① Fork 图形端推送（955-1638）② 真实 Tauri build 打包+安装包验证（按 docs/RELEASE_RUNBOOK.md，含 engine-rs release 重编 + core/dist 重烤）。
