# 1497 号：RUNBOOK 三层陈旧面更新轮（零代码改动）

## 目标与范围

/goal 自主持续开发续轮。1496 npm 面刷新（hono 4.13.19 生产+5 dev）
后重烤必要性再加一层，但 docs/RELEASE_RUNBOOK.md 背景段仍写
「两层陈旧」（1486 依赖+1487 fmt）——手册与实际陈旧面脱节，
本轮同步。

## 实施

- 背景段三层化：+1496 npm 面刷新层（hono 生产依赖+构建链产物，
  desktop-build-inkos.sh 全量构建自动覆盖说明）；
- 安全基线段补 npm outdated 态（6 条已刷/跨 major 978 钉定/pi 族
  Deprecated 备案）+1496 复绿引证。

零代码改动零新缺陷，U+FFFD 净。

出口=下一巡检周期（约 30 天后，2026-12 中旬前后到期）/earendil-mz
单包合并（pi 族 Deprecated 备案与出口联动）；用户侧待办=**Fork
推送（含 955-1497，Fork 图形端操作，按 docs/RELEASE_RUNBOOK.md
待办一）+正式打包三脚本重烤（按 RUNBOOK 待办二——三层陈旧面，
必须含 engine-rs release 重编）**。下一号自 1498 起。
