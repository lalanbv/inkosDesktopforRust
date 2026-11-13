# 1014 号 · pi 双栈升级专项勘察轮——实装 .d.ts diff + 升级真红回滚 + typebox 跳变定性

> 1013 出口清偿（「pi 1.5 升级专项·契约勘察轮」）。本轮=勘察+**升级试探执行**：
> earendil 三件 0.87.1→1.5.2 与 mariozechner 0.67.1→0.73.1 双线 API 面 .d.ts
> 实装 diff（/tmp 探针目录 npm 实装）+ 仓内 48 文件用面符号逐一对照 + mz 升级
> 实跑（typecheck 抓真红→回滚复绿）。产出=升级路径决策台账 + 触发条件链修正。

## 一、双栈定性修正（1013 台账勘误）

- **mariozechner 0.67.1 = 运行时主力**：38 文件（harness/tools 全家 15+agent 域 6+llm 域 4+agents/play/harness 上下文 13）——provider/pi-stream/agent-session/worker-agent 全链。
- **earendil 0.87.1 = 类型/遥测浅层**：10 文件（telemetry/pi-dispatch/forecast-tools/skill-tool/session-transcript+测试），几乎全 type import+少量运行时值。
- **两包名独立演进非同源**：earendil 1.5.2 pi-ai types.d.ts=706 行（telemetry 集成/cache-warming/auth 面=功能超集）vs mz 0.73.1=404 行（旧线小步维护）；engines 各异（earendil node>=22.19.0 / mz node>=20.0.0）；发布节奏均活跃（earendil 1.5.2 三天前、mz 0.73.1 近期）。
- 1013 台账「earendil=agent 运行时核心」表述修正为上。

## 二、earendil 0.87.1→1.5.2 API 面 diff（12 符号逐一对照）

- pi-agent-core：AgentMessage/AgentTool/AgentToolResult/AgentToolUpdateCallback **4/4 兼容**（export-star 链可达）。
- pi-telemetry：NOOP_TELEMETRY_CONTEXT/InMemoryTelemetryContext/TelemetryContext/TelemetrySpan **4/4 兼容**。
- pi-ai：Api/Context/Model/normalizeContext **4/4 兼容**（types.d.ts 在，export-star 可达）；**Type/Static 断链**——typebox 依赖迁入新子包 pi-schema（1.3.27），pi-ai 不再 re-export；AgentTool.parameters 放宽为 pi-schema builders ∪ TypeBox ∪ JSON Schema。
- **engines node>=22.19.0**——node_min 冲突族第三处（vite 22.12/unpdf 24/pi 22.19）。
- pi-ai deps 大改：openai 7.19/@anthropic-ai/sdk 0.129/@google/genai 2.21 SDK 全家大版本抬升（流式协议行为面需全测兜底）。

## 三、mz 升级实跑：真红→回滚→定性

- 0.67.1→0.73.1（exact 保持）install 后 **typecheck 恰红 2 处**（play-agents.ts:254 `result` unknown——38 文件用面唯一类型红点，破坏面窄但非零）。
- **根因钉死**：pi-ai 0.67→0.73 六 minor 之间上游把 **typebox 0.x→1.x**（deps.typebox ^1.1.24→实装 1.3.27）——仓内 `import { Type } from "@mariozechner/pi-ai"` 的 7 文件拿到 typebox 1.x 构造器，与仓内直装 `@sinclair/typebox@0.34.52`（**35 文件主面**）的 `WorkerResultTool<TParameters extends TSchema>` 泛型约束跨版本断裂→推断回落 unknown。
- **处置=回滚 0.67.1 exact**（typecheck/build/core 1062 复绿）。按 978 纪律：升级前置=typebox 0.34→1.x 全仓统一专项（35 文件主面迁移），非本轮可携行。
- **教训入库**：**0.x 依赖的 minor 必须按 breaking 审查**（semver 0.x 的 minor 允许破坏；上游把跨 major 语义装进 0.x minor 是常态而非例外）——979/983 轮的「依赖刷新」判据对 0.x 包要加严。

## 四、升级路径决策台账（触发条件链）

| 线 | 路径 | 前置（硬） | 体量 |
|---|---|---|---|
| mz 0.67.1→0.73.1 | 符号级全兼容（12+ 符号逐一 FOUND，经 export-star 链修正核对面后） | **typebox 0.34→1.x 全仓统一专项**（35 文件） | 中 |
| earendil 0.87.1→1.5.2 | 10/12 符号兼容；Type/Static 迁 pi-schema（2-3 文件 import 改） | **node_min ≥22.19 专项**（vite/unpdf/pi 三族共同前置）+ SDK 全家行为面全测 | 中 |
| 远期归并 | earendil 1.5.2 单线（超集+活跃主线） | 上两前置+双栈 import 收敛 | 大专项 |

**node_min 抬升专项**=vite(22.12)/pi 1.5(22.19)/unpdf 2(24) 三族升级的共同前置，目标值与桌面壳 engine 组装 node_min 声明联动——升级族触发条件的最大公约数。

## 五、门禁（回滚后终态）

typecheck ✓ + core build ✓ + core 1062/1062 全绿 + 工作树净（本轮零净代码改动——package.json 升降持平）。1013 的 undici exact 6.29.1 钉定在位复查 ✓。

## 六、下一号自 1015 起

出口=typebox 0.34→1.x 统一专项（mz 升级前置，35 文件）/ node_min 抬升专项（三族共同前置）/ 用户侧待办=Fork 推送（含 955-1014）。
