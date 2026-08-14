# 43 号｜Phase 3 llm 域：write-next 生产接线与 E2E 契约（strangler 端点可用）

> 里程碑：write-next 从"可测试"进入"可运行"——`AgentRouter` 生产 LLM 接线
> （默认端点 + per-agent 覆盖 + 客户端缓存）+ 九路端口真实适配 + 独立 bin
> （`inkos-engine-server`）+ **真实 HTTP 栈的 E2E 契约测试**（mock LLM 服务
> 驱动全链：路由 → AgentRouter → StreamingChatClient → SSE 收集 → 全管线 →
> 落盘 → SSE 事件 → task-store 快照）。

## 改动

### 新建 `src/llm/agent_router.rs`（~370 行，含测试）

- `AgentRouter`：默认端点（`LlmEndpointConfig`）+ per-agent 覆盖表
  （`AgentOverride`：model / baseUrl+apiKey 独立端点 / maxTokens——TS
  `resolveOverride` 的 Rust 子集）；客户端按端点缓存
- `chat(agent, messages, temperature, max_tokens)`：路由 + 流式收集 +
  usage 映射（total 缺省 0 求和）
- `RoutedAgent`：宏生成七路同签名 trait 实现（WriterChat/PlannerChat/
  ReviserChat/LengthNormalizerChat/ChapterAnalyzerChat/StateValidatorChat +
  ComposerChat 带 options）；**CycleAuditor** 最小审计协议（首行 PASS/FAIL +
  `[分类] 描述` 行 + 分数行——完整 continuity 编排随 audit 域端点补齐，备案）；
  **RoutedSettler**（writer.settleChapterState 的链内包装）
- `SettleRequest`/`SettlementRetryParams`/`TruthValidationParams` 补 book/
  book_dir 字段（settle 需书面上下文——结构修正而非适配层兜底）

### 新建 `src/bin/inkos-engine-server.rs`（独立 HTTP bin）

- `router_with_runtime` 装配：utility + SSE + write-next
- 配置面（env，对齐 Node sidecar 约定）：`INKOS_PROJECT_ROOT` /
  `INKOS_LLM_BASE_URL|API_KEY|MODEL|MAX_TOKENS` / `INKOS_PORT`（默认 8787）；
  per-agent 覆盖 `INKOS_AGENT_<NAME>_{MODEL,BASE_URL,API_KEY,MAX_TOKENS}`
- runner 闭包：九路端口经 OnceLock/leak 单例构造（进程生命周期）+
  write-next 执行

### **Send 修复**（`memory_retrieval.rs`，真实阻塞 bug）

rusqlite `Connection` 非 Sync——`retrieve_memory_selection` 的 DB 分支在
持有 `&MemoryDb` 时 `await` markdown 读取，导致 `write_next_chapter` future
不满足 `Send`（lib 测试单线程 runtime 掩盖，bin 的 `tokio::spawn` 暴露）。
修复：markdown 预读提前到 DB 打开之前，DB 分支收敛为**纯同步段**
（`assemble_db_selection` 去异步化）。这是"多线程运行时才能真正验证 Send
纪律"的实证。

### 新建 `tests/e2e_write_next_contract.rs`（E2E 契约测试）

- **mock LLM 服务**：本地 axum `/chat/completions`，按 system prompt 首行
  分发 planner/writer/审计脚本响应（OpenAI SSE chunk 形态，含 usage 事件）
- **契约断言**（对齐 Node server.ts L3512-3530）：
  `POST /books/:id/write-next` → 200 `{status:"writing",bookId}`；
  SSE 序列 `write:start` → `write:complete {bookId,chapterNumber,status,
  title,wordCount}`；task-store 快照落盘（version=1 / requestedIntent=
  write_next）；mock LLM 确实收到 planner 调用
- **失败路径**：不可达端点（discard 端口 9）→ `write:error` 事件
- 全链真实面：HTTP 路由 → AgentRouter（含 planner 的 plan-model 覆盖路由）→
  StreamingChatClient（reqwest + sse_parser）→ write-next 全管线 → 落盘 →
  事件 → 快照

## parity/工程要点

1. **`sessionId` camelCase**：路由 body 字段首版漏 `#[serde(rename)]`——
   E2E 断言"快照落盘"失败，debug 打印发现 `session=None`。教训再证：
   **E2E 是 serde 契约的最后防线**（单测直构结构体测不到反序列化）
2. **Send 纪律**：`!Sync` 资源（DB 连接）不得跨 `await`——bin 的
   多线程 spawn 是比 lib 测试更强的验证面
3. **审计端口的最小协议**：完整 continuity.ts 编排是独立大件，43 号以
   PASS/FAIL+分数协议先行接通 write-next 环（备案，随 audit 域端点补齐）
4. **runner 闭包的生命周期收敛**：九路端口借用 → BoxFuture 内构造，
   `Box::leak` 单例化进程级资源（bin）；测试内 per-call 构造
5. **快照写入在 complete 广播之后**：E2E 轮询等待（事件到达 ≠ 快照落盘）

## 验证

- **lib 单测**：870 passed / 0 failed（+3：路由解析/不可达错误字符串化/
  审计协议解析）
- **E2E 契约**：2 passed（成功路径全链 + 不可达失败路径）
- **golden 差分**：76 域全绿
- **export-bindings**：1029 passed
- **TS 全量**：185 文件 / 1798 测试全绿
- **clippy**：`cargo clippy --lib --tests --bins` 零警告
- **bin 编译**：`cargo check --bin inkos-engine-server` 通过

## 下一步

- ⬜ **44 号**：audit→revise 环的完整审计编排（continuity.ts 主体移植——
  替换 CycleAuditor 最小协议为真实维度审计）+ `/api/v1/books/:id/audit/:chapter`
  端点。
- 后续：`/plan`、`/settle`、`/draft` 同域端点批量挂载（复用 42/43 号装配面）；
  Node sidecar 同场景双跑 diff（生产环境验收步，需真实 LLM 配置）。
