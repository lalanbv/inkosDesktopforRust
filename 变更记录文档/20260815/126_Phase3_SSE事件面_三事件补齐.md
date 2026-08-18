# 126 号变更记录：SSE 事件面勘测与三项 UI 订阅事件补齐（session:title / llm:progress / context:compression）

## 一、勘测（勘测方法本身是本轮交付物）

事件词汇表对照：TS sidecar `broadcast(...)` 66 事件 vs Rust `hub.broadcast`（**multiline 提取**——单行 grep 会漏跨行调用，曾致首轮假差异 29 个）→ Rust 已覆盖 59。缺口 7 个，按 UI 可观测性分级：

| 事件 | UI 是否订阅（use-sse.ts STUDIO_SSE_EVENTS） | 处置 |
|---|---|---|
| `session:title` | ✅（侧栏标题实时更新） | **本号补齐** |
| `llm:progress` | ✅（运行卡进度心跳） | **本号补齐** |
| `log` | ✅（日志面板） | 备案（Rust 无 logger 基建——下一候选） |
| `context:compression` | 面板外（任务卡附加） | **本号补齐**（composer 回调已存在但未接线） |
| `thinking:start/delta/end` | ❌ 未订阅（parts-builder 走 agent 响应流另道） | 备案 |

## 二、交付

### 1. `session:title`（agent_route）

- 装载点快照 `title_before_run`（TS `titleBeforeRun` 语义）；`maybe_broadcast_session_title`：run 前为空、run 后（首条 user 消息落盘）derive 出标题 → `{sessionId, title}` 广播一次。
- 四个终态调用点：确认式生产 Ok/Err + 循环聊天 Ok/Err——统一**终态事件后补发**（不扰动既有 E2E 事件序列断言）。

### 2. `llm:progress`（全链贯通）

- `provider::StreamProgressCallback` 类型；`ChatCompletionParams.progress`（唯一流式收口）；`StreamMonitor`——TS `createStreamMonitor` 对应：30s 节流 streaming + 流成功终态 done；中文区 U+4E00..U+9FFF 逐字。
- `AgentRouter.progress_hook` + `with_progress_hook`（builder 字段语义，with_api_format/with_stream 覆盖不丢）+ getter（挂载态断言）。
- `BooksRuntime::effective_router` 构造时挂 hub 广播闭包（**不带 sessionId**——TS books 面 pipeline 无 sessionIdForSSE）；agent 聊天轮 `RouterLoopChat` 挂 **带 sessionId** 闭包（TS 聊天轮语义）。

### 3. `context:compression`（接线既有回调）

- `WriteNextConfig.on_context_compression`（composer `CompressionCallback` 既有类型）；write-next 链 prepare 与 compose 端点接 hub 广播（camelCase 负载、可选键省略）。
- 共享装配 `books_routes::write_next_config_with_events`（from_project + 广播回调）——run_draft 与 bin runner 同源。

### 4. 已知微差（备案，非观测级）

- 静默期不心跳（chunk 驱动节流 vs TS setInterval——生成中 chunk 持续到达，仅停滞期少报）；错误路径不发 done（TS finally 发）；totalChars 为 Unicode 标量数（TS UTF-16 码元——BMP 一致）。

## 三、测试（+5：lib 1167 / E2E 186）

- `stream_monitor_counts_chars_and_emits_done`（lib，纯计数+终态）。
- `chat_broadcasts_llm_progress_and_session_title`（E2E：真实事件序 agent:start → {llm:progress(done, sessionId+camelCase), draft:delta} → agent:complete → session:title——llm:progress/draft:delta 相对序不锁定）。
- `agent_router_progress_hook_fires_done_on_stream`（E2E：router 钩子经真实流式 mock 收 done+计数）。
- `effective_router_attaches_llm_progress_broadcast`（E2E：inkos.json 服务项热解析路径挂钩、payload 无 sessionId）。
- `write_next_config_broadcasts_context_compression`（E2E：回调广播 camelCase/可选键省略）。

## 四、验证基线（126 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1167** 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **186** 过（+4） |
| `cargo test --features export-bindings --lib` | **1326** 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | 7/7 |

## 五、下一步候选

1. `log` 事件（Rust logger 基建 + LogSink → SSE 管道）。
2. thinking:* 三事件（若 agent 响应流面勘测确认 UI 消费路径）。
3. 实切条件到位后真实流量切换（duel 7/7 前置门）。
