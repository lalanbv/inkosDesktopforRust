# 1496 号：npm outdated 复查轮——26 条分类处置+6 条刷新+Deprecated 族备案

## 目标与范围

/goal 自主持续开发续轮。识别未覆盖验证面：**npm outdated 复查**
（1423 后 70+ 轮未查；1486 巡检只盖 audit 面）。对照结果重大漂移：
1423 **空输出** → 本轮 **26 条**（约 3 天 npm 生态发版潮）。

## 分类处置

①**6 条 patch/minor 刷新**（semver 兼容）：
- **hono 4.13.18→4.13.19（生产依赖**——studio server 框架）；
- dev 工具链 5 条：@streamdown/cjk 1.0.5/math 1.0.4/mermaid 1.0.4/
  user-event 14.6.12/axe-core 4.14.1。
- 锁变更 39+39 行；picomatch peer 警告=1490 已定性双版本既有形态。

②**跨 major ~20 条维持 978 钉定制**：typescript 5→7/vite 6→8/
zod 3→4/ai 6→7/commander 13→15/dotenv 16→18/ink 7→8/marked 15→18/
undici 6→8/unpdf 1→2/@types/node 22→26/motion 12→14/nanoid 5→6/
lucide-react 0→1/@xyflow 12→13/motion 等——触发条件制未点火不升。
- 备注：zod latest 4.6.7 含 GHSA-mrhx 补丁线 4.6.6（升 4 可出清
  758 白名单条目），但 zod 3.25.76 受 MCP/Anthropic SDK peer 契约
  阻塞（758 定性维持：须随上游 SDK 动作）。

③**Deprecated 2 条备案**：@mariozechner/pi-agent-core+pi-ai
0.73.1（@actalk/inkos-core 依赖）上游标记废弃——**earendil 出口
关联族**（earendil-mz 合并时该族将被替换），当前无 advisory 无
运行时风险，维持现状等出口事件联动处置。

## 验收

**gate:ts 十腿整跑全绿 exit 0**（hono 生产面刷新零回归：build/
typecheck/test 107.2s/audit:npm/rust-bin/node-fallback-smoke/
engine-contract-diff/export-epub-smoke/work-alternation/
check-routes）。

零新缺陷，U+FFFD 净。

出口=下一巡检周期（约 30 天后，2026-12 中旬前后到期）/earendil-mz
单包合并（**pi 族 Deprecated 备案与出口联动**）；用户侧待办=
**Fork 推送（含 955-1496，Fork 图形端操作，按 docs/RELEASE_
RUNBOOK.md 待办一）+正式打包三脚本重烤（按 RUNBOOK 待办二，
必须含 engine-rs release 重编）**。下一号自 1497 起。
