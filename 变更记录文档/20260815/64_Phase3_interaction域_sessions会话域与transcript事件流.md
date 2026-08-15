# 64 号变更记录：sessions 会话域 8 端点与 transcript 事件流（Phase3 · interaction 域）

- 日期：2026-08-14
- 范围：engine-rs（`src/interaction/session.rs`、`session_transcript.rs`、
  `session_restore.rs`、`book_session_store.rs` 新建共 ~1900 行、
  `src/server/session_routes.rs` 新建 ~330 行、`src/server/mod.rs` 8 路由挂载、E2E）
- 契约源：`server.ts` L4536（interaction/session）+ L4703-L4803（sessions 七端点）+
  `core/src/interaction/`（session.ts / session-transcript*.ts / book-session-store.ts /
  project-session-store.ts）

## 背景

strangler 迁移 64 号（62 号清单次优先级缺口域）。sessions 域是 Studio 聊天会话的
持久层：transcript JSONL 事件流为真源（session_created/metadata_updated/
request_started/committed/failed/message 六事件），derive 重建 BookSession，
legacy `.json` 单文件自动迁移。`POST /agent`（agent loop 大件，依赖 runtime.ts
1153 行 + interaction-tools + SSE 流）随 65 号交互运行时专项移植。

## 交付

### 核心域（`src/interaction/`）

- **session.rs**：SessionKind（10 值）/ PlayMode 枚举 + BookSession（宽松 serde：
  messages 为 Value 列表）+ `create_book_session`（id 缺省 `毫秒-6位base36`）+
  `is_safe_book_id` + `to_response_json`（补 zod default：draftRounds/events 空数组）
- **session_transcript.rs**：六事件 [`TranscriptEvent`]（tagged serde，camelCase
  字段对齐 zod schema）+ `read_transcript_events`（坏行跳过、seq 排序、version≠1
  行拒绝）+ `append_transcript_events`（per-session tokio Mutex 串行队列：
  读→算 nextSeq→追加写，对齐 TS appendQueues）
- **session_restore.rs**（derive 子集，886 行 TS 的端点消费面）：
  `committed_message_events`（request_committed 过滤 + legacy 无 kind 放行）+
  `message_events_to_interaction_messages`（工具执行卡聚合：toolCall 配对
  toolResult、中文标签表逐字（建书/写作/审计…读取文件/搜索…）、play 工具卡独立、
  pending 工具挂前文、thinking 合并（`\n\n---\n\n` 连接）、use_skill 轮 thinking
  抑制、空收尾 assistant 折叠）+ `derive_book_session_from_transcript`（元数据折叠
  + updatedAt 活动时间取最大 + 首条 user 消息标题（20 UTF-16 码元截断 + …））+
  legacy 读取与迁移（session_created + request_started + 消息事件 + committed）
- **book_session_store.rs**：load（derive → legacy 迁移回退）/ list（.jsonl+.json
  双形态扫描 + `bookId !== bookId` **严格过滤**——null 只列未绑定会话）/
  rename（metadata 事件追加）/ delete（双删）/ create_and_persist（幂等 + kind/
  playMode 变更走 metadata）+ `load_project_session`（.inkos/session.json 宽松 +
  zod default 填充）+ `resolve_session_active_book`（在册优先 / 唯一书回退）

### 8 端点（`src/server/session_routes.rs`）

| 端点 | 契约要点 |
|---|---|
| GET /interaction/session | `{session, activeBookId}`；activeBookId 以解析值覆盖（含缺失态） |
| GET /sessions?bookId= | 摘要列表（updatedAt 降序）；bookId=null 严格过滤 |
| GET /sessions/:sessionId | 404 平铺 `{error:"Session not found"}`；task 对账快照附加 |
| POST /sessions | normalizeSessionKind（bookId→book 回退 chat）/ safeSessionId（`^[0-9]+-[a-z0-9]+$` 防注入）/ 幂等 + 删除标记复活 |
| PUT /sessions/:sessionId/play-mode | 400 INVALID_PLAY_MODE / 404 / 幂等更新 |
| PUT /sessions/:sessionId | title trim 必填（400 ApiError）/ metadata 事件改名 |
| DELETE /sessions/:sessionId | 删除标记 + transcript/task 双删 |
| POST /sessions/:sessionId/abort | scope chat/all；无运行执行体 → `{ok, aborted:false}` + agent:aborted 广播 |

`load_reconciled_task_snapshot`：running/processing 快照且本进程无运行确认 →
改写 error 终态并落盘（对齐 TS"旧进程遗留任务卡"对账语义）。

## parity 要点

- 事件字段 camelCase 逐字（sessionId/requestId/parentUuid/toolCallId/legacyDisplay）
- 标题截断按 UTF-16 码元（TS `slice(0,20)`）
- `deletedSessionIds` 进程内标记集（DELETE 先标记防中止后错误持久化重建快照）
- 无过滤列表的 `bookId !== null` 严格语义（已绑定书的会话不出现）——E2E 固化
- normalizeApiBookId 四段校验（非串/空白/不安全 → 400 INVALID_BOOK_ID）

## 偏差备案

- **POST /agent 整体暂缓**（64 号指令 9 端点中的第 9 个）：agent loop 深链
  （runtime.ts 1153 行 + processProjectInteractionRequest + interaction-tools +
  edit-controller 其余 kind + SSE 流式 + abort 控制器注册表）是独立大件，
  65 号专项移植——本轮交付其全部持久层依赖（transcript/derive/store），
  会话端点面先行切换
- **restore 的 agent 重放面暂缓**：restoreAgentMessagesFromTranscript /
  cleanRestoredAgentMessages / adaptRestoredAgentMessagesForModel（给 LLM 历史回放
  用的消息清理与跨模型适配）——仅 agent 端点消费，随 65 号
- **abort 恒 false**：进程内无 agent 执行体生产者（write-next 等不注册会话任务）；
  控制器注册表随 65 号接入后 abort 生效
- **对账错误文案**：中文版固定（TS 按项目语言 pick；64 号场景项目语言读取面已有，
  该分支文案以 zh 固化——E2E 断言中文）

## 暂缓件

- POST /agent（65 号：runtime.ts + 工具面 + SSE + abort 注册表 + B12 系列校验）
- 会话消息的 manual 追加（appendManualSessionMessages——agent 端点持久化用）

## 下一步（65 号候选）

- POST /agent 交互运行时大件（本轮交付的持久层直接复用）
- foundation/revise + resumeFrom + 同步钩子（books 域补全）
- interactive-films/projects 域（10 条）

## 影响面

- Rust 业务端点 97 → 105 个；e2e 77 → 84；lib/export-bindings/TS 基线不变
- 62 号核对清单缺口 37 → 29 条（sessions 8 条全清；agent 1 条仍缺）
- 全量基线：`cargo test --lib`（970）/ `golden_leaf`（76）/
  `e2e_write_next_contract`（84）/ `--features export-bindings --lib`（1129）/
  `clippy --lib --tests --bins`（零警告）/ TS vitest（185 文件 1798 测试）
