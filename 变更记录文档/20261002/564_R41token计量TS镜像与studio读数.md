# 564 号：R41 token 计量——TS 镜像+node 端点+usageOf 活体接线+studio 读数

日期：2026-10-02
类型：feat(core+studio)，563 号 R41 Rust 侧的 TS 镜像同水位批次

## 选题来路

563 号落地 Rust 侧计量核心+聊天挂线+观测端点+span schema 计量面，并预留
三项 TS 侧对齐：golden 消费（双端共享向量）、usageOf 调用方接线、studio
读数（v7 R41 设计列"面板挂 RunLog 旁"）。本号同水位清偿，R41 全项收官。

## 实施

- **TS 镜像** `packages/core/src/utils/token-meter.ts`：TokenMeter 类逐语义
  同构（append O(1)/noteUsage 收缩防御/measure 锚点校准+覆盖率 4 位舍入/
  clamp 0.25–4/total 缺省回退 input+output/空锚退启发式）；启发式复用
  `estimateTextTokens`（双端同源）；core index 出口 TokenMeter+四类型。
- **双端 golden 消费** `src/__tests__/token-meter.test.ts`：回放 563 号
  `token-meter-vectors.json` 12 case——首跑 13/13 即绿（双端同源向量对拍
  成立；改向量须双端同批）。
- **usageOf 活体接线** `agents/base.ts` chat()：worker-agent 响应自带权威
  usage → span end 计量四键（563 号预留提取器的首个真实调用方；
  submitStructured 结果无 usage 不接）。
- **会话计量** `agent-session.ts`：AgentSessionResult 加 `contextMeter`
  快照——全量 state.messages 回放入表（含本轮 assistant 回复 = 下一轮
  请求面，与 Rust 表语义同构）、usage 入锚（锚点模型=请求侧 model.id——
  AssistantMessage 无 modelId 字段的考古修正）、pi `model.contextWindow`
  为窗口；尽力而为 try/catch（观测不阻断）。
- **node 端点** `studio/api/server.ts`：contextMeterRegistry（Map 保序
  cap 64 逐出最旧，删旧再插=最新在尾）+ `GET /api/v1/context-meter`
  （缺参 400/无记录 404，与 Rust 误差面同形）；runAgentSession 解析后
  同点留痕（与 Rust agent_route 同点尽力而为）。
- **studio 读数** `ContextMeterBadge.tsx`：聊天输入状态条挂载（模型选择器
  同行右端）——末轮请求面 token（k 格式化）+usage/estimate 来源+超窗
  告警色+tooltip 详情（锚点校准/覆盖率/窗口/模型）；5s 轮询自刷新；
  无快照不渲染。挂位裁决=v7"挂 RunLog 旁"的意图是运行遥测邻近观测位，
  但 Dashboard 无 sessionId 语境——落位 ChatPage 状态条（会话面观测的
  自然位，备案）。
- **smoke 套件** `node-fallback-smoke.mjs` 双腿 +2 检查：context-meter
  缺参 400/未知会话 404 双端对照（快照面需真实聊天轮，fixture 不驱动——
  契约由双端 golden/单测锁定，备案）。

## 门禁

- core vitest 全量 258 文件 2214 测试绿（含 token-meter 13）；studio
  components 32 文件 168 绿（含 ContextMeterBadge 4）；core/studio tsc 双 0；
- gate:ts 七步全绿（含 node-fallback-smoke 双腿 context-meter 误差面对照、
  engine-contract-diff 活体差分）。
- Rust 侧零改动（563 已锁向量），cargo:testgate 不涉重跑。

## 教训

- 门禁并行干扰：手动 vitest/tsc 与 gate:ts 同时跑 → typecheck 瞬态红
  （产物竞争），隔离复跑即绿——bench:gate 高负载误报同类（记忆条目印证），
  门禁窗口内禁并行重负载。
- 双端镜像先考古字段存在性：锚点模型身份初版取 AssistantMessage.modelId，
  pi-ai 类型实为 DeferredHandle 字段——改请求侧 model.id（usage 归属请求
  模型，语义更准）。
- 端点形状一经交付即为对照面：smoke 双腿 400/404 断言锁死误差面，快照面
  契约由双端各自测试锁定，fixture 不驱动的面要显式备案豁免理由。
