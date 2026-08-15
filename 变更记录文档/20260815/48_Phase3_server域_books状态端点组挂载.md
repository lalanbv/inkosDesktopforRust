# 48 号变更记录：books 状态端点组（CRUD/章节/approve-reject/truth/review-mode）

- 日期：2026-08-14
- 范围：engine-rs（state/server）+ E2E
- 主题：books 域状态类端点批量挂载——书籍列表/详情/更新/删除、章节读取、
  approve/reject（回滚链）、truth 文件读、chapter-review-mode 读写

## 一、StateManager 补齐（state/manager.rs）

对齐 `packages/core/src/state/manager.ts`：

- **`list_books`**（L422）：books/ 下含 book.json 的目录名，目录序
- **`restore_state`**（L648）：快照恢复——必需文件（current_state/pending_hooks）
  任一缺失 → false；可选文件（ledger/summaries/subplot/arcs/matrix）快照缺失时
  **删除目标**（回退到快照时刻）；结构化 state/ 有内容则恢复、无则整目录删除
- **`rollback_to_chapter`**（L717）：restore → 索引 kept/discarded 分割 → 删除
  废弃章产物（chapters/ 与 story/drafts/ 的 `NN_*.md`、snapshots/N/、runtime/
  `chapter-NN.*`）→ 删 sqlite 加速索引（memory.db/-shm/-wal，防废弃章回流检索）
  → saveChapterIndex(kept)；失败文案 `Cannot restore snapshot for chapter N in "id"` 逐字

## 二、新模块 `engine-rs/src/server/books_state_routes.rs`（11 端点）

契约逐条对齐 server.ts：

| 端点 | Node 位置 | 契约要点 |
|---|---|---|
| GET /books | L3016 | `{books: [{...bookConfig, chaptersWritten}]}`；chaptersWritten = nextChapter-1；任一书损坏 → 500（TS Promise.all 语义） |
| GET /books/:id | L3022 | `{book, chapters, nextChapter}`；404 `Book "id" not found` |
| PUT /books/:id | L5967 | 四字段更新（chapterWordCount/targetChapters 数字化、status/language）+ updatedAt；`{ok, book}` |
| DELETE /books/:id | L5952 | rm -rf（ENOENT 静默 ok，对齐 force:true）；`{ok, bookId}` + SSE book:deleted |
| GET /books/:id/chapters/:num | L3146 | `{chapterNumber, filename, content}`；**parse 失败也 404**（TS parseInt NaN → 无匹配 → 404，无 400 分支） |
| POST .../approve | L3632 | 索引状态翻转 approved（目标缺失静默 200）；`{ok, chapterNumber, status}` |
| POST .../reject | L3648 | 目标缺失 404；rollbackToChapter(num-1)；num=0 → TS -1 语义 500；`{ok, rolledBackTo, discarded}` |
| GET /books/:id/truth | L4392 | `{files: [{name, size, preview, legacy?, readonly?, readonlyReason?}]}`；size/preview 均 UTF-16 码元语义 |
| GET /books/:id/truth/*file | L3451 | 白名单校验失败 400 `Invalid truth file`；200 `{file, content, frontmatter?, body?, legacy?, readonly?}`；**缺文件 content:null 仍 200** |
| GET/PUT …/chapter-review-mode | L5817/L5837 | `{mode, bookMode, projectMode}`；PUT raw JSON 保未知字段（inherit 删键、writing 空则整体移除） |

### truth 白名单（resolveTruthFilePath 逐条移植）

- 拒绝：空 / `\0` / 绝对路径 / 含 `..`
- 允许：15 平面文件 + 4 大纲文件（outline/story_frame.md 等）+ runtime 诊断
  正则 `runtime/chapter-\d{4}\.(intent\.md|plan\.md|context\.json|rule-stack\.yaml|trace\.json)`
  + 角色卡正则 `roles/(主要角色|次要角色|major|minor)/[^/]+\.md`
- join 后 strip_prefix 校验不逃出 story/

### truth 单文件附加语义

- **frontmatter/body**：`try_parse_book_rules_frontmatter`（已有）成功时附结构化字段
  （UI 卡片渲染用；content 保持原文往返）
- **legacy**：story_bible/book_rules 且 `is_new_layout_book`（已有，story/outline/story_frame.md 存在）
- **readonly + readonlyReason: runtime-diagnostic**：runtime 诊断文件
- **size = UTF-16 码元数**、**preview = 前 200 码元**（`utf16_preview` 辅助）

### review-mode 怪癖（逐一对齐）

- **inkos.json 缺失 → 404 `Book "id" not found`**（TS loadRawConfig 抛 → catch 404）
- projectMode：inkos.json writing.reviewMode（manual→manual，其余含缺失键→auto）
- bookMode：book.json writing.reviewMode **仅精确 manual/auto**，其余 null
- PUT mode=inherit → 删 writing.reviewMode；writing 空对象 → 整体移除（TS undefined
  键不序列化）；PUT 无 mode → auto（TS normalizeChapterReviewMode(undefined)）
- 不安全 id → 400 `Invalid book id`（is_safe_book_id）

## 三、偏差备案

- **PUT /books/:id 数值字段**：TS `Number(x)` 对非法值产生 NaN → JSON null 落盘；
  Rust `coerce_number` 跳过该字段（更安全；NaN 落盘属 TS 腐蚀行为不复刻）
- **PUT status 非法串**：TS 强转落盘任意字符串；Rust BookStatus 严格 → 500
- **reject num=0 错误文案**：章节号与 TS -1 保持一致（i64 计算）

## 四、暂缓件

- `PUT/DELETE /books/:id/chapters/:num`（章节写/删）：依赖 edit-controller
  版本化事务（executeEditTransaction/deleteLatestChapter + archive），编辑事务域整体移植
- `POST /books/create` + `GET create-status`：依赖交互运行时（architect sub-agent LLM 链）
- workspace 域（brief/inspiration/versions/restore）、detect 域、import 域、fanfic 域、
  foundation/revise、rewrite、resync、export-save：同交互运行时/检测服务依赖，后续批次
- acquireBookLock 跨进程文件锁（延续 41 号备案）

## 五、测试

- StateManager 回滚链经端点测试覆盖（快照 v1 → 前进 v2 → reject → 断言文件删除 +
  状态回滚 + 索引收敛 + discarded 列表）
- books_state_routes 单测 8 例（列表摘要/详情 404/更新落盘/章节读 404/approve 翻转/
  reject 回滚/truth 白名单+legacy+frontmatter/review-mode 往返）
- E2E books48_e2e 6 例（**嵌套 wildcard 路由**验证 roles 中文路径与 runtime 诊断文件、
  runtime readonly 标记、白名单 400、review-mode 400 不安全 id、delete 广播 SSE）
- 全量：lib 913（+8）/ golden 76 / E2E 19（+6）/ export-bindings 1072 / clippy 零警告 / TS 1798

## 六、下一步（49 号候选）

1. 编辑事务域（executeEditTransaction + 章节版本化 + PUT/DELETE chapters）
2. 交互运行时子集（export-save 落盘变体）
3. sessions/state 配置域端点（Phase 2）
4. Node sidecar 下线核对：books 状态组（本轮 11 端点）已可切换
