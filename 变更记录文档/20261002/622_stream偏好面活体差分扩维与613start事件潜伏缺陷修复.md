# 622 号：stream 偏好面活体差分扩维 + 613 号 start 事件潜伏缺陷修复

日期：2026-10-02
类型：fix(core)+test(scripts)——差分器扩维 + mock 协议保真 + 真潜伏缺陷修复

## 背景与动机

v7 路线（R41–R46）于 602 号收官后，项目在案裁决项仅余待用户决策/人工走查项（620 快照）。本轮按「分析当前软件→找真实改进」开展全维扫描（监听器/Observer 泄漏面、Rust panic 面、TODO 债、双端 stream 链同构独立复核），确认代码面干净后，选定最后一个结构性缺口：**stream 偏好链（108/606/607/612/613/621 号）从未被活体双端对照**——

- 612 教训明确「此类功能落地必须 GUI 层真机验证而非仅 API 层」，但 613 的「真机复验」环境是零配置直发（层 3 secrets 兜底→custom:Mock 无 stream 字段→**走的是流式路径**），guardedCompleteStream 非流式路径从未被任何活体（GUI 或双端）验证过；
- 差分器（46 对照）覆盖 GET 面/会话面/计量面/技能写面/SSE 事件名集合，不含 stream 偏好行为面；
- mock（walkthrough-mock.mjs）对 chat/completions **恒返 SSE、无视请求 stream 旗标**——Rust 非流式线上路径（streaming_client 108 号：`stream:false` → 整体 JSON + `serde_json::from_str`）在 mock 环境下必然 JSON 解析失败，即使想活体验证也无法进行。

## 实施三件

### ① mock 协议保真（scripts/walkthrough-mock.mjs）

chat/completions 按请求 body.stream 旗标返回形态：`stream:false` → 整体 JSON（OpenAI 非流式形态：`choices[0].message.content/tool_calls` + choice 级 `finish_reason` + `usage` 三 token 键——与 Rust streaming_client 非流式解析面逐字段对齐）；`stream:true`（含缺省）→ 既有 SSE 不变。propose_action 工具调用分支与纯文本分支双形态都覆盖。既有消费者（fixture/冒烟/走查全部流式请求）行为零变化。

### ② 差分器新维度「stream 偏好面」（scripts/engine-contract-diff.mjs，46→48 对照）

- **活体补丁**：驱动前给双腿 inkos.json 追加第二服务 `custom:MockNF`（`stream:false`）+secrets 密钥。必须在 fixture 跑完之后补丁（fixture 会整体重写 inkos.json，preseed 被覆盖——首跑实证），且必须写**标准布局 `llm.services`**（层 1 显式服务解析只读该布局；顶层 services 为非标布局靠 INKOS_LLM_BASE_URL 环境兜底——593 号教训的又一次实证）。配置双端逐请求加载无启动缓存，热补丁即刻生效；fixture 环境对其他消费者保持单服务不变。
- **驱动**：每腿两轮（非流式 `custom:MockNF` / 流式 `custom:Mock`），各自层 1 显式 service+model；每轮独立 chat 会话 + 新会话过滤 SSE 收集器（event:+data: 逐行解析、按 data.sessionId 客户端过滤——双端总线全量广播、query sessionId 仅触发快照补发，过滤只能在客户端做）。
- **对照断言**：①有序事件名序列逐位一致（剔除 ping=协议保活；session:title 剔除备案=双端发射点有意不同，见④）②draft:delta 恰一次 ③载荷文本双端逐字节相等（不锚 mock CANON 常量——差分器对照双端等价，mock 内容正确性是 mock 自身的事）。

### ③ 真潜伏缺陷修复：guardedCompleteStream 缺 start 事件（packages/core/src/agent/pi-stream.ts）

首跑即抓获：非流式轮 node 事件序 `["agent:start","agent:complete"]`——**draft:delta 从未发出**，rust 正常 `["agent:start","draft:delta","agent:complete"]`。

根因链：pi-agent-core agent-loop.js 消费 streamFn 事件流时，`text_delta` 等增量事件仅在 `partialMessage` 就位（即见过 `start` 事件）后才转发为 `message_update`（`case "text_delta"` 的 `if (partialMessage)` 闸门）；613 号适配器只推 `text_delta`+`done` 缺 `start`，转发被静默跳过 → server.ts 的 `ame.type==="text_delta"` 分支永不命中 → draft:delta 永不广播。

**被掩盖的原因**：POST /agent 响应体携带完整文本，前端 sendMessage 的 `hasStream=false` 分支把回复当静态消息追加渲染——用户看得到回复（非打字机形态），无人察觉流渲染链断裂；613 单测只断言适配器自身事件序（text_delta→done），「真机复验」又走的是流式路径（见背景），三层验证全部错过。**612 号回滚的渲染回归在 stream:false 层 1 路径上实际始终存在**。

修复：按 pi-ai 真实流事件词汇（openai-completions.js）补齐 `start → text_delta → done` 序列——`start` 推 `{type:"start", partial:{...message, content:[]}}`（与真实流一致：partial 为空内容快照），text_delta 保持 613 形态（partial=message=增量施加后快照）。608 单测同步：新增事件序断言 `["start","text_delta","done"]`（可证伪性：修复前该断言按旧代码必红）。

## ④ 备案：session:title 相对终态时序双端不同

活体对照发现 Node 在 agent:complete **前**广播 session:title、Rust 在**后**（126 号有意设计「终态事件后补发，不扰动既有事件序列断言」）。标题事件**在场性**双端一致，相对时序非合同（前端两类事件独立消费、序无关），新维度剔除该事件并留注释备案。

## 验证

- 新维度双臂双端全绿：`stream 偏好面[nonStream]`/`[stream]` 均事件序 `["agent:start","draft:delta","agent:complete"]` + draft:delta 恰一次且载荷文本逐字节相等；
- 差分器 **48 对照 0 分歧**（46→48）；
- guarded-complete-stream 单测 3/3 绿（新增序断言）；
- **gate:ts 全量七步绿**（build 16.9s / typecheck 16.2s / test 74.7s / audit:npm / node-fallback-smoke 34.6s / engine-contract-diff 40.6s / export-epub-smoke 34.2s）——mock 改动经冒烟套件回归证实对既有流式消费者零影响；
- Rust 门禁裁剪备案（535/621 先例）：本轮零 Rust 改动（engine-rs/src-tauri 无触碰），clippy/testgate/bench 对 HEAD 证据链沿用 621 收口运行。

## 教训

1. **「活体验证」必须核对实际命中的代码路径**：613 的真机复验环境（零配置直发）解析到的是流式路径，guardedCompleteStream 分支从头到尾没被活体驱动过——环境解析链决定验证覆盖面，探针环境最小化纪律（600 号）之外还需**路径命中核验**（本次由差分器层 1 显式 service 驱动补上）。
2. **适配器单测锁不住集成面**：适配器自身事件序 2/2 绿的同时集成面断裂（消费端有 start 闸门）——事件流契约的验证必须覆盖「生产者→消费者」全链，差分器新维度正是这条链的活体合同。
3. **mock 的协议保真是活体验证的前置**：mock 无视 stream 旗标使整条非流式线上路径在测试环境不可达（Rust 侧 JSON 解析必失败）——假端点保真度决定可验证性上界。
4. 非标布局（顶层 services）在环境兜底下长期潜伏（593 号教训第二例）：层 1 显式解析一接入即暴露——「标准布局核验」对脚本 fixture 同样适用。
