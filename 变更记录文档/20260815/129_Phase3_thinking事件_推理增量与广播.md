# 129 号变更记录：thinking:start/delta/end 三事件——推理增量解析与广播补齐（词汇表 66/66 收官）

## 一、勘测结论（修正 126 号的一处误判）

126 号曾备案"thinking:* UI hub 未订阅"——**误判**：订阅不在 `use-sse.ts` 的 STUDIO_SSE_EVENTS 列表，而在 `stream-events.ts` 的**第二个 EventSource 订阅**（`attachSessionStreamListeners` 对 `/api/v1/events?sessionId=…` 直接 `addEventListener("thinking:start"/"thinking:delta"/"thinking:end")`——实时思考面板的通道）。

事件源链（全在仓外依赖 pi-ai 0.67.1）：

- **解析**：`pi-ai openai-completions` 流式循环——`reasoning_content | reasoning | reasoning_text` **首个非空字段**（llama.cpp / OpenAI 兼容各形态 + chutes.ai 双字段防重复）；
- **事件**：首条推理增量 → 关当前块开 thinking 块 → `thinking_start`；每条 → `thinking_delta {delta}`；块切换/结束 → `thinking_end`；
- **广播**：server.ts onEvent ame.type 分支 → `thinking:start {sessionId}` / `thinking:delta {sessionId, text}` / `thinking:end {sessionId}`。

Rust 侧现状：sse_parser 只解 `delta.content`，无 reasoning 字段；无 thinking 广播——**需补齐**。

## 二、交付

### 1. 解析层（sse_parser + streaming_client）

- `SseEvent::ReasoningDelta(String)`：`reasoning_content | reasoning | reasoning_text` 首个非空（pi-ai reasoningFields 逐字），同帧 content 优先；
- `StreamedCompletion.reasoning`：chat 流式路径累积（与 content 分离不回填——pi-ai thinking 块语义；纯推理响应正文为空 → 「（无回复内容）」回退与 TS 同构）。

### 2. 广播层（agent_route）

`ThinkingBridge`（hub + sessionId）：`RouterLoopChat` 每轮 chat 返回后若 reasoning 非空 → 三连发 `thinking:start/delta/end`。**聚合语义**——与 66 号 draft:delta 每轮聚合同款（Rust loop 端口为聚合返回；单块推理下与 TS 逐 delta 形态等价，多块为一次聚合 delta——UI batcher 拼接语义下显示等价，备案）。

### 3. duel 词汇扩充（第 8 测试 4 → 7 类）

- mock 流式形态加 `reasoning_content` 增量（驱动两侧 thinking 面）；
- 共享词汇 += thinking:start/delta/end——**双端（TS sidecar pi-ai 真实链 vs Rust bin）序列与负载形态逐键等价**（含 thinking:delta text 值逐字相等）。

### 4. 备案（非本次范围）

- responses 传输的 reasoning summary 事件（pi-ai responses 面）——fixture chat 格式未覆盖；
- 非 stream 偏好下的 thinking（Rust 非流式 reasoning 恒空）；
- transcript 持久化 thinking（TS pi-agent AssistantMessage thinking 块 vs Rust 纯文本——会话恢复时思考面板不回放）。

## 三、测试（+3：lib 1169 / E2E 188）

- `reasoning_delta_first_non_empty_field_wins`（parser：优先级 + content 同帧优先 + 别名兜底）。
- `chat_broadcasts_thinking_events_for_reasoning_models`（e2e：reasoning mock → agent:start → thinking 三事件（start 带 sessionId / delta 带 text+sessionId / end）→ draft:delta → agent:complete 全序）。
- duel 第 8 测试词汇扩充（七类事件齐全断言 + 双端逐负载等价）。

## 四、验证基线（129 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1169** 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **188** 过 |
| `cargo test --features export-bindings --lib` | **1328** 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | 8/8（SSE 面词汇 7 类） |

## 五、事件词汇表终态：**66/66**

TS sidecar 66 事件全数对齐（63 实装 + 3 thinking 本号补齐）。SSE 事件面迁移收官。

## 六、下一步候选

1. 实切条件到位后真实流量切换（duel 8/8 前置门）。
2. 备案项按需展开（responses thinking / transcript thinking 持久化）。
3. 按需轮（用户指定）。
