# 1609 bench:gate 高性能/0GC 面补齐轮（2026-11-21）

基线 = 1608 终态（8104dcd2，git clean）。本轮零源码变更，唯一靶面 = 1608 唯一未覆盖门禁：bench:gate（高性能/0GC 证据面）。

## 主靶面：bench:gate 全过

- 负载前置：会话静默窗口（无并行负载）满足 1448 号隔离条件；gate 自带 loadavg/核比检查通过。
- **30 条 criterion 基准全部过阈**（阈值 = 基线 +30%，1594 采集，未动基线）。
- 漂移分布：最大正漂 **+4.7%**（sensitive_words/scan_chapter，16.52µs→17.29µs，噪声级），负漂最深 **-68.6%**（sse_broadcast/dispatch/1，363.09ns→113.99ns）。
- 大负漂归因（归因前逐流落盘纪律）：改善方向非回退；当前 HEAD 零源码变更（1607 起 git clean 实证）→ 非代码信号，属测量环境方差（CPU 频率/缓存态）；门禁只卫 +30% 回退侧设计正确；**未跑 --update**（严禁洗基线，改善侧漂移同样不动基线）。

## 当前 HEAD 验证面全景（1608+1609 合账，零源码变更纯验证）

clippy/fmt 双 0（1608）+ testgate 2732 passed 含 audit:rust 0 漏洞/必红自检 9/1/duel 真跑（1608）+ gate:ts 十腿（1608）+ ignored-sweep 23 枚（1608）+ audit:npm/semantic 双 exit 0（1608）+ bindings 186/publish-manifests 三包（1608）+ 上游零落后（1608）+ static 快照零漂移（1608）+ GUI 真人走查九形态/停机链/清场（1608）+ **bench 30 条（本轮）** ——全部门禁面闭合，零失败零漂移零孤儿。

## 本轮零源码变更（纯验证轮）

## 后续

- 下号自 1610 起；用户侧待办不变：① Fork 图形端推送（955-1609）② 真实 Tauri build 打包+安装包验证。
