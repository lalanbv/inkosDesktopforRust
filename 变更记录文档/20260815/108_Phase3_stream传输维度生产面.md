# 108 号变更记录：stream 传输维度生产面 + env 族对账终版（CLI env 族维持备案的结论落档）

## 一、背景

107 号候选之二：56 号 env 请求级合并族。勘测落定关键结论：**TS `resolveEffectiveLLMConfig` 的 env 覆盖只存在于 CLI/embedding 模式**（`applyCliProjectConfig`/`applyLegacyEnvConfig`）；**studio 模式显式忽略 env**（`warnIfStudioIgnoresEnv`——"Studio 运行时不会使用 env 中的 INKOS_LLM_* 配置"，服务项镜像走 63 号已移植的 `resolve_effective_llm_studio`）。Rust engine 的服务面即 studio 面——CLI env 族（PROVIDER/SERVICE/TEMPERATURE/THINKING_BUDGET/PROXY_URL/HEADERS/EXTRA_*）对 Rust 无消费面，**维持 56 号备案并落档终版结论**。

勘测同时发现真缺口：**stream 传输维度生产面**——TS `client.stream=false` 时全部调用非流式（不支持 SSE 的端点兼容）；Rust `AgentRouter` 恒 `stream:true`（探测面 99 号已有 stream 维度，生产面缺失）。

## 二、交付

### 1. `AgentRouter` stream 维度（`llm/agent_router.rs`）

- 增 `stream: Option<bool>`（`with_stream` builder，None = 缺省流式；`stream_preference()` 读取）；`chat()` 与 RouterLoopChat（agent_route）参数统一 `stream.unwrap_or(true)`。

### 2. 传输层非流式分支（`llm/streaming_client.rs`）

- `chat_completion` 补非流式路径：整体 JSON——`choices[0].message.content` + `message.tool_calls`（id/name/arguments，代理回环非流式形态）+ usage（TS chatCompletion 非流式同构）；此前非流式请求也按 SSE 解析（空事件 → 空回复）。
- responses 传输的非流式分支 106 号已有——两协议 now 对称。

### 3. 配置接线

- **bin**：`INKOS_LLM_STREAM` env（TS `parseBoolean` 逐字："true"/"1"/"yes" → 流式，其余已设值 → 非流式，未设 → 缺省）。
- **97 层**：`AgentModelOverride` 增 `stream: Option<bool>`——层 1 经新助手 `resolve_configured_service_stream`（服务项 stream 字段）、层 2 取 first entry.stream；agent_route 覆盖路由 `.with_stream(ov.stream)`。

### 4. E2E（`sub108_e2e`，1 测试）+ 契约同步

- `service_stream_false_makes_agent_chat_non_streaming`：服务项 `stream:false`（custom:Quiet + secrets）→ 97 层 1 显式命中 → **chat 请求体 `stream:false` + 响应取自非流式 JSON 路径**（双形态 mock 佐证）。
- 96/99 号两处探测 mock 随 TS 语义现代化（非流式探测期待整体 JSON——原 mock 恒 SSE 在旧解析器下"碰巧"通过）。

## 三、env 族对账终版（56 号备案落档结论）

| TS env 面 | Rust 状态 |
|---|---|
| studio 模式 env 忽略（设计如此） | Rust 服务面 = studio 面——无 env 合并即 TS 语义 ✓ |
| CLI/embedding env 族（PROVIDER/SERVICE/TEMPERATURE/THINKING_BUDGET/PROXY_URL/HEADERS/EXTRA_*） | Rust engine 无 CLI 面；bin 部署端点族（BASE_URL/API_KEY/MODEL/MAX_TOKENS/API_FORMAT/**STREAM 本轮**）为等价 bootstrap——**维持 56 号备案，不移植**（无消费面） |
| INKOS_LLM_STREAM | **本轮接**（bin + 97 层服务项） |
| INKOS_DEFAULT_LANGUAGE | 随 CLI 族维持备案（studio 面语言走 inkos.json） |

## 四、parity 要点

- 非流式请求体与响应解析（message.content/tool_calls/usage）TS 同构；parseBoolean 逐字；服务项 stream 直通（TS applyServiceEntry 的 entry.stream 分支）。

## 五、偏差备案

1. **bin env 的 STREAM/FORMAT 与 inkos.json 的关系**：bin env 只作启动 bootstrap（62 号部署设计）；inkos.json llm.stream/apiFormat 的运行时消费在 /agent 面经 97 层（服务项级），其余面的 router 为启动态——配置热更新语义维持 62 号备案（重启生效）。
2. 非流式 chat 空内容不报错（沿用 Rust 空回复占位语义；TS 非流式空内容报 "LLM returned empty response"——Rust 回环层以"（无回复内容）"呈现，既有偏差族）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1152 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **173** 过（+1：sub108；96/99 两处 mock 随非流式契约现代化） |
| `cargo test --features export-bindings --lib` | 1311 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（109 号候选）

传输层三维度（协议 chat/responses × 流式/非流式 × 探测/生产）全对称接线完毕。候选：

1. **首选：strangler 实切演练**（98 号 runbook 只读面起跑）——功能面、协议面、配置面（服务项 apiFormat/stream/apiKey）对账后无已知缺口；需真实运行环境与流量（请求用户提供）。
2. 其次：非 agent 面（write REST/daemon/forecast/play/film）的运行时配置装配（inkos.json 服务项 → runtime router 热解析——当前为 bin 启动态，62 号备案的潜在升级件）。
3. 或：聊天卡 details 外露 + SSE tool:end 结构化 result（前端契约确认前提）。
