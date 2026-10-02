# 613 号：guardedCompleteStream 补 text_delta 渲染链适配——612 回归精准修复（stream:false 分支恢复接线）

- 日期：2026-10-02
- 类型：fix(core+studio)（607/612 非流式完成的渲染链适配闭环）
- 路线：612 回归的精准修复（612 真机走查定位的前端渲染链缺口）
- 状态：✅ 门禁全绿+真机复验通过

## 根因（612 遗留的定位）

612 回归的根因：guardedCompleteStream 只推 `done`（携带完整 message）——**前端打字机消费 `text_delta` 事件**（stream-events → draft:delta），纯 done 事件不含增量导致聊天页回复不渲染。

## 修复（一处，精准）

`guardedCompleteStream` 成功分支推 `done` 前先推 **`text_delta` 事件**（完整文本作为单个增量，与 pi-ai 事件形态同构：contentIndex/delta/partial）——前端渲染链（draft:delta 消费）收到打字机内容，非流式完成的 UX 为「一次性出现全文」（stream:false 的语义即此）。

## 恢复接线

agent-session streamFn 恢复 608 回滚的 `config.streamPreference === false → guardedCompleteStream` 分支（613 注记替换 608 回滚备案）。

## 验证

- guarded-complete-stream 单测更新：事件序断言 `text_delta → done`（成功分支）/ `error` 终结（失败分支）——2/2 绿；
- **真机复验**（新环境 4602+mock 4319）：**不配置不选模型直接发**——回复正常渲染（SECTION 在场）、无拦截、chip 回填——零配置直发+非流式渲染链全链贯通（612 回归修复实证）；
- gate:ts 七步全绿。

## 下一号自 614 起
