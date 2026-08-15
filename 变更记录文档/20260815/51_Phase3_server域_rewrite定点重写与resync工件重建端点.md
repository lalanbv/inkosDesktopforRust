# 51 号变更记录：Phase3 server 域——rewrite 定点重写与 resync 章节工件重建两端点

- 日期：2026-08-15
- 范围：engine-rs（扩展 `server/books_routes.rs`、`server/mod.rs`）
- 契约源：`packages/studio/src/api/server.ts` L5994-L6041（rewrite/resync 端点）、`packages/core/src/pipeline/runner.ts` L1342-L1672（reviseDraft rework 路径）、L2315-L2462（`_resyncChapterArtifactsLocked`）
- 验证：`cargo test --lib`（921）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（30）+ `cargo test --features export-bindings --lib`（1080）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

50 号后 books 域剩两个管线触点：`rewrite/:chapter`（人工定点重写——reviseDraft 的 rework 变体）与 `resync/:chapter`（章节工件重建——从已编辑正文重跑结算同步真相）。两端点分别复用 46/47 号已移植的 revise 审核环与 44 号的 settle→validate→重试链，本轮补端点装配与差异面。

## 二、交付内容

### 1. `POST /api/v1/books/:id/rewrite/:chapter`（server.ts L5994）

revise 审核环的 rework 变体，三点差异：

- **门控强制 Always**：`run_revise_chain` 新增 `gate: RevisionGate` 参数（revise 端点传 `runtime.revision_gate`，rewrite 传 `Always`）——修稿结果恒采纳，不因变差拒绝（E2E 用 post 审计 3 critical 变差 mock 断言 strict 会拒的场景下 applied=true）
- **brief 有键先落盘**：`hasOwnProperty("brief")` → `saveChapterUserBrief(brief ?? "")`（字符串 trim 落盘、null 同空串删文件、其他类型按 TS TypeError → 500）
- **SSE 三事件**：`rewrite:start`（键名 `chapter`）/ `rewrite:complete`（`chapterNumber/wordCount/status`）/ `rewrite:error`；响应 `{status: "complete", bookId, chapter, result}`，错误统一 500（无 revise 端点的 404 分支）

parseInt 语义对齐：NaN → `"Chapter NaN not found in index"`（链内 find 无命中路径）；负数/0 → `No chapters to revise for "{bookId}"`（reviseDraft 前置守卫）。

### 2. `POST /api/v1/books/:id/resync/:chapter`（server.ts L6025）

`run_resync_chain` 全量移植 `_resyncChapterArtifactsLocked`：

1. 前置校验（文案逐字）：空索引 → `Book "{id}" has no persisted chapters to sync.`；缺章 → `Chapter {n} not found in "{id}".`；非最新 → `Only the latest persisted chapter can be synced safely (latest is {latest}).`
2. 读正文（去标题行）+ 旧 current_state/pending_hooks
3. `settle_chapter_state`（复用 RepairSettle 端口）→ `StateValidator` 校验 → 失败走 `retry_settlement_after_validation_failure`（Degraded → `issues[0].description ?? "Chapter sync still failed for chapter {n}."`）
4. 落盘全量面：`save_chapter`（章节标题重构 + current_state/pending_hooks/chapter_summaries + state/*.json 原子集）+ `save_new_truth_files`（subplot/emotional_arcs/character_matrix 等）——与 repair-state 只写 state/hooks 两文件的窄面不同
5. `snapshot_state` + 索引回写：state-degraded → 基础状态 + 剥 reviewNote 注入问题；**非降级直接置 ready-for-review**（repair-state 的 degraded 前置在此不设限）
6. 返回 `ChapterPipelineResult` resync 形状：`{chapterNumber, title, wordCount（透传 meta 不重算）, auditResult{passed, issues: [], summary}, revised: false, status, lengthWarnings, lengthTelemetry, tokenUsage}`

语言解析：`book.language ?? genre.language`（save_chapter 的 numerical_system 同源 genre profile）。

### 3. E2E（books51_e2e，3 例）

- rewrite：Always 门变差放行（对照 47 号 strict 拒绝测试）+ brief 落盘 + SSE 三事件 + NaN 500
- resync 全链：形状断言 + 快照落盘 + 真相更新 + NaN/缺章文案
- resync 拒绝：非最新章 + 空索引文案

## 三、parity 要点

1. rewrite 与 revise 的 404 差异：revise 缺章 404（TS 有分支）、rewrite 统一 500（catch 无分支）
2. rewrite 的 `rewrite:start` 广播键名是 `chapter`（非 chapterNumber），`complete` 才是 `chapterNumber`——server.ts 原文如此
3. resync 的 wordCount 透传索引 meta（不按正文重算）——人工改字数后 resync 不刷新索引字数（TS 同款怪癖）
4. resync 非降级章直接 ready-for-review 且保留 auditIssues 原值

## 四、偏差备案

1. rewrite body.brief 非字符串非 null：TS 抛 TypeError 长文案；Rust → 500 `"brief must be a string"`（状态码一致）
2. resync body（externalContext）仅进 TS governed 输入（v2 治理面，47 号起暂缓），R侧忽略该字段——受治理面影响的审计/修稿差异沿 47 号暂缓口径
3. `syncLegacyStructuredStateFromMarkdown` / `syncNarrativeMemoryIndex` / `syncCurrentStateFactHistory` / `persistAuditDriftGuidance` 四个同步钩子沿 47 号暂缓（不阻塞主数据流）

## 五、暂缓件

- resync 的 governed artifacts 注入（createGovernedArtifacts 复用件——随 reviseControlInput v2 治理面统一移植）
- acquireBookLock 跨进程文件锁（沿既有策略）

## 六、下一步（52 号候选）

1. books/create + create-status 端点（书籍创建向导）
2. detect-all / detect/stats / detect/:chapter 检测域端点（analyzeAITells 纯函数已移植）
3. import/fanfic 域端点
4. sessions/state 配置域端点（Phase 2 清单）
5. Node sidecar 下线核对：books 域累计 34+2=36 端点已可切换

## 七、影响面

- Rust 业务端点累计：34（50 号后）+ 2 = **36 个**
- 新增测试：e2e +3（books51_e2e）；lib 无新增（链路复用已测件：revise 环 46/47 号、settle 链 44 号）
- 侵入修改：`run_revise_chain` 加 gate 参数（revise 端点行为不变——传 `runtime.revision_gate`，既有测试全绿佐证）
