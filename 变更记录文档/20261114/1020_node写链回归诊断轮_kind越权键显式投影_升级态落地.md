# 1020 号 · node 写链回归诊断轮——kind 越权键显式投影 + 升级态落地

> 1019 出口执行：export-epub-smoke 复现 → 根因钉死 → 显式投影修复 → **复跑绿
> 验收** → 升级态（typebox 1.4.7 + mz 0.73.1）正式落地。

## 一、根因（现场日志实锤）

node 引擎 server.log：`WriterAgent.settle`（writer.ts）的 `RuntimeStateDeltaSchema.parse` 抛 **`Unrecognized key(s) in object: 'kind'`**（HookRecordSchema `.strict()`）——`hookOps.upsert[i].kind` 为 LLM 越权键（mock 编排携带；真实模型同样可能输出）。

**行为差异链**：typebox 侧 `HookRecordToolSchema` 八字段（无 kind）；0.67.1 的 agent loop 在 tool 执行前**隐式剥离 unknown 参数键**→kind 不穿透；0.73.1 **不再剥离**→kind 穿透 `result.hookOps.upsert` 直达 zod strict→fail-loud。1.4.7 Decode 探针=对 unknown 键同样剥离（Decode 面无罪）——剥离点差异在 **mz loop 内部**。

## 二、修复：边界显式投影（不依赖上游 loop 清理行为）

writer.ts settle 的 zod parse 输入构造处——`hookOps.upsert` **八字段白名单投影**（与 HookRecordToolSchema 对齐：hookId/startChapter/type/status/lastAdvancedChapter/expectedPayoff/notes+可选 dependsOn/paysOffInArc）。

**语义决策**：LLM 参数进入存储 schema 前显式投影=边界卫生不依赖上游 loop 的清理行为（0.67.1 的隐式剥离是未文档化行为，0.73.1 移除即回归）——真实模型的越权键在双版本下统一被投影剥除。

## 三、验收与门禁

- **export-epub-smoke 复跑 exit=0 全绿**（rust 腿 EPUB 结构校验 ✓+node 腿写链落章 ✓）=验收达成；
- 升级态 typecheck **0 红**；三包全绿 core 1062+studio 893+cli 81；testgate engine-rs 2114+src-tauri 593；clippy:gate 双 crate 零告警；gate:ts 十腿（终态见提交时运行）。

## 四、升级总账（1013→1020 八轮链）

1013 评估定钉 → 1014 勘察实装 diff → 1016 node_min 前置 → 1017 归并方案+试探 → 1018 A/B/D/C 清偿 → 1019 Value 适配+安全语义恢复（Check-before-Decode）→ **1020 node 写链根因修复=升级态全绿落地**。

**双栈现状**：mz 0.73.1（运行时主力，六 minor 上游修复消化）+ earendil 0.87.1（类型/遥测浅层）；typebox 1.4.7（alias 归并，ESM-only）；node_min 22.19.0（三声明+双卫兵）。earendil 1.5.2 单线归并（Type/Static 迁 pi-schema）仍为远期出口。

## 五、下一号自 1021 起

出口=earendil 1.5.2 单线归并专项（远期）/ 回归审查轮（1014-1020 新增面）/ 用户侧待办=Fork 推送（含 955-1020）。
