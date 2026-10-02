# 607 号：Node 聊天路径尊重服务「流式响应」开关——stream:false 非流式完成落地（606 备案的 Node 侧缺口闭合）

- 日期：2026-10-02
- 类型：feat(core+studio)（行为变更：配置 stream:false 的服务在聊天路径改走非流式完成）
- 路线：606 备案的 Node 侧缺口实施（层 3 stream 双端分歧的 Node 半边）
- 状态：✅ 门禁全绿+真机探针通过

## 缺口（606 备案的 Node 半边）

服务详情「流式响应」开关（108 号 `stream` 偏好）在 Node 回退端**聊天路径被忽略**：`streamFn` 架构恒走 `guardedAgentStream` 流式——配置 `stream: false` 的服务（流式不稳定的端点）用户偏好被无视。Rust 层 3 在 605 已尊重配置。

## 实施（三层链路）

1. **`pi-stream.ts` `guardedCompleteStream`**：非流式完成→单事件流适配——`piCompleteSimple`（非流式完成，等完整结果）+ 守卫链保留（窗口断言/轨迹头）+ `queueMicrotask` 一次推入 done 并 end；失败推 error 事件（worker-agent.ts:223 同款形态）。
2. **`agent-session.ts`**：`AgentSessionConfig` 新增 `streamPreference?: boolean`（undefined/true=默认流式；false=非流式）；`streamFn` 分支——`config.streamPreference === false` → `guardedCompleteStream`。
3. **`server.ts` /agent handler**：服务 stream 偏好解析——显式 `reqService` 优先，零显式按 secrets 首个有 key 服务（与后端层 3/4 解析序一致），读 `resolveConfiguredServiceEntry(...)?.stream` 传入 `runAgentSession`。

## 验证

- gate:ts 七步全绿（core+studio tsc 0；test 全绿含既有流式路径回归）；
- **真机探针**（新环境 4600：inkos.json llm.services custom:MockNF 带 `stream: false` + mock 4319）：零显式直发**成功**——前端放行（无「请先选择一个模型」拦截）+ 后端层 3 secrets 兜底命中 custom:MockNF（配置 baseUrl 探测模型）+ 非流式完成返回 mock 回复——607 全链（前端放行→层 3 兜底→非流式完成）贯通。

## 边界

- 流式路径（默认）行为零变更；非流式分支 UX 为等待完整回复（无打字机增量）——`stream:false` 的语义即此（用户自选）；
- Rust 层 3 在 605 已尊重配置——双端兜底路径的 stream 语义就此对齐（606 备案的分歧闭合）；
- Rust 侧无对应改动（Rust 层 3 已尊重配置）。

## 下一号自 608 起
