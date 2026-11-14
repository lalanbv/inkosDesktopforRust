# 1027 号 · cache-warming 宿主形态前提勘察轮

> 1026 出口复核。1019 备案的「forecast/skill 链走 earendil pi-dispatch——cache-warming 适用面」假设的**深化勘察**——结论：**暂缓维持，且前置比 API 集成更深**。零代码改动轮。

## 一、API 语义实锤（1.5.2 cache-warming.d.ts）

- `getPromptCacheTtlMs(model, options)`：模型 promptCache tier 的 TTL（模型元数据驱动，caching off 时 undefined）。
- `evaluateCacheWarming(model, promptTokens, phase: "streaming"|"idle")` → CacheWarmingDecision：**由调用方执行的决策函数**（非 streamSimple 内建）。
- `getCacheWarmingRequestOptions(options, signal)`：warm 请求=**重放上一请求**（输出 cap 1 token、无 retries、不替换 provider thread state）。
- `isCacheWarmingReplayable`：Anthropic 预算型 thinking 的重放约束校验（budget_tokens 从 max_tokens 派生→重放预算不同→缓存键不同→不可重放）。

## 二、产品化前提：长驻会话 idle 宿主（仓内无此形态）

cache-warming 是**调用方编排的能力**：warm 请求（重放）需宿主在 **idle 阶段**（无进行中请求）主动发起。仓内消费面形态：

| 消费面 | 运行时 | 会话形态 | idle 宿主 |
|---|---|---|---|
| writer（长篇写链） | mz 0.73.1 | 多章序列（有续暖价值） | **无 cache-warming 能力**（mz 线 dist 零命中） |
| forecast/skill | earendil 1.5.2（pi-dispatch） | **短命单次调用**（调完即弃） | 有能力但**无 idle 宿主** |

→ **两线错位**：有能力的一侧无宿主形态，有宿主形态的一侧无能力。产品化前置=①mz 线同步 cache-warming（上游演进观察）或 ②forecast/skill 链建长驻会话宿主（idle 生命周期管理=架构面新形态）——均非本轮 API 集成可携行。

## 三、决策

**暂缓维持**（1026 定性不变+前置深化备案）。触发条件更新=①上游 mz 线 sync cache-warming 或 ②仓内引入长驻会话宿主架构。当前基线（typebox 1.4.7+mz 0.73.1+earendil 1.5.2）保持。

## 四、下一号自 1028 起

出口=earendil-mz 最终单包合并（远期）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1027）。
