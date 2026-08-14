# 44 号｜Phase 3 agents 域：完整审计编排接入主链与 audit 端点

> 里程碑：write-next 审核环的 auditor 从 43 号最小 PASS/FAIL 升级为**真实
> `audit_chapter` 编排**（11 路真相文件 + 维度审计 + 四策略解析——早前已
> 移植于 continuity.rs 的 2339 行主体，本号接通端口）；`/api/v1/books/:id/
> audit/:chapter` 端点挂载（**同步契约**，区别于 write-next 的
> fire-and-forget）。

## 背景

勘测发现 continuity.ts 的完整编排（842 行 TS → 2339 行 Rust，含
`audit_chapter` 主体）早已移植——43 号的"最小协议"只是端口层临时物。
本号工作：①把真实编排接进 write-next 环（FullCycleAuditor 适配）；②挂
audit 端点（Node 契约：读章节 404 → 审计 → 200 AuditResult JSON + SSE
`audit:start/complete/error`）；③bin 装配。

## 改动

### `src/llm/agent_router.rs`

- `AuditorChat for RoutedAgent`（真实编排的 chat 端口；chat_with_search
  默认回落——对齐 TS 容错路径）
- **`FullCycleAuditor`**：完整审计编排的 CycleAuditor 适配——构造时持
  router + 项目路径面；`for_chapter(book_dir, chapter, genre)` 按章克隆
  （write-next 环内以正确 book 上下文重建主链）；control 入参（intent/
  memo/package/ruleStack）逐调用透传为 `AuditChapterOptions`
- 原 `RoutedAgent` 的最小 PASS/FAIL CycleAuditor 保留为轻量回退（单测/
  健康探测），文档标注主位已移交

### `src/pipeline/write_next.rs`

- `WriteNextAgents` 增 `full_auditor: Option<FullCycleAuditor>`——auto 环
  内 `for_chapter` 重建后作主链，None 回退 `agents.auditor`（43 号协议/
  测试 mock 路径），全部既有测试零改动通过

### 新建 `src/server/audit_route.rs`（端点）

- `POST /api/v1/books/:id/audit/:chapter`：章节号解析（坏参 400）→
  `NNNN*.md` 首匹配（缺失走 500 `"Chapter not found"`——Node 同文案）→
  真实 `audit_chapter` → 200 AuditResult JSON；SSE `audit:start` →
  `audit:complete {bookId, chapter, passed}` / `audit:error`
- `AuditRuntime { hub, state, router, builtin_genres_dir }`；
  `router_full` 组合路由（utility + SSE + write-next + audit）

### `src/bin/inkos-engine-server.rs`

- 装配 `router_full`：audit 端点接入（AuditRuntime 复用 hub/state/router）

### serde 面（端点 JSON 契约）

- `AuditResult`/`AuditIssue`/`AuditTokenUsage`/`RepairScope` 补 Serialize
  （camelCase 对齐 TS 响应形状）；`AuditTokenUsage` 升 Copy（连锁 clone
  清理：writer/review_cycle/continuity 四处）

## 验证

- **lib 单测**：872 passed / 0 failed（+2：审计端点 404 路径 + 不可达
  LLM → 500 + audit:error SSE）
- **E2E 契约**：3 passed——新增**审计链路 E2E**（mock LLM 返回维度审计
  JSON → 完整 audit_chapter 编排 → 200 `{passed, overallScore, summary,
  issues[]}` + SSE start/complete{passed:true, chapter:1}）
- **golden 差分**：76 域全绿
- **export-bindings**：1031 passed
- **TS 全量**：185 文件 / 1798 测试全绿
- **clippy**：`cargo clippy --lib --tests --bins` 零警告

## parity/工程要点

1. **同步 vs fire-and-forget 契约差异**：audit 端点是 Node 同步流（返回
   AuditResult），write-next 是即返 + SSE——strangler 端点迁移必须逐端点
   核对契约形态，不可一刀切
2. **FullCycleAuditor 的构造权分层**：路径面（project/builtin）构造时持；
   book 上下文（book_dir/章节/genre）`for_chapter` 重建——write-next 环内
   这些是调用级常量而 CycleAuditor 接口不携带
3. **fixture 语义**：write-next E2E 的 fixture 不写章节文件（管线生成）；
   审计 E2E 需自带——复用 fixture 时踩 404，审计测试补写后通过
4. **Option 端口而非条件分支**：`full_auditor: Option<...>` 让 43 号全部
   测试（mock auditor）零改动存活——主链升级不破坏既有验证面

## 下一步

- ⬜ **45 号**：同域端点批量挂载——`/plan`（复用 34 号 planner + 35 号
  plan 持久化）、`/settle`（32 号 settle_chapter_state）、`/draft`、
  `/revise/:chapter`（36 号）——均为同步契约 + AuditRuntime 同款装配面；
  Node sidecar 的 books 域开始按端点下线。
