# 42 号｜Phase 3 pipeline 域：write-next 主体装配与路由挂载（write-next 链收官）

> 里程碑：`_writeNextChapterLocked` 编排本体 + SSE 广播面 +
> `/api/v1/books/:id/write-next` 路由挂载——**strangler 首个核心业务端点上线**。
> 全链：控制文档 → book 配置 → 降级守卫 → 章号 → 输入准备（plan 持久化复用 +
> composer）→ writeChapter → manual 写完即停 / review-cycle → promotion →
> 标题去重双轮 → buildPersistenceOutput → 长跨度疲劳 → truth-validation →
> 段落形态 → persistChapterArtifacts → 通知回调。

## 改动

### `src/utils/long_span_fatigue.rs`：+`analyze_long_span_fatigue`（32 号预留件补齐）

- 三压力（节奏/情绪/标题——`analyzeChapterCadence` 消费，仅 high 报警）+
  首尾句式同构（近 2 章正文 + 当前，三体两两相邻 Dice ≥ 0.72 才报）
- `chapterSummary` 同章节号行替换合并；`LongSpanFatigueIssue` → `AuditIssue`
- 全链集成测试：6 章连续"紧张/推进章"+ 3 章同构首尾 → 四类问题全命中

### 新建 `src/pipeline/write_next.rs`（~1500 行，含测试）

**类型层**：`ChapterPipelineResult` / `ChapterReviewMode` / `InputGovernanceMode` /
`WriteNextConfig` / `WriteNextAgents`（九路 LLM 端口聚合：writer/planner/
composer/reviser/auditor/normalizer/analyzer/state-validator/settler）/
`WriteNextCtx`（路径 + 双 store + 预算 + 通知回调）/ `WriteNextError`

**主链 `write_next_chapter(_locked)`**
- 进程内 per-book tokio 互斥（TS 文件锁的备案等价物）
- prepareWriteInput：v2 = plan 持久化复用（无新上下文跳过 planner LLM）+
  composeGovernedChapter；legacy 仅 externalContext
- writeChapter → manual 写完即停（表面净化 + 空内容守卫 + "尚未审查"审计）/ auto
  审核环（37 号 review-cycle：AI 味 + 敏感词 + post-write 每轮重跑 + hook 账本
  校验注入）
- promotion pass（落盘前，零 LLM）→ 标题去重双轮（去重名 warning 注入审计）→
  buildPersistenceOutput（40 号）→ 长跨度疲劳 + hook 健康并入 → 长度警告/
  遥测 → truth-validation（38 号）→ 段落形态（近 5 章对照）→
  persistChapterArtifacts（37 号：saveChapter / saveTruthFiles + 结构化状态
  bootstrap / 索引 upsert / markBookActive / 漂移指引 / 快照）
- 降级守卫：最新章节 state-degraded → 拒绝续写（报错带章号）
- `persist_audit_drift_guidance`：audit_drift.md 写入 + current_state 旧纠偏块
  剥离（四标题锚点取最早出现）

### 新建 `src/server/sse.rs`（SSE 广播面）

- `BroadcastHub`（tokio broadcast 通道；无订阅者静默）
- `/api/v1/events`：连接即 ping → `?sessionId&projectRoot` 补发 `task:snapshot`
  → 广播转发 → 30s keep-alive（hono streamSSE → axum Sse 契约对齐）
- 慢消费者 Lagged → ping 保活继续

### 新建 `src/server/write_next_route.rs`（端点挂载）

- `POST /api/v1/books/:id/write-next`：fire-and-forget——立即返回
  `{status:"writing", bookId}`；完成推 `write:complete {bookId, chapterNumber,
  status, title, wordCount}`；失败推 `write:error {bookId, error}`
- `WriteNextRuntime { hub, state, runner, project_root }`：runner 闭包构造九路
  端口聚合并在其生命周期内跑完（LLM 域路由器的生产接线点；测试注入 mock）
- 任务快照（39 号 task-store）：body 带 sessionId 时按会话落盘完成/错误态
- `router_with_runtime`：utility + SSE + write-next 组合路由

## parity 要点（移植难点）

1. **manual 模式解析状态 = audit-failed**：`passed:false`（尚未审查）→
   `chapterStatus ?? (passed ? ready : failed)` 落 audit-failed（TS 同语义，
   集成测试固化——非 bug 是契约）
2. **摘要真相面双形态**：settler delta 存在时走 state/*.json（结构化投影），
   无 delta 走 markdown 重写——断言按"任一面存在"验证
3. **SSE 帧格式**：axum 输出 `event: <name>`（冒号后空格）——与 hono
   `writeSSE` 一致，测试首版按无空格断言失败后修正
4. **hook 账本校验的 severity 映射**：`ViolationSeverity::Critical`（ledger
   域）→ `AuditSeverity::Critical`；post-write 的 `Error` → `Critical`
5. **死存储收敛**：TS 的 `finalWordCount` 在 persistence 覆写前的两次赋值
   是死存储——Rust 收敛为单点 `let`（行为等价，编译器验证）
6. **infinite SSE 流的测试方法**：不做 `to_bytes` 的 EOF 等待——有界帧消费
   （deadline + 帧数上限）

## 与 TS 的已备案差异

- **互斥**：进程内 per-book tokio 锁（41 号备案）
- **`syncCurrentStateFactHistory`**：SQLite 记忆索引重建——markdown 回退面下
  空操作（随记忆索引端点移植）
- **webhook**：`emitWebhook` 通知回调注入面已备（notify），webhook 通道随
  notify 域端点接线

## 验证

- **lib 单测**：867 passed / 0 failed（+8 新单测：长跨度疲劳三态[空书/四类
  问题全命中/三体门槛]、write-next 全链集成[manual 模式：plan 持久化 + 草稿 +
  结算 + 索引 + 快照 + durable 推进]、降级守卫、漂移指引往返 + 空清单清除 +
  旧块剥离、长度警告区间、SSE 帧承载 + 无订阅静默、write-next 路由
  [writing 即返 + start/error SSE])
- **golden 差分**：76 域全绿
- **export-bindings**：1026 passed
- **TS 全量**：185 文件 / 1798 测试全绿
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **43 号**：write-next 的生产 LLM 接线——`agents_factory`/runner 的真实
  实现（llm 域 registry/streaming-client 之上的九路端口适配 + 模型路由配置）
  + Tauri/独立 bin 的 `router_with_runtime` 装配 + E2E 契约测试
  （对 Node sidecar 同场景响应 diff）。
- 后续：audit→revise 环的 auditor 真实端口（continuity.ts 编排本体）；
  `/api/v1/books/:id/plan`、`/settle` 等同域端点批量挂载。
