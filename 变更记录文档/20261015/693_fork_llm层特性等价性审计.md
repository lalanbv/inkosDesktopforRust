# 693 号：fork llm 层特性等价性审计（688 号 §五.3 裁决：上游 2.0 原生覆盖，无需移植）

日期：2026-10-15。类型：docs/audit（零代码变更）。前置：692 号（82556a3f）。

## 一、审计结论（本号最重要的「不移植」决定）

688 号 §五.3 立项的「fork llm 层特性移植」经逐项勘察**裁决为无需移植**：上游 2.0 的 provider 重写（1745 行 HTTP 传输层客户端，含 openai-completions/responses/anthropic 三格式）已原生实现三项 fork 特性的等价语义，且**每项都有上游自测覆盖 + UI 面在位**，合并树全绿含这些测试。把 fork 实现（earendil pi-dispatch 形态）移植到 2.0 之上会制造双实现漂移面，严格劣于原生路径。

## 二、逐项证据（fork 特性 → 2.0 原生等价物）

| fork 特性 | fork 实现（backup 分支） | 2.0 原生等价物 | 证据 |
|---|---|---|---|
| **R31 guarded stream deadline**（流挂死防护：首事件超时+空闲超时，失败推 error 事件不死锁） | pi-stream guardedPiStream + STREAM_GUARD_DEFAULTS + llm-retry 重试环 | `createStreamActivityDeadline` + `LLMStreamInactivityError` + `guardAssistantMessageStream` + `createRequestDeadline`（provider.ts:47-142），env 可调（readPositiveTimeout） | provider.ts 结构；pi-stream.test.ts:391「ends a stalled non-stream request at the configured deadline」 |
| **607/613 非流式支持**（服务「流式响应」开关关 → completeSimple 整体响应适配回事件流；text_delta 渲染链） | pi-dispatch completeSimple 分支 + guardedCompleteStream | 每传输层 `if (!client.stream)` 分支走 `piCompleteSimple`（provider.ts:1625-1654），非流式工具调用适配回 Pi 事件 | provider.ts:346 `stream = config.stream ?? true`；pi-stream.test.ts:660「adapts a non-streaming tool call back into Pi events」+ :426/:652 stream:false 客户端用例 |
| **621 stream 偏好跟随实际解析服务**（选中服务条目的 stream 提升进生效配置，层内同源） | effective-llm-config 提升链 | provider 侧全尊重 `client.stream`；effective-llm-config.ts:352 `if (entry.stream !== undefined) llm.stream = entry.stream`（transportDefaults 兜底：353）+ env INKOS_LLM_STREAM（:365）+ cli override（:224/323/331） | effective-llm-config.test.ts:285-291「entry stream:false → result.llm.stream false」 |
| 深度思考模式（deepseek reasoning_content / minimax thinking） | pi-stream explicitDeepSeekThinkingMode | provider 自有 reasoning 处理（extractChatReasoningContent:1032）+ minimax thinking 用例 | provider-minimax-thinking.test.ts |

**UI 面核验**：每服务「流式响应」开关在合并树 `service-detail-state.ts` 完整存活（stream 字段 :53/:80/:199/:215/:244——双侧同形，合并无断点）；配置层 `LLMServiceEntrySchema.stream` 与 `LLMConfigSchema.stream` 均在。

## 三、审计方法（对齐 690 号「不做也需要活体证据」纪律）

1. 结构勘察：`grep -n "^export \|^function "` 全量映射 2.0 provider 导出面 → 发现 deadline/非流式机制原生在位；
2. 语义比对：三特性逐一在 2.0 代码找等价物（file:line 级），**不以「有类似名字」为准，以语义（触发条件/失败形态/配置来源）为准**；
3. 覆盖核验：每项找到上游自测的具名用例（见上表证据列）并实跑确认——`effective-llm-config + pi-stream + llm-retry-behavior + config-loader` 四文件 **60/60 绿**（合并树实跑）；
4. UI 面核验：服务详情页 stream 开关双侧齐全。

## 四、教训

1. **欠账清偿的第一步是「等价性审计」而非「动手移植」**：688 号立项时按「fork 有 → 合并后丢 → 移植回来」的三段式思维写下了 §五.3，实际「丢」的只是 fork 的实现载体，特性本体上游已原生实现——移植三特性会制造双实现漂移（正是 689-692 号极力避免的）。
2. **上游大重写后，先做特性等价矩阵再排施工计划**：本号零代码变更清偿一项欠账，是全部迁移工作中性价比最高的一个号；后续 minimax 仲裁等项同样应先审计再动手。
3. fork 侧残留物处置：fork 的 pi-dispatch/pi-env-keys/model-chain 等 earendil 形态模块已于 688 号随 llm/ 整体切换退役（孤儿删除，backup 分支可查）——本号确认无需复活。

## 五、欠账台账更新（688 号 §五）

- ✅ §五.1 SSRF 防线（689）
- ✅ §五.2 skills:change（690）
- ✅ §五.3 asset-library 路由族（691）
- ✅ §五.4 会话树 R36（692）
- ✅ §五.5 fork llm 层特性（**本号：审计裁决原生覆盖，无需移植**）
- ⬜ contract-diff 三域重组（受 2.0 契约源守卫保护中，迁移完成后守卫自动复活）
- ⬜ minimax 元数据语义仲裁（上游 endpoint 目录 vs fork pi 注册表继承——低优先，contract-diff 复活时随面仲裁）
