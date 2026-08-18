# 106 号变更记录：responses 协议传输全链 + INKOS env 族对账（96 号备案 #1 闭合）

## 一、背景

105 号候选 + verifier 指令：P3 余量精修——responses 协议传输 + provider 特判族同轮，或 INKOS_* env 族与 TS 全对账。勘测落定范围：**responses 传输全链**（96 号备案"流式层仅 chat completions、深链一律 chat 探测"的实质闭合）+ **env 对账表**；Anthropic 原生协议（provider 特判族核心）与 Rust 统一 OpenAI 兼容架构冲突面最大且 anthropic 端点普遍有 OpenAI 兼容层——维持 95 号架构备案，不与本轮混作。

TS 语义勘测：`apiFormat: "chat" | "responses"` 是端点级配置（inkos.json llm.apiFormat / services[].apiFormat / 传输探测载荷）；`chatCompletionVia` responses 分支——`POST {baseUrl}/responses`，payload `{model, input, stream, store:false, max_output_tokens, temperature, ...extra, instructions?}`（system 进 instructions、非 system 进 input_text 数组）；非流式 `extractResponsesContent`；流式事件 `response.output_text.delta` / `response.completed|incomplete`（终态守卫：缺终态 → PartialResponseError）；`buildProbePlans`：preferred 有值 → [preferred×(stream??false)] +（stream 时）preferred×false，无偏好 → [chat×false, responses×false]（**stream 偏好仅在 apiFormat 有偏好时生效**）。

## 二、交付

### 1. `llm/streaming_client.rs`：responses 传输（TS 逐字语义）

- `ChatCompletionParams` 增 `api_format: TransportApiFormat`；`stream_chat` 分流（Chat → 原路径；Responses → `responses_completion`）。
- `build_responses_request`（input 构建 + instructions 合并 + extra 覆盖基础键 + store:false）；`extract_responses_content`（output[].content[] 的 text/content/output_text 拼接）；`sse_data_events`（原始 data 载荷提取——responses 事件为独立 JSON，不经 chat 形态解析器）。
- 非流式：JSON 解析 + 空响应 `LLM returned empty response`；流式：delta 累计 + 终态事件 usage（input/output/total_tokens）+ 空 content 兜底 + **缺终态 `stream closed without response.completed`**（PartialResponseError 语义）。
- `StreamError` 增 `Protocol(String)` 变体（协议层失败锚）。
- 单测 4：请求形态（input/instructions/无 images 注入）、extra 覆盖 + 无 system、多段 content 抽取、SSE 分块剥离。

### 2. `llm/agent_router.rs` + 配置接线

- `AgentRouter` 增 `api_format`（`with_api_format` builder，缺省 Chat——全部既有构造零破坏）+ `api_format()` 读取；RoutedAgent chat 与 RouterLoopChat 透传。
- **bin**：`INKOS_LLM_API_FORMAT` env → router 覆盖（env 对账本轮接线件）。
- **97 层**（`agent_production.rs`）：`AgentModelOverride` 增 `api_format`——层 1（显式服务，经 `resolve_configured_service_api_format` 新助手查服务项）/ 层 2（defaultModel+services[0] 的 entry apiFormat，TS selectedEntry 镜像语义）命中 responses 时置位；agent_route 覆盖路由 `.with_api_format(ov.api_format)`——**聊天/写作全代理面按服务配置走 responses**。

### 3. 探测计划（`service_routes.rs` + `ops_routes.rs`）

- `build_probe_plans(preferred_api_format, preferred_stream)` TS 逐字（含"无 apiFormat 偏好时忽略 stream 偏好"的 quirk）；单测 2。
- `minimal_chat_probe` 增 api_format 参数（同一最小探测载荷经所选传输发出）；test_service：计划维度 apiFormat×stream，`detected.apiFormat` 回显**计划真实值**（原回显 preferred）；preferred 缺省从 `"chat"` 改 `None`（TS undefined 语义——无偏好时 chat 失败回退 responses 探测）。
- doctor（99 号）：计划升级为 `build_probe_plans(llm.apiFormat, llm.stream)`（原仅 stream 维度）；两处 sub99 测试随契约补 `apiFormat:"chat"` 偏好（stream 偏好生效前提）。

### 4. E2E（`sub106_e2e`，2 测试）

- `test_service_falls_back_to_responses_probe_when_chat_unavailable`：/models 与 /chat/completions 皆 404 + 无 apiFormat 偏好 → chat 计划失败 → **responses 探测成功**（96 号"一律 chat"备案闭合的行为证据）：`ok:true` + `detected.apiFormat=responses` + 探测请求形态逐字（input_text "Reply with OK only." / store:false / max_output_tokens:16）。
- `agent_chat_uses_responses_transport_for_configured_service`：inkos.json 自定义服务（`custom:Resp`）`apiFormat=responses` + secrets key → 97 层 1 显式命中 → **studio-agent 聊天走 /responses**：200 + 响应文本来自 delta/completed 事件流 + 请求侧断言（input 数组末位 user 指令、system 全进 instructions 不进 input、store:false、stream:true）。

## 三、INKOS_* env 族对账表（TS 34 变量全量）

| 分类 | 变量 | 状态 |
|---|---|---|
| 已接 | INKOS_LLM_BASE_URL / API_KEY / MODEL / MAX_TOKENS、INKOS_AGENT_{\*}_MODEL/BASE_URL/API_KEY/MAX_TOKENS（bin 部署面，62 号） | ✓ |
| 已接 | INKOS_COVER_ENDPOINT / BASE_URL / MODEL / API_KEY / SIZE（71/78 号封面链） | ✓ |
| 已接 | INKOS_AGENT_ALLOW_SYSTEM_READ（105 号 read 系统读开关） | ✓ |
| **本轮接** | INKOS_LLM_API_FORMAT（bin 端点级 → router 传输协议） | ✓ |
| 不适用（studio/测试进程域） | INKOS_STUDIO_PORT、INKOS_CONFIG、INKOS_DEBUG_SQLITE_MEMORY、INKOS_AGENT_LLM_STUB、INKOS_TEST_COVER_KEY、INKOS_PLAN_*（plan 解析标记）、INKOS_LIVE_E* | — |
| 请求级 env 合并族（56 号备案维持） | INKOS_LLM_PROVIDER / SERVICE / STREAM / TEMPERATURE / THINKING_BUDGET / PROXY_URL / HEADERS / EXTRA_* —— TS resolveEffectiveLLMConfig 合并面；桌面单仓（配置全在 inkos.json）无行为差异 | 备案 |
| 未接（域内小件） | INKOS_DEFAULT_LANGUAGE（项目语言 env 覆盖）、INKOS_USER_AGENT（Rust 固定 "InkOS/1.3.5" 常量——TS env 可覆盖）、INKOS_SKILL_DIRS（60 号技能目录覆盖）、INKOS_FILM_IMAGE_SIZE（74 号 play 面 1024×1024 vs 1024×1536 备案） | 后续小件 |

## 四、parity 要点

- 传输 payload/事件解析/终态守卫/空响应错误逐字；探测计划序与去重逐字（含无偏好分支忽略 stream 的 quirk）；服务项 apiFormat 镜像（TS L2103 selectedEntry 语义）。

## 五、偏差备案

1. **responses 传输不携带 tools**：TS responses 分支 payload 无 tools 键（工具循环需 chat 协议）——代理面 responses 端点仅支持纯文本对话，与 TS 一致（非偏差，显式记录）。
2. **多模态 images 在 responses 传输不注入**（TS buildResponsesInput 仅 input_text）——95 号 vision 注入限 chat 协议，两侧一致。
3. **PartialResponseError 降级为错误文本**（`stream closed without response.completed`）：Rust 流式层无部分内容保留通道（collect-or-error 语义），TS 的 content 保留给上层重试不适用。
4. **thinking/reasoning 事件**（response.reasoning_summary_text.delta 等）不消费（TS 同——仅 output_text.delta）。
5. 请求级 env 合并族维持 56 号备案（本轮仅 bin 端点级 API_FORMAT）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1151** 过（+6：传输 4 + 计划 2） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **172** 过（+2：sub106；sub99 两处随 TS 逐字计划契约更新） |
| `cargo test --features export-bindings --lib` | **1310** 过（+6） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（107 号候选）

103 号审计开放清单 #3（responses 传输）闭合；#5（provider 特判族）维持 95 号架构备案（Anthropic 原生协议与统一 OpenAI 兼容层冲突，且 anthropic 端点普遍提供兼容层——切换后按真实流量需求再评估）。候选：

1. **首选：strangler 实切演练**（98 号 runbook 只读面起跑）——功能面含 responses 传输全链后已无已知协议缺口；需真实运行环境与流量条件（可请求用户提供：桌面端起 Rust engine + 指向真实 LLM 端点的 inkos.json）。
2. 其次：env 余量小件批（INKOS_DEFAULT_LANGUAGE / INKOS_USER_AGENT env 化 / INKOS_SKILL_DIRS / INKOS_FILM_IMAGE_SIZE——四件一轮）。
3. 或：聊天卡 details 外露 + SSE 结构化 tool:end（#9，需前端契约确认）。
