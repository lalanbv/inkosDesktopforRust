# 1022 号 · earendil 单线归并轮——1.5.2 落地 + 工具 schema 统一 typebox 面

> 1021 出口执行（前置全解锁：1014 勘察/1016 node_min 22.19/1018-1019 兼容层/
> 1020 node 写链修复）。**earendil 三件 0.87.1→1.5.2 落地**——pi 双栈归并的
> earendil 侧收口。

## 一、符号审计（14 符号全清单探针）

earendil 1.5.2 对仓内用面=**13/14 兼容**：pi-agent-core 4/4（AgentMessage/AgentTool/AgentToolResult/AgentToolUpdateCallback）+pi-telemetry 4/4（NOOP/InMemory/TelemetryContext/TelemetrySpan）+pi-ai 5/6（streamSimple/normalizeContext/Api/Context/Model）——**仅 Type/Static 断链**（pi-ai 1.5.2 不再 re-export，1014 已勘）。

## 二、归并中的关键定性反转：pi-schema 不是迁移目的地

1014 台账预设「Type/Static 迁 pi-schema」——实装探针**推翻**：pi-schema 1.5.2=earendil 自带的**独立 schema 系统**（小写 builders `obj/str/num/arr/...`，非 TypeBox 兼容层），其 Schema 产物与 mz 0.73.1 的 typebox TSchema 校验链**不互通**。仓内工具 schema（forecast-tools/skill-tool）的消费方=mz agent loop 的 typebox Value 校验链——**必须保持 typebox 形态**。

**修正**：Type/Static 改从 **`@sinclair/typebox`（=typebox 1.4.7 alias 面）** 导入——与仓内 35 文件及 mz 0.73.1（依赖 typebox ^1.1.24 同 1.x 线）同源互通。**earendil 的 pi-schema 不进入仓内消费面**（仅 earendil 内部使用）。

## 三、门禁终态

- typecheck **0 红**；三包全绿 **core 187 文件 1067 + studio 893 + cli 81**；
- cargo:testgate engine-rs 2114 + src-tauri 593 双绿；clippy:gate 双 crate 零告警；
- gate:ts 十腿（终态见提交时运行——**export-epub-smoke=node 写链活体验收**）。

## 四、升级总账（1013→1022 十轮链）——pi 双栈归并收口

1013 评估 → 1014 勘察 → 1016 node_min → 1017 归并方案 → 1018 清偿 → 1019 Value 适配+安全语义 → 1020 node 写链根因修复 → **1022 earendil 1.5.2 归并落地**。

**双栈终态**：mz 0.73.1（运行时主力 38 文件）+ earendil **1.5.2**（类型/遥测/dispatch 浅层 10 文件）——两线各自 latest。earendil 与 mz 的**最终合并**（单包终态）仍为远期出口（取决于上游 mz 线是否跟随 earendil 1.x——双名发布若收敛则自然归并）。

## 五、下一号自 1023 起

出口=下一巡检周期 / 回归审查轮（1014-1022 新增面）/ 用户侧待办=Fork 推送（含 955-1022）。
