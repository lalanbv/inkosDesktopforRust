# 496 号：pi-ai 0.73 升级可行性摸底——运行时兼容，typebox 迁移需专项

日期：2026-09-16　分支：develop　基线：dadd7576（495 号）

## 摸底过程与结论（实测驱动）

按候选做 pi-ai 0.67.1→0.73.1 升级可行性摸底（434 号安全钉的滚动评估）：

1. **依赖面**：pi-ai 0.73.1 已移除 protobufjs（434 号 GHSA 的直接诱因消失），且无 fast-xml-parser 直依赖；但新增/升级 `typebox ^1.1.24`（typebox 1.x）；
2. **使用面**：仓内对 pi-ai 的消费极窄——几乎全为类型导入（Api/Message/Context/AssistantMessageEventStream 等）+ `createAssistantMessageEventStream` 工厂；
3. **运行时实测**：双 pin 升 0.73.1 后 **core 2113 全测绿**（mock 驱动 chat/e2e 链全过）——运行时兼容证据充分；
4. **类型层断点 ×6**（3 文件）：play-agents.ts / agent-tools.ts / worker-agent.ts 的工具注册泛型——pi-agent-core 0.73 的 `WorkerResultTool<TSchema>`/`AgentTool.execute` 改用 **typebox 1.x** 类型体系，与仓内既有 typebox（经 pi-ai 0.67 传递的 0.x）类型身份冲突。

## 结论（摸底交付）

pi-ai 0.73.1 升级 = **typebox 0→1 迁移专项**（已知生态级破坏事件），涉及 play-agents/agent-tools/worker-agent 的工具 schema 泛型改写。运行时零破坏、类型层需专项——**本号回退双 pin 至 0.67.1（全绿恢复），专项另立**，宜与 cargo 解阻后的全量回归窗口合并执行（pi-agent-core 同时对齐 0.73.1）。

## 附带清理

- 快速验证了 smoke 套件的 `--engine rust` 单腿模式（486 号参数）工作正常。

## 门禁

- 回退后：core 2113 + studio 906 + cli 218 = 3237 全绿；studio tsc 0 错；工作区干净（仅本记录入库）；
- cargo 链接仍被 Xcode 许可阻断（481 号欠账 + 489 号 2 新单测持续排队）。

## 后续

- pi-ai 0.73 + typebox 1.x 迁移专项（估：3 文件泛型改写 + 全量回归，宜合并 cargo 窗口）；
- Xcode 许可、种子 canonical 化、npm 余 2 条上游钉死项——均维持既有排队。
