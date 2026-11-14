# 1021 号 · 回归审查走查轮——typebox 升级态四审查面 + 适配层契约锁定

> 1020 出口执行（1014-1020 七轮新增面回归审查）。四审查面逐面定性，**三面净
> + 一面补测试增量**（NEUTER1021 恰红 1/1 锚定）。活体走查由 1020 验收覆盖
> （export-epub-smoke exit=0 全链+gate:ts 三活体腿绿）——不重复执行。

## 一、四审查面定性

| 面 | 勘察 | 结论 |
|---|---|---|
| **A. schema 结构消费面复扫** | 全仓 `.schema.type/.items/.properties/[Kind]` 运行时消费 grep | structured-arguments/tool-arguments=SchemaLike 化 ✓；reviser:167/176 `contract.parameters.properties`=TObject 具体类型（1.4.7 探针证 properties 在产物）；settler-tool:71-72 spread 同 ✓；`[Kind]` symbol 零消费 ✓——**净** |
| **B. Value.Errors 适配层边界** | valueAtPath（RFC 6901 转义序 ~1→~0 ✓/数组下标/越界/空段）+keyword 形态 | 实现边界齐——**补直接契约单测 5 例**（type 全面重建/Union 聚合/嵌套数组下标精确定位/顶层空路径/合法输入零错误） |
| **C. pi-compat 宽容器消费点** | AnyAgentTool 5 处类型引用+execute 调用 2 点（production-capabilities:537/bound-production:35） | 调用传参 params any→下游 schema 校验链保证=设计意图注释一致——**净** |
| **D. writer-settle 投影完备性** | 投影 9 字段 ↔ HookRecordToolSchema 9 字段 ↔ HookRecordSchema.zod 9 字段**三面对照** | 7 必需+2 可选（dependsOn/paysOffInArc）逐字段一致——**完备** |

## 二、测试（+5）

`tool-arguments.test.ts` 新建：适配层直接契约单测（真实 Value.Errors 全链——type 重建/Union 聚合/RFC 6901 嵌套定位/顶层空路径/合法零错误）。**NEUTER1021**：allowedValues 聚合分支摘除→Union 例恰红→互逆恢复。

## 三、门禁

三包全绿 **core 187 文件 1067（+5）+ studio 893 + cli 81**；typecheck 0 红；U+FFFD 净。

## 四、下一号自 1022 起

出口=earendil 1.5.2 单线归并专项（远期）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1021）。
