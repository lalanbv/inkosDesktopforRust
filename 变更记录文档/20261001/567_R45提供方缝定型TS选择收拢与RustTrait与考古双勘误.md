# 567 号：R45 LLM/subagent 提供方缝定型——TS Provider 选择收拢+Rust trait LlmProvider+考古双勘误

- 日期：2026-10-01
- 类型：refactor(engine+core)（纯形态定型，零行为变更面）
- 路线：dsh 对标 v7 P2 R45（541 号 §表 L114「R30 迁移在先，R45 只做形态升级」）
- 状态：✅ 门禁全绿收口

## 考古勘误两处（v7 施工面前提复核）

1. **subagent 嵌套轨迹「接线备案」已陈旧**：agent_trajectory.rs 模块 doc 称「结构已备，接线备案」，考古证实 **135 号已双端接线**（Rust `tool_sub_agent` 执行体整体包 `derive_subagent()` 作用域 + TS agent-tools:950 `runWithAgentTrajectoryRole("subagent")`）。本专项仅修正陈旧注释（567 号勘误），非增量。
2. **537 号 daemon verdict 化载体已清偿**：540 号已完成 daemon 判定化——R45 表述中该立案载体前提不再成立。

## 交付面

### A. TS Provider 选择收拢（Consumer 不辨提供方身份）

- 考古：stub/真传输双实现已在（llm-stub INKOS_AGENT_LLM_STUB+pi 传输），但 provider 选择散布三处调用点（provider.ts:1464 缝内✓、agent-session:968、worker-agent:317 在缝上）。
- `pi-stream.ts` 新增 `guardedAgentStream`（stub→pi 选择收拢，模块 doc 声明三角色边界：Service Definition=LLMConfig/端点解析、Provider=pi 传输+llm-stub、Consumer=agent 编排层）；agent-session streamFn 收敛为单缝调用。
- `llm-stub.ts` 新增 `stubWorkerStructuredResult`（stub JSON→resultTool 解析是 stub 实现细节，归 Provider 角色；undefined 哨兵回落真传输）；worker-agent 的 env 分支+手写解析收敛为单调用（顺带清 Value 死导入）。
- 测试：llm-stub.test.ts +2（选择双态：无 env→undefined/有 env→解析；guardedAgentStream 返回可迭代流——stub 不触网不炸）。

### B. Rust `trait LlmProvider` 定型（stream/usage 声明）

- `streaming_client.rs`：`#[async_trait] trait LlmProvider`（`stream_chat(&ChatCompletionParams) -> Result<StreamedCompletion, StreamError>`）+ 生产实现 `impl LlmProvider for StreamingChatClient`（委托固有方法，显式路径限定不构成递归；**固有方法优先=既有 Consumer 调用点零漂移**）。
- 三角色入 doc：Service Definition=`LlmEndpointConfig`/`ResolvedEndpoint`+providers_bank lookup；Provider=trait 实现；Consumer=`AgentRouter::chat`/管线/RouterLoopChat。usage 面随 `StreamedCompletion`（prompt/completion/total tokens）喂 R41 计量锚点。
- **定型证明=测试内 MockProvider 第二实现**：`provider_seam_accepts_second_implementation` 经 `dyn LlmProvider` 分发（非 HTTP 提供方可接纳：进程内 mock/未来本地推理）。
- price 声明**备案不做**：仓库零定价数据源（providers_bank/TS 模型卡均无 price 字段），发明价格表=伪数据；待真实消费方（成本观测）立项再入契约。

### C. 备案（v7 表四项裁两）

- **引擎 rust/node 选择 seam 化：不做**。该选择是**部署层**决策（164 号 env>配置>miss 回退，桌面壳拉起链），与 LLM 提供方不同层次；强塞同一 seam=层次混淆（v7 不采纳清单精神）。R45 的「提供方」语义落在 LLM 面已足。
- subagent 提供方接口预留：当前唯一提供方=进程内（双端轨迹已接线），外部提供方无近期真实消费方——接口预留=死抽象，备案待立项（到达时沿 `LlmProvider` trait+既有子作用域结构落地）。

## 教训

- 施工图表述依赖的「立案载体」要复核后State：537 daemon 载体（540 已清偿）、subagent 接线备案（135 已接线）两处前提过期——与 565 号「新增文件前提失实」同源第四例。
- Rust trait impl 委托固有同名方法：`Type::method(self, …)` 显式路径限定解析到固有方法，不构成递归；trait 插入位置须在 impl 块结束后（本专项曾误插块中提前闭合，编译当场拦截）。
- perl 单行替换 `@xxx` 会被当数组插值吞掉（`@sinclair`→空）——含 `@` 的替换用 `\@` 或改用 Edit 工具。

## 门禁

- clippy:gate 双 crate 0 告警
- cargo:testgate：engine-rs 45 目标 1893 passed（+1 seam 定型测试）+src-tauri 23 目标 576
- gate:ts 七步全绿（core llm-stub 4/4+agent-session/provider 105/105）

## 关联

- 前序：554（R30/R30b pi-ai 迁移+provider.ts 单适配缝）、563（R41 usage 锚点）、135（subagent 轨迹接线）、540（daemon verdict 化）
- 路线：v7 P2 剩 R46（WASM 插件宿主契约收敛）按需立项
- 下一号自 568 起
