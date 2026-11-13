# 1017 号 · typebox 统一专项勘察轮——alias 归并试探 + 红点群四类定性

> 1016 出口转正。升级试探（alias 归并 + mz 0.73.1 实装）→ typecheck 红点群
> 四类现形 → 回滚复绿（1062/1062）→ 台账定性。**专项改判多轮推进**：1017=
> 红点群勘察，1018+ 逐类清偿。

## 一、生态定案（探针实测）

- **`typebox` 1.4.7 = @sinclair/typebox 的 1.x 线新包名**（同作者 sinclairzx81/typebox，ESM-only，TS 7 native 对齐）；`@sinclair/typebox@0.34.52` = 0.x 线终点（仓内现版即 latest 0.x）。迁移=包名+主版本双跳。
- **1.4.7 兼容面实测**（/tmp 探针）：`import { Type } from 'typebox'` 运行时 ✓（Object/Optional/String 全在）；`type Static` type-only 导出在根 ✓（1.x 拆 StaticType/StaticDecode/StaticEncode/StaticParse 方向参数化，Static<T> 兼容导出仍在）；`import { Value } from 'typebox/value'` 运行时 ✓（Value.Check/Parse 活体）；`.properties` JSON Schema 标准字段保留 ✓；**0.34 的 `[Kind]` symbol 机制仓内零直接消费** ✓。
- schema 产物形态变化：keys=["type","required","properties"]（JSON Schema 2020-12 顶层化）——依赖 schema 结构的仓内面=仅 `.properties` 消费（兼容）。

## 二、归并方案（1018+ 执行形态）：pnpm alias

`"@sinclair/typebox": "npm:typebox@^1.4.7"`——**35 文件 import 零改动**（Type 运行时+Static 类型+Value 路径同构全兼容），mz 内部 typebox 与仓内 alias 同生态归并（本轮实测：仓内 alias 解析 1.4.7 ✓，mz 线自带 1.3.27 实例共存——pnpm dedupe 决策，结构兼容不强求单实例）。

## 三、升级试探红点群（四类 breaking 实锤，typecheck 真红清单）

| 类 | 实锤点 | 破坏内容 | 清偿方向 |
|---|---|---|---|
| **A. execute params 收紧**（any→unknown） | work-creation:35/37（**已修**：显式 `params: Static<typeof CreateParameters>` 注解，0.34 下合法保留）+ agent-tools:664 | 0.73.1 `AgentTool<TParameters,TDetails>` 双泛型，`AgentTool<any>` 绑 TParameters 后 Static 链回落 unknown | 逐点显式类型化（或 AgentTool<any,any> 双参补齐） |
| **B. TSchema 结构面收窄** | work-initial-production.flow:77（`.properties` 不在 TSchema 基接口） | 1.x TSchema 基接口无 properties（TObject 子类才有） | 具体化类型（TObject）或类型守卫 |
| **C. 双栈 AgentMessage 结构分裂** | agent-session:1433/1434/1519 | mz 0.73.1 AgentMessage 与 earendil 0.87.1 AgentMessage **结构不再互相赋值**（0.73.1 加字段后差异超结构兼容） | 双栈交界点类型锚（adaptor/显式转换）——**双栈归并的硬压力点** |
| **D. 1.x 移除面** | film-authoring-tools:437（`Type.Composite` 不存在） | 1.x 移除/改名 Composite | 查 1.x 等价（Composite→Intersect 组合？）逐点改 |

## 四、决策与回滚

红点群跨 5+ 文件四类，每类需测试面验证+双栈 golden 复核——**一轮内收完风险=收尾草率**，按 1014 形态回滚复绿（typecheck/build/core 1062/1062），台账定性改判多轮。**保留面**：work-creation params 显式注解（0.34 下合法的纯增强，为 1018 铺路）。

## 五、下一号自 1018 起

出口=**typebox 专项逐类清偿**（建议序：A 独立点先行→B→D 单点→C 双栈交界最后——C 依赖 earendil 1.5.2 归并决策，可能直接跳到单线终态）/ 用户侧待办=Fork 推送（含 955-1017）。
