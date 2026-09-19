# 535 号：use_skill 工具卡激活预览与分组白名单修复

日期：2026-09-19
提交：本文件同批路径限定提交

## 选题来路

备案候选（534 轮遗留）之一：「聊天轮 use_skill 激活的 RunLog/tool_executions 前端展示核对」。走查即坐实缺陷，核对轮转为修复轮。

## 走查链与缺陷定位

自模型侧到人侧逐环核对 use_skill 激活结果的可视链路：

1. **工具执行体（双端，已对齐）**：TS `createUseSkillTool`（skill-tool.ts）与 Rust `tool_use_skill`（interaction/skill_tool.rs）的 tool result 均为「文本主体 + details 元数据」，details 形态逐字同构：`{ kind: "skill_activated", skillId, resourcePath?, query?, retrievedResources?: [{path, heading, charStart, charEnd, score}] }`。检索段的段级元数据由 534 号修复补齐（此前写入端恒 None）。
2. **SSE 透传（双端，通）**：TS 聊天轮 tool:end 载荷带 `details`（server.ts tool_execution_end 分支）；Rust `SseBridge::on_tool_end`（agent_route.rs）details 进 payload（None 时键缺省，114 号对齐形态）。
3. **前端 store（通）**：`stream-events.ts` 的 tool:end 监听 `data.details ?? extractToolDetails(data.result)` 写入 `execution.details`。
4. **渲染面（缺陷所在，两处）**：
   - **缺陷 A（分组错位，主缺陷）**：`isPipelineTool` 白名单缺 `use_skill`——工具被 `groupToolExecutionsChronologically` 归入「N 个文件操作」折叠组（UtilityToolsGroup），主卡 PipelineExecution 及其全部 Preview 对 use_skill 一律不渲染。技能激活是 532 号刚闭环的语义级动作，人侧在聊天流里却完全不可见（240 号移植工具本体时即归错组，潜伏至今）。
   - **缺陷 B（结构化详情零消费）**：`details.kind === "skill_activated"` / `"skill_expired"` 在前端零分支——同文件 chapter_written / chapter_revision / chapter_state_resynced 等均有专用 Preview，唯独 skill 激活没有。`SkillUsagePreview` 消费的是 `details.skillIds`（复数，写作链/sub_agent run 快照面），与 use_skill 的 `skillId`（单数）不是一回事。

## 修复

`packages/studio/src/components/chat/ToolExecutionSteps.tsx`：

1. **白名单**：`isPipelineTool` 增 `use_skill`——独立主卡，激活动作在聊天流可见。
2. **`getSkillActivationDetails(exec)` 导出解析函数**：`kind:"skill_activated"` → `{ skillId, expired:false, resourcePath?, query?, resources[] }`（resources 逐项校验 path 非空才收）；`kind:"skill_expired"` → `{ skillId: 从 args 回取, expired:true }`（历史回放时 transcript-restore 只给 kind 不给 skillId，见下）；其余 → null。
3. **`SkillActivationPreview` 组件**（挂主卡 SkillUsagePreview 之后）：
   - activated 态：「激活 Skill」标签 + skillId chip + resourcePath chip + query 行 + 检索段明细列表（`path:charStart-charEnd` 等宽体 + 段题 + `相关度 N.NN`）；不渲染 body 全文（全文已在折叠的 PipelineResultDetails 里，预览只做定位）。
   - expired 态：灰条「技能指令已过期 · skillId chip · ——原轮激活的指令不再重放」。
4. **历史回放语义承接**：`session-transcript-restore.ts` 对 use_skill 刻意置换为 `{kind:"skill_expired"}` + 固定过期文案（技能指令有时效性，回放不重放旧指令）——预览的 expired 态与该机制对齐，刷新/恢复会话后卡片形态仍正确。

## 测试（ToolExecutionSteps.test.ts，新增 4 组，38/38 绿）

1. activated 全字段：chip/query/两段检索项（path:charStart-charEnd、heading、相关度）逐项断言。
2. resourcePath-only：无检索段时不渲染「相关度」。
3. expired：过期文案 + args 回取的 skillId + 不含 activated 态文案。
4. 负例：sub_agent 的 chapter_written details → `getSkillActivationDetails` 为 null，且既有「专业 Skill」chip 面不受影响。

## 实现注记

- JSX 检索段定位串改模板字面量单表达式（`{`${path}:${start}-${end}`}`）——多个相邻插值子节点会被 React SSR 以 `<!-- -->` 注释切开，断言与真实 DOM 文本都不连续（`renderToStaticMarkup` 实测抓获）。
- 本轮纯 studio 前端 diff（组件 + 测试两文件），零引擎面/契约面改动：Rust 双 crate test、clippy、契约差分器、双冒烟套件、duel、bench 的覆盖面均不涉及（533 轮 radar 纯 studio 轮同裁剪先例），TS 门禁 `gate:ts:fast` 为本轮门。

## 教训

1. **白名单/分组器是隐形接入面**：新工具移植（240 号 use_skill）只核对了执行与注入链，工具卡分组器漏改——展示面缺陷潜伏 300 号轮次级时长。今后新工具接入清单须含「isPipelineTool 白名单核对」。
2. **「查链路两端」的前端版**：534 号教训是字段管道要查写入端与读取端；本轮是数据到齐后还要查**消费末端**（渲染分支），SSE→store 三环全通不代表用户看得见。
3. 备案候选的价值在走查本身：本轮从「核对展示」开工，走查即坐实两处缺陷，核对轮转修复轮。

## 下一号

自 536 起（533 被并行会话 radar 路由轮占用、534/535 本会话已用；开号与收口双端双查 git log）。
