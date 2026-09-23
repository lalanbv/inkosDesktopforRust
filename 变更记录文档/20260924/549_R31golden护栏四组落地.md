# 549 号：R31 golden 护栏四组落地——pi-ai 0.87 迁移后的守门面锁定

- 日期：2026-09-24
- 类型：test（护栏快照，产品代码仅加只读导出）
- 前序：548 号 R30 迁移落地 → 本轮按 542 号施工图 §5 补齐四组 golden 护栏（先拍后迁的护栏补课）
- 编号：开号三查 HEAD=7e726282（548）、当日目录空——549 无撞。

## 一、实施内容

### 考古结论（四组现状盘点）
- 瞬时重试预算 2 与 failover 链序（R25）**已有覆盖**（llm-retry-behavior / golden-task-routing），本轮不重复。
- 死线缺省值表、事件流形态快照、AgentEvent 变体名单、estimatePiContextTokens 口径守门**均无**，本轮补齐。

### 第 1 组：流守卫参数表快照
- provider.ts 新增只读导出 `STREAM_GUARD_DEFAULTS`（本笔唯一产品代码改动，零行为）：chat 首包 120s / 空闲 90s、pipeline 首包 300s / 空闲 180s、瞬时重试 2——四常量此前是模块私有，golden 无从锁定；导出后测试 `toEqual` 精确匹配 golden（golden/r31-pi-guard-vectors.json `streamGuardDefaults`）。

### 第 2 组：AssistantMessageEvent 流形态 + adaptive 容忍
- 受控序列回放（start→thinking_start/delta/end→text_start/delta/end→done）经 `guardAssistantMessageStream` 消费，透传序列 `toEqual` golden 快照（锁定守卫的**透传语义**——549 时点守卫不过滤不改序）。
- **adaptive 新变体容忍**：0.87 新增 adaptive 事件注入流尾，断言不崩且 done 终态到达（施工图 §3-3 的断言要求，锁定 548 号迁移语义）。

### 第 3 组：AgentEvent 十变体名单 + SSE 事件集合
- golden 锁定十变体名单（agent_start/end、turn_start/end、message_start/update/end、tool_execution_start/update/end——0.73→0.87 零漂移面）；真实 pi-agent-core Agent + mock streamFn 回放一轮，出现过的变体必须全在名单内（上限锁：上游新增变体即红，防未知漂移）。
- studio 侧 `STUDIO_SSE_EVENTS`（46 事件）升级为**精确快照**：新增 golden/studio-sse-events.json 全集锁定（492 号事件面此前只有 arrayContaining 部分断言，防新增/删除/改名漂移）。

### 第 4 组：estimatePiContextTokens 双语义守门（R31-4）
- 数值级快照：legacy `{systemPrompt, messages}` 形态实测 **17** token、0.87 transcript `{messages 含 leading SystemMessage}` 形态实测 **19**（golden 回填实测值）。
- **parity 不变量**：transcript 形态较 legacy 多出的 2 = 多一条 message 的 `role` 标签（"system"）计入——断言差值恒等于 `estimateTextTokens("system")`。首跑抓到 17≠19 的口径差并完成定性：**内容计入口径等价（同文本同值），偏移恒定且保守**——若未来 system 内容计入路径变化（丢失或重复计），parity 先红。

## 二、门禁矩阵

| 门禁 | 结果 |
| --- | --- |
| core typecheck | 0 错误 |
| core test | 2132/2132 绿（249 文件，较 548 号 +6 护栏测试） |
| studio typecheck | 0 错误 |
| studio use-sse.test | 6/6 绿（含新精确快照） |
| gate:ts 七步 | 全绿（typecheck 18.6s / test 92.4s / audit 1.7s / build 19.7s / smoke 34.7s / 差分 36.0s / epub 34.2s） |
| duel 真跑 | 10/10（41.16s） |

## 三、教训与备案

1. **golden 首跑回填纪律的变体**：数值级快照的 expected 值首跑前是猜测——本笔 27 的猜测值首跑即红，实测回填 17/19；红的原因本身有信息量（口径差 2 = role 标签），golden 红灯=考古入口而非单纯失败。
2. **同值断言要分辨"内容等价"与"数值相等"**：R31-4 原意的守门本体是"system 内容计入路径不丢"——严格数值相等在双语义下本就不成立（message 条数不同）；把 parity 收紧为"差值恒等于已知常数偏移"才是既不过松也不过紧的断言。
3. **上限锁 vs 快照锁的选用**：AgentEvent 变体面用"出现过的必须在名单内"（上限锁，不要求全出现——单轮回放天然不全）；SSE 事件名单用"全集相等"（快照锁，名单是有限封闭集）。锁法跟集合语义走。
4. provider.ts 私有常量无导出面时，golden 只能锁行为不能锁参数——为可守门性补一个只读快照导出（`as const`）是零风险最小侵入。
5. R30b（compat 正统化）仍备案待实施；本笔后 R31 护栏面收口，v6 路线下一工程为 R32/R33（压缩与遥测，按 543 号修订顺序 R30→R31→R33→R32 已完成前二）。
