# 1039 号 · 运行时可达性勘察轮——earendil 1.5.2 API 面重构实锤，迁移工程重估

> 1038 出口（迁移轮 A 续）执行前的前置探针——**重大发现改变迁移工程定性**。

## 一、运行时可达性探针（node 实测 1.5.2 主入口，20 符号）

| 面 | 可达 | MISSING |
|---|---|---|
| pi-ai 1.5.2（10） | **createAssistantMessageEventStream + normalizeContext（2）** | Api / Context / Model / Type / Static / **getEnvApiKey / getModel / streamSimple（8）** |
| pi-agent-core 1.5.2（10） | **Agent（1）** | AgentEvent / AgentMessage / AgentTool / AgentToolResult / AgentToolUpdateCallback / BeforeToolCallContext / UserMessage / AssistantMessage / ToolCall（9——全 **type-only**） |

**与 .d.ts 声明面完全不同**：1014 探针（.d.ts grep）=13/14 FOUND——那是**类型层**；运行时层 1.5.2 的 exports 重构后**值符号大幅收缩**（streamSimple/getModel/getEnvApiKey 运行时 MISSING——声明在 dist/*.d.ts 但主入口运行时不可达，实际可达路径待 1.5.2 子路径 exports 深探）。

## 二、迁移工程重估

- **1014/1037 预估**：类型面 78 红点（轮 A）+行为面（轮 B）=中专项。
- **实态重估**：类型面 78 红点（1038 实测）+ **运行时 API 面重构**（pi-dispatch 的 streamSimple/getModel/getEnvApiKey 三个运行时函数在 1.5.2 无直可达路径——需子路径深探或替代 API）+ Agent 类语义（轮 B）=**大专项**（多轮）。

## 三、决策：优先级回落+触发条件制

- mz 0.73.1 虽 EOL 冻结但**功能稳定**（2941 例全绿+export-epub/IAB 活体）——维持现状合理。
- **earendil 归并优先级：中期→回落待触发**。触发条件=①mz 供应链安全事件（EOL 风险兑现）②真实需求需要 earendil 1.5.2 独有能力（cache-warming 需 idle 宿主、images-api 生态）③上游 mz 复活。
- 本轮产出=运行时可达性探针数据（迁移工程的真实基线）。

## 四、下一号自 1040 起

出口=earendil-mz 单包合并（**触发条件制**）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1039）。
