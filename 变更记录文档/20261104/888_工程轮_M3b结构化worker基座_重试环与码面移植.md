# 888 号 · 工程轮：M3b——结构化 worker 基座（runWorkerAgentTool 等价环）

> 886 设计 §三 M3b 落地。新增 `production/structured_agent.rs`（≈470 行环+300 行
> 测试），通用件供 M3c 四代理与 M4 复用。零 TS 行为变更（仅测试文件补一条金标准
> 断言）。

## 一、精读定案（base.ts 224 行 → 真源在 worker-agent.ts）

- **重试环不在 BaseAgent**：`submitStructured` 只是「技能注入包装 + runWorkerAgentTool
  转发」——环本体在 `agent/worker-agent.ts`（488 行）的 runWorkerAgentTool。
- **环语义清单**（Rust 基座逐条对齐）：
  1. system 合并（\n\n）+ finish-by-calling 尾追加（逐字金标准）
  2. `maxTurns ?? 3` 模型轮预算；预算闸在**下一轮开始前**（streamFn 调用时）
  3. `toolChoice: "required"` 传输面（TS 测试已断言——Rust 经 extra 顶层透传
     等价，build_chat_completion_request 的 extra 合并不含保留字段）
  4. `submitted` 单槽定稿：首次 validate 成功即终，后续 result 调用 →
     WORKER_RESULT_ALREADY_FINALIZED；**supporting 调用同轮定稿 →
     WORKER_RESULT_READBACK_REQUIRED**（须下一轮读回结果）
  5. supporting 非空 → 每轮 system 追加预算说明（workerBudget JSON，response
     递增）+ lastSupportingTurn 记录
  6. validate 失败 → 错误经 toolResult 回喂（环自动重试）；耗尽 →
     WORKER_RESULT_INVALID（lastValidation.code 透传 + attempts）
  7. no-call 一次性补轮（`You did not call {name}. Call it now…`）；length →
     MODEL_OUTPUT_LIMIT；error → WORKER_MODEL_ERROR；全空 → WORKER_RESULT_MISSING
  8. usage 全 assistant 轮聚合 + onUsage
- **Rust 侧盘点**：streaming_client 已有 tools 透传/tool_calls 聚合/finish_reason
  终态（length→Protocol 守卫在传输层，基座经 ChatTurnError.output_limit 映射
  MODEL_OUTPUT_LIMIT）；tool_choice 走 extra 零改动透传。

## 二、交付面

`production/structured_agent.rs`：
- **注入式 ChatTurnFn**（生产=streaming_client 包裹 M3c 接线；测试=脚本化 fake
  逐轮响应+请求捕获）——mock 双腿形态对齐经捕获断言（tools 首位 result schema/
  tool_choice required/finish-by-calling 逐字）而非真 HTTP duel（真 duel 留 M3e）。
- **WorkerToolSpec/SupportingToolSpec/ValidateFn**（StructuredToolCallback 拆型）+
  七事件码面 + StructuredAgentError（code/attempts/resultTool）。
- 生产适配器（streaming_client 包裹为 ChatTurnFn 实例）留 M3c 首件事（与四代理
  接线一体——需 AgentRouter 运行时配置装配）。

## 三、测试（8 用例全绿）

1. **首轮提交形态金标准**：system 追加逐字（`Finish by calling submit_state
   exactly once…`，前接合并 system \n\n）+ tools[0]=result schema + tool_choice
   required + usage 聚合
2. **变异（验收核心）**：validate 恒红 → 预算耗尽必红（attempts==3 + 末次校验
   码 SHORT_CHAPTER_TOO_LONG 透传 + 第三轮后停召）
3. READBACK：supporting→result 同轮 → 必须下一轮（turns==2 + workerBudget
   response:1/2 递增断言）
4. 已定稿后二次 result 调用 → 拒回喂 + 提交槽不被污染
5. no-call 补轮（金标准文本进 user 消息）后接受
6. output_limit 错误 → MODEL_OUTPUT_LIMIT 码
7. system-only prompt → WORKER_EMPTY_PROMPT 守卫
8. 非法 JSON 参数 → WORKER_SCHEMA_INVALID 回喂 → 下一轮成功

**TS 腿**：worker-agent.test.ts 补第 7 用例——system 合并+finish-by-calling 追加
**逐字断言**（`你是角色状态记录员。\n\nFinish by calling submit_state exactly
once. Do not print…`）——与 Rust finish_by_calling_text + 合并序一次对齐通过。

## 四、验证清单

testgate 2031+584 / clippy 双 0 / gate:ts 八步全绿 / Rust 基座 8/8 / TS
worker-agent 7/7 / U+FFFD 干净。备案：fake 脚本消费顺序坑（Vec::pop 尾弹→
VecDeque 顺序）；生产适配器留 M3c 首件事。**下一轮 = M3c 四代理+prompts+reviser
新形态（reviser.ts 252 行对照勘误前置）。下一号自 889 起；出口不变；用户侧待办
=Fork 推送 22 个提交。**
