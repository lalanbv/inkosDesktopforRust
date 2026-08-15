# 49 号变更记录：Phase3 interaction/state 域——编辑事务域（chapter-replace + 章节版本化 + PUT/DELETE chapters 端点组）

- 日期：2026-08-15
- 范围：engine-rs（新增 `interaction/edit_controller.rs`、`state/chapter_delete.rs`；扩展 `server/books_state_routes.rs`、`server/mod.rs`、`state/chapter_workspace.rs`、`utils/utc_time.rs`）
- 契约源：`packages/core/src/interaction/edit-controller.ts`（523 行）、`packages/core/src/state/chapter-delete.ts`（117 行）、`packages/studio/src/api/server.ts` L3164-L3368
- 验证：`cargo test --lib`（921）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（24）+ `cargo test --features export-bindings --lib`（1080）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

48 号完成 books 状态端点组后，Node sidecar books 域剩编辑事务面：章节人工保存（PUT）、删最新章（DELETE）、版本归档读取与恢复、workspace（brief/plan/versions/canDelete）。本轮 49 号把这组端口全部移植到 Rust，`chapter-workspace.ts` 的版本化基础设施此前已随 write-next 轮次移植完毕（`state/chapter_workspace.rs`），本轮补齐其上的事务编排与 HTTP 面。

## 二、交付内容

### 1. `engine-rs/src/interaction/edit_controller.rs`（新建，~290 行）

移植 `executeChapterReplace`（TS `EditRequest` 六 kind 中 HTTP 面唯一可达的 `chapter-replace`）：

- `ExecutedEditTransaction`（camelCase 序列化：transactionType/bookId/chapterNumber/touchedFiles/reviewRequired/summary）
- `execute_chapter_replace(state, book_id, chapter_number, full_text, version_source, now_millis, now_iso)`：
  1. `fullText.trim()` 空 → 错误逐字 `"Chapter replacement requires fullText."`
  2. 定位 `chapters/{NNNN}_*.md`（前缀**必须带下划线**，对齐 TS `findChapterPath`；未命中 `"Chapter {n} not found."`）
  3. 归档旧稿到 `chapters/.versions/{NNNN}/{millis}_{source}_{uuid_v4}.md`（复用 chapter_workspace，时间注入）
  4. 覆写正文，缺尾换行补 `\n`
  5. 清 `story/runtime/chapter-{NNNN}.*` 工件（保留 `user-brief.md`，unlink 失败静默）
  6. 索引标记：status→`audit-failed`、updatedAt=now、wordCount=roughChapterLength、auditIssues 先剥同文案旧项再追加 `[warning] Manual chapter replacement requires review before continuation.`
  7. touchedFiles = [章节文件相对路径, …runtime 工件, `chapters/index.json`]
- `rough_chapter_length`：剥 frontmatter（`(?m)^---[\s\S]*?---\s*` 首匹配）+ 标题行（`(?m)^#{1,6}\s+.*$`）+ 全部空白后按 **UTF-16 码元**计数（对齐 JS `.length`）
- 单测 3 例：全链（归档/覆写/索引标记/runtime 清除）、空正文与缺章错误、roughChapterLength 语义

### 2. `engine-rs/src/state/chapter_delete.rs`（新建，~270 行）

移植 `deleteLatestChapter`：

- `DeleteRequest` 枚举：`Latest`（缺省最新）/ `Chapter(i64)` / `NaN`（对齐 TS `parseInt` 失败时 `NaN !== latest` 恒真比较——错误文案显示 "NaN"）
- 快照可用性**前置校验**（`story/snapshots/{latest-1}/{current_state,pending_hooks}.md` 任一缺失 → `"Cannot delete chapter N: the state snapshot for chapter M is missing (story/snapshots/M/...). Nothing was changed."`，零文件变更）
- 正文移入 `chapters/.trash/`（重名追加 `-2`/`-3` 后缀，永不硬删），随后走 48 号已移植的 `rollback_to_chapter` 回滚链
- 错误文案逐字：空书 `"Book \"id\" has no chapters to delete."`、中间章 `"Only the latest chapter (N) can be deleted, but chapter M was requested. Deleting a middle chapter would require renumbering later chapters and replaying state."`
- 章节文件匹配正则 `^([0-9]+)[_-]?.*\.md$`（`\d` 限定 ASCII，规避 Rust regex 的 Unicode 数字类偏差）
- 单测 5 例：全链/trash 重名后缀/中间章+NaN 拒绝/快照缺失零变更/空书

### 3. `engine-rs/src/server/books_state_routes.rs`（+6 端点）

| 端点 | 行为要点 |
|---|---|
| `GET .../chapters/:num/workspace` | `{chapterNumber, brief, plan, versions, canDelete}`；非法章节号 400 `"Invalid chapter number"`；canDelete = num === max(index) |
| `PUT .../chapters/:num/workspace/brief` | brief 落盘（trim；空串删文件）；400 `"A valid chapter number and brief string are required"`；无效 JSON 按 TS `.catch(() => ({}))` 视同缺 brief → 400 |
| `GET .../versions/:versionId` | 版本正文；**任何错误 404**（含非法 id/缺文件/非法章节号） |
| `POST .../versions/:versionId/restore` | 读版本 → chapter-replace（source=restore）+ SSE `chapter:restored`；错误 500 |
| `PUT .../chapters/:num` | chapter-replace（source=manual）→ 索引 audit-failed 待复核；错误 500 |
| `DELETE .../chapters/:num` | deleteLatestChapter；错误 **400**（与 PUT 的 500 不同）+ SSE `chapter:deleted`；成功体 `{ok, bookId, deletedChapter, title, trashedFiles, rolledBackTo, discarded}` |

辅助：`ts_parse_int`（对齐 JS `parseInt`：去前导空白/可选符号/数字前缀，无数字 → None=NaN）；路由合并挂载（`chapters/:num` get+put+delete 单 route）。

### 4. 基础设施

- `utils/utc_time.rs` 增 `utc_now_millis()`（13 位毫秒，对齐 `Date.now()`）
- `state/chapter_workspace.rs` `as_id_segment` 提为 pub（端点序列化 versions source 段）

## 三、parity 要点（TS 怪癖固化）

1. **PUT/DELETE 的 parseInt NaN 路径**：PUT 非法章节号 → TS `String(NaN).padStart(4,"0")` 无匹配 → `"Chapter NaN not found."` 500；DELETE → `NaN !== latest` → 中间章错误文案内嵌 "NaN" → 400
2. **workspace brief 写入**：trim 后空串 → `rm { force: true }` 删文件（非写空文件）
3. **versions GET 与 restore 错误码不对称**：GET 404 / restore 500（TS 同款——restore 的 assert 错误落外层 catch）
4. **DELETE 成功 200 / 失败 400**（server.ts L3333 `c.json(..., 400)`；PUT 则 500）
5. **markChapterForManualReview 的 wordCount 覆写**用 roughChapterLength（剥 frontmatter+标题+空白），非全文字数
6. **归档先于覆写**：即使覆写失败旧稿已归档，无数据丢失窗口

## 四、偏差备案

1. PUT chapters 缺 `content` 字段/非字符串：TS 抛 TypeError → 500 `"TypeError: Cannot read properties of undefined (reading 'trim')"`；Rust → 500 `"content must be a string"`（状态码一致，文案不同）
2. 无效 JSON body：TS Hono 未捕获异常 → 500 纯文本；Rust → 500 `"invalid JSON body"`（json_string_field 内联处理，仅此一处文案）
3. 版本缺文件的 404 错误体：TS 为 ENOENT 原生英文长文案；Rust 为 `"chapter version not found: {id}"`（状态码一致）
4. workspace 的 brief PUT 无效 JSON：TS `.catch(() => ({}))` → 400；Rust `serde_json::from_slice().unwrap_or(json!({}))` 同语义 → 400 ✓（无偏差，列为对照）

## 五、暂缓件

- edit-controller 其余 kind：`entity-rename`（全库替换+文件重命名规划）、`chapter-local-edit`（精确/弹性空白/近似段落三级替换 + dice 相似度）、`truth-file-edit`、`focus-edit`——仅 agent-tools/project-tools 的 LLM 工具路径可达，随交互 runtime 移植
- `planEditTransaction` 规划面（同理，工具路径）
- `POST .../workspace/inspiration`（LLM 灵感卡，temperature 0.9 chatCompletion）——归入下轮交互运行时子集
- acquireBookLock 跨进程文件锁（沿 41 号起的进程内互斥策略，strangler 下 Node/Rust 不同域不同时写同一书）

## 六、下一步（50 号候选）

1. **交互运行时子集**：`POST /books/:id/export-save`（落盘变体）+ workspace/inspiration（LLM 卡片）
2. books/create + create-status、detect/import/fanfic 域端点
3. sessions/state 配置域端点（Phase 2 清单）
4. Node sidecar 下线核对：books 状态组（48 号 11 端点）+ 本轮编辑事务组（6 端点）已可切换

## 七、影响面

- Rust 业务端点累计：26（48 号前）+ 6 = **32 个**（不含 health/utils）
- 新增测试：lib +8（edit_controller 3 + chapter_delete 5）、e2e +5（books49_e2e）
- 无破坏性变更：既有端点行为未动；`chapters/:num` 路由由 get 扩为 get+put+delete（同一处理器函数）
