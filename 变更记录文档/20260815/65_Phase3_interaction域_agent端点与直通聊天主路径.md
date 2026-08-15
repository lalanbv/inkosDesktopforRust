# 65 号变更记录：POST /agent 交互 agent 端点——校验装配层与直通聊天主路径（Phase3 · interaction 域）

- 日期：2026-08-14
- 范围：engine-rs（`src/server/agent_route.rs` 新建 ~320 行、`src/server/mod.rs` 路由挂载、
  `src/server/session_routes.rs` 两个 helper crate 化、E2E）
- 契约源：`server.ts` L4805-L5400（POST /agent 执行体）

## 背景

strangler 迁移 65 号。POST /agent 是 Studio 聊天入口，完整执行体深链包括：校验链、
会话装配、模型四层解析、外部编辑捷径、确认式生产任务分支（task 快照 + 建书迁移）、
聊天分支（runAgentSession 的 pi-agent loop + 20+ 工具面 + SSE 流式增量）。
64 号已交付全部持久层（transcript/derive/store），本轮在其上接通端点。

## 交付

### `src/server/agent_route.rs`

**完整校验链与装配层**（逐字对齐）：
- 无 instruction → 平铺 400 `{error:"No instruction provided"}`；
  非 JSON body 同错（json 解析失败 → instruction 缺省）
- 无 sessionId → ApiError 400 `SESSION_ID_REQUIRED`
- 会话不存在 → 404 `SESSION_NOT_FOUND`（消息带 sessionId）
- activeBookId 与会话绑定不一致 → 409 `SESSION_BOOK_MISMATCH`
  （消息 `Session {id} is bound to {persisted}, not {requested}`）
- sessionKind/playMode 变更 → 幂等更新（复用 64 号 create_and_persist）
- 绑定书配置缺失 → 404 `BOOK_NOT_FOUND`
- `agent:start` 广播（instruction/activeBookId/sessionId/actionSource/attachments 计数）

**直通聊天主路径**（无工具问答——Studio 聊天最常见形态）：
- 简化系统提示（语言跟随 + 活动书籍上下文注入）→ `AgentRouter::chat`（复用
  既有 StreamingChatClient 真网络面）→ responseText
- transcript 持久化：`append_chat_turn`（request_started → user → assistant →
  request_committed 四事件，对齐 appendManualSessionMessages 语义）——64 号暂缓件清账
- `agent:complete` / `agent:error` 广播；响应 `{response, session:{sessionId,
  sessionKind, activeBookId?}}`
- LLM 失败 → 500 `AGENT_SESSION_FAILED` + response 文案（agent:error 形态）

**abort 注册表**：`running_agent_sessions()`（sessionId → AbortFlag）——64 号
abort 端点的消费面挂接；直通聊天不支持中途截断（标记供 66 号工具面轮询）。

## parity 要点

- 404/409/400 三种错误形态逐字（SESSION_NOT_FOUND/SESSION_BOOK_MISMATCH/
  BOOK_NOT_FOUND/SESSION_ID_REQUIRED 均为 ApiError 结构；No instruction 平铺）
- 聊天轮四事件 transcript（E2E 验证会话详情可 derive 出 user/assistant 消息与
  首条 user 消息标题）
- 会话 kind 变更幂等（book 会话走 `SessionKind::Book` 回退链）

## 偏差备案

- **工具调用面暂缓（66 号专项）**：runAgentSession 的 pi-agent loop（runtime.ts
  1153 行）、20+ 工具（read/edit/grep/ls/propose_action/sub_agent/play_*/short_
  fiction_run 等）、SSE 流式增量（draft:delta/thinking:delta/tool:start|end
  ——本轮 agent:complete 前无增量事件）、确认式生产任务分支（reservedProductionSessions
  闸门 + executeConfirmedProductionAction + task 快照 + 建书迁移 + book:creating/
  created/error 事件）、validateAgentActionExecution
- **模型四层解析暂缓**：reqService+reqModel 显式选择 / defaultModel+firstService /
  secrets 首个连接服务 三层（本轮用 BooksRuntime.router 的既有端点——
  E2E 以 mock LLM 验证主路径；63 号 resolve_effective_llm_studio 已具备数据面）
- **attachments 归一化暂缓**（上传落盘 + MAX_AGENT_ATTACHMENTS 校验链）随工具面
- **surfaceLanguage 推断暂缓**：书语言优先链已有数据，本轮系统提示固定中文开头

## 暂缓件（66 号：交互运行时工具面专项）

- pi-agent loop（多轮 tool-use、SSE 增量、abort 中途截断）
- interaction-tools 全量（agent-tools + project-tools + edit-controller 其余 kind）
- 确认式生产任务分支（write_next/short_run/create_book/play_start intent 执行器）
- restoreAgentMessages 历史回放（restore.ts 后半，64 号备案）

## 下一步（66 号候选）

- 交互运行时工具面（上述暂缓件）
- foundation/revise + resumeFrom + 同步钩子（books 域补全）
- interactive-films/projects 域（10 条）、translations（6 条）

## 影响面

- Rust 业务端点 105 → 106 个；e2e 84 → 88；lib/export-bindings/TS 基线不变
- 62 号核对清单缺口 29 → 28 条（POST /agent 端点面已挂载；工具面深度仍列暂缓）
- 全量基线：`cargo test --lib`（970）/ `golden_leaf`（76）/
  `e2e_write_next_contract`（88）/ `--features export-bindings --lib`（1129）/
  `clippy --lib --tests --bins`（零警告）/ TS vitest（185 文件 1798 测试）
