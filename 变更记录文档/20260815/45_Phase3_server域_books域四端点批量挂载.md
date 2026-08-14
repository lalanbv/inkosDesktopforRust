# 45 号｜Phase 3 server 域：books 域四端点批量挂载（/plan、/settle、/draft、/revise）

> 里程碑：books 域同步端点批量上线——复用 32–36 号已移植的编排本体
> （planner/settle/write-next manual/reviser）挂 4 个端点，Rust 侧业务端点
> 达 **8 个**（SSE + write-next + audit + 本号四个）。`router_books` 组合
> 路由 + bin 装配 + 每端点 E2E 契约。

## 契约勘测（Node server.ts 逐端点核对）

| 端点 | Node 契约 | Rust 实现 |
|---|---|---|
| `POST /books/:id/plan`（L3579） | **同步**；body `{context?}` → `PlanChapterResult {bookId, chapterNumber, intentPath, goal, conflicts:[]}` | planner::plan_chapter + plan.md 持久化（对齐 runner resolveGovernedPlan——E2E 抓出首版漏持久化） |
| `POST /books/:id/settle` | **Node 无此端点**（Node 走 /repair-state） | Rust 侧补齐：body `{chapter, title, content, allowReapply?}` → settle_chapter_state 同步结算，返回落盘摘要 |
| `POST /books/:id/draft`（L3532） | **fire-and-forget**；`{status:"drafting", bookId}` + SSE draft:start/complete/error | write-next **manual 模式**（写完即停 = draft 语义，备案） |
| `POST /books/:id/revise/:chapter`（L5604） | **同步**；body `{mode?, brief?}`（默认 spot-fix）；章节缺失 404；SSE revise:start/complete/error | reviser::revise_chapter + NNNN*.md 首匹配（404 文案对齐） |

## 改动

### 新建 `src/server/books_routes.rs`（~640 行，含测试）

- 四端点 handler + `BooksRuntime`（AuditRuntime 同款装配面：
  hub/state/router/builtin_genres_dir）
- `/plan`：控制文档 → book 配置 → 章号 → planner → **save_persisted_plan**
  → PlanChapterResult（intentPath 为 bookDir 相对 POSIX 路径）
- `/settle`：loadBookConfig → settle_chapter_state（writer 端口 + 泄漏
  static ctx）→ `{chapterNumber, title, wordCount, updatedState, updatedHooks}`
- `/draft`：spawn + write_next（Manual 模式 + externalContext=brief）→
  SSE 三事件（Node 契约）
- `/revise`：章节文件匹配 → mode 映射（默认 SpotFix）→ revise_chapter →
  ReviseOutput JSON（camelCase）
- serde：`PlanChapterOutput`/`ReviseOutput` 补 Serialize

### `router_books` + bin

- 组合路由（router_full 之上叠四端点，BooksRuntime clone 共享）
- bin 装配：BooksRuntime 复用 audit 的 router/hub/state/builtin

### 与 Node 的已备案差异

- `/revise`：Node reviseDraft 内部**先审后修**（audit→revise 环）；
  Rust 首版直接以 brief 驱动修稿（空问题清单）——审核环接入待后续
- `/draft`：Node writeDraft 是独立管线变体；Rust 映射为 manual 模式
  （写完即停 + 结算照走）
- `/settle`：Node 侧不存在——Rust 补齐（对齐 32 号已移植能力面）

## 验证

- **lib 单测**：877 passed / 0 failed（+5：plan 不可达 500 / draft 即返 +
  SSE error / settle 缺书 500 / revise 缺章 500（Node 文案）/ 坏章号 400）
- **E2E 契约**：5 passed——新增**plan 主链**（mock planner → 200
  PlanChapterResult 形状 + plan.md 落盘——差分抓出漏持久化后补齐）+
  **revise 主链**（mock reviser TAG 输出 → 200 ReviseOutput camelCase +
  SSE start/complete）
- **golden 差分**：76 域全绿
- **export-bindings**：1036 passed
- **TS 全量**：185 文件 / 1798 测试全绿
- **clippy**：`cargo clippy --lib --tests --bins` 零警告

## 下一步

- ⬜ **46 号**：`/revise` 补审核环（audit→revise 全链对齐 Node reviseDraft）；
  `/compose`、`/consolidate`、`/repair-state/:chapter` 同域端点；
  sessions/state 配置类端点（Phase 2 域）；Node sidecar books 域按端点
  开始下线核对（端点契约 diff 清单）。
