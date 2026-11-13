# 1018 号 · typebox 专项清偿轮——A/B/D/C 四类落地 + pi-compat 宽类型 + Value 错误面备案

> 1017 台账执行。清偿成果=**升级态红点 87 → 10**（A/B/D/C 四类修点全部落地且
> 在 0.34 基线下合法保留——双兼容结构类型/宽容器/边界 cast），剩余 10 处=
> **1.x Value.Errors 产物形态变化的运行时适配面**（keyword/instancePath/params
> 探针实锤，非类型层修），备案 1019 收尾。

## 一、清偿明细（双兼容保留——0.34 基线 typecheck 0 红 + 三包全绿）

| 类 | 修点 | 形态 |
|---|---|---|
| **A** | work-creation（1017 注解改 cast：逆变位不可窄化注解，体内一次 `params as Static<typeof CreateParameters>` 具体化 15 引用）+ bound-production（`params: Record<string, unknown>` 显式） | 逆变位注解不可行→体内 cast |
| **B** | structured-arguments（`SchemaLike` 结构类型：type/items/properties——递归解码消费面 duck-typing，0.x/1.x 双兼容）+ tool-arguments（failed 子 schema 结构 cast）+ 递归签名 `TSchema \| SchemaLike` | 结构消费面与版本解耦 |
| **D** | film-authoring-tools:437 `Type.Composite([StoryNodeToolSchema])` → `Type.Object({...properties}, {additionalProperties: false})`（单元素 Composite 等价改写，StoryNodeToolSchema 纯 Type.Object 产物已核） | 单点改写 |
| **C** | agent-session 1434/1522 双栈边界 cast（`as unknown as Parameters<typeof sanitizeSkillTurnMessage>[0]`）——1037/1519 边界 cast 先例形，sanitize 消费面只读 role/content 核心字段（0.73.1/0.87.1 同形） | 参数面边界 cast（0.73.1 下红点前移自返回面到参数面） |
| **容器群** | 新建 `agent/pi-compat.ts AnyAgentTool`（Omit execute + params any 重声明）——production-capabilities（ProductionAgentTool/intentSkillTool/tool）/explicit-action/bound-production 全量替换 | 泛型逆变（0.73.1 具体参数工具不可赋 unknown-params 容器）的统一宽容器 |

## 二、剩余 10 处=1.x Value 错误面运行时适配（1019）

探针实锤：typebox 1.4.7 `Value.Errors` 产物=**JSON Schema 2020-12 校验器形态**
（keyword/schemaPath/instancePath/params/message）——与 0.x（path/type/message/
schema/value）完全不同。tool-arguments 的错误消费面（path/expectedType/
allowedValues 推导→给 LLM 的结构化修复提示）是**行为面重写**（含测试校准），
非类型层修。capability-registry:114 同型。**修点保留在树上，1019 重装升级时
红点余量=该适配面。**

## 三、门禁与备案

- 0.34 基线（回滚后保留修点）：typecheck **0 红** + core 1062/1062 + studio 893/893 + cli 81/81 + build ✓ + studio typecheck ✓。
- **dist 副本噪声备案**：根目录 `npx vitest run` 会把 `src-tauri/target/*/engine/dist` 与 `packages/*/dist` 的**打包副本测试**重复扫入（109 failed=陈旧产物假象）——门禁口径=**`pnpm -r test`**（per-package include src/**），与 1015/1017 全绿口径一致。
- 升级态重装操作序（1019）：core/package.json 三行（alias+mz 0.73.1）→ install → Value 错误适配 → 全量门禁。

## 四、下一号自 1019 起

出口=**Value 错误面 1.x 适配 + 升级态重装收尾**（红点余量 10 处）/ 用户侧待办=Fork 推送（含 955-1018）。
