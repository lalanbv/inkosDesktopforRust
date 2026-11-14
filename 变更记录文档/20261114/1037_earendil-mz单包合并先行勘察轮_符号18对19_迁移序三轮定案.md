# 1037 号 · earendil-mz 单包合并·先行勘察轮——符号 18/19 + 迁移序三轮定案

> 1036 出口执行（单包合并专项先行勘察）。**零代码改动轮**。核心结论：
> **符号级迁移面=零缺口**——真正的迁移风险全部在**运行时行为面**（Agent 类
> 语义），与 1014 的浅层结论一致但本次覆盖 mz 主力 38 文件全量。

## 一、mz import 符号全清单（38 文件聚合=19 符号）

pi-agent-core（10）：Agent / AgentEvent / AgentMessage / AgentTool / AgentToolResult / AgentToolUpdateCallback / BeforeToolCallContext / UserMessage / AssistantMessage / ToolCall
pi-ai（9）：Api / Context / Model / Static / Type / createAssistantMessageEventStream / getEnvApiKey / getModel / streamSimple

## 二、对照 earendil 1.5.2（/tmp 探针逐符号核对）

- **18/19 FOUND**：agent-core 全 10（含 mz MISSING 的 ToolCall——earendil 反有）+ pi-ai 运行时函数 4/4（streamSimple/getModel/getEnvApiKey/createAssistantMessageEventStream）+ Api/Context/Model/normalizeContext。
- **Type/Static 断链**（pi-ai 1.5.2 迁自带 pi-schema）——**迁移路径已定案**（1022）：改 `@sinclair/typebox` alias 面（typebox 1.4.7，与 mz 0.73.1 的 typebox ^1.1.24 同 1.x 线互通）。

## 三、迁移风险重定性：符号零缺口，行为面是全部

| 行为面 | 已知证据 |
|---|---|
| **Agent 类 loop 语义** | 0.73.1 已现 beforeToolCall 抛错 fallback/attempts 差异（1019）；1.5.2 又一版（需行为测试校准） |
| **streamSimple 协议细节** | writer 链全走（1.5.2 流式协议/错误形态——flow 测试校准） |
| **参数清理行为** | 0.67.1 隐式剥离→0.73.1 不剥（1020 kind 实锤）→1.5.2 行为待探（1020 投影/Clean 防线为迁移铺垫，双防线在位） |
| **Value.Decode 清理语义** | 1.x 已实锤（1019 Check-before-Decode 修复在位） |

## 四、迁移序定案（三轮）

1. **轮 A（类型+浅层）**：pi-ai/pi-agent-core 的 import 源切换（mz→earendil 1.5.2；Type/Static→typebox）——typecheck 驱动，预计红点=签名微差。
2. **轮 B（运行时行为）**：Agent 类用法迁移（agent-session/worker-agent 的构造/loop/事件语义）+ 行为测试校准（stream-contract/work-initial 等 flow 的形态断言）+ 1020 投影/Clean 防线再验证。
3. **轮 C（收尾）**：export-epub-smoke+IAB 写链活体+全量门禁+package.json 清理（mz 双件移除）。

## 五、下一号自 1038 起

出口=**迁移轮 A（类型+浅层切换）** / 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1037）。
