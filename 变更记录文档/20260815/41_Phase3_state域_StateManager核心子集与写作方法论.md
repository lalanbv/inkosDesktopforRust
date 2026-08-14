# 41 号｜Phase 3 state 域：StateManager 核心子集与写作方法论（write-next 装配地基）

> 里程碑：`_writeNextChapterLocked` 的 state 层依赖落地。StateManager 核心
> 子集（book.json 存取 / 控制文档保障含方法论注入 / durable 章节进度 /
> 章节索引含文件重建 / 状态快照）+ writing-methodology 全文逐字 +
> utc_time 共享工具。42 号进入 runner 纯装配 + 路由挂载。

## 背景

40 号扫清 agent 依赖后，runner 装配前唯一缺口是 `this.state.*` 方法面。
按 write-next 消费面移植 StateManager 子集（821 行 TS 中的 ~300 行核心），
暂缓件明确备案。writing-methodology 是 ensureControlDocuments 的硬依赖
（164 行逐字）。

## 改动

### 新建 `engine-rs/src/utils/writing_methodology.rs`（~230 行，含测试）

zh/en 方法论全文**逐字移植**（脚本比对 TS 模板字符串：两块 1679/3346 字符
均逐字存在）。首版手抄漏一字（"反制"→"制"），比对抓出修正——逐字纪律
以工具验证兜底。

### 新建 `engine-rs/src/utils/utc_time.rs`

`utc_now_iso` / `unix_to_utc_iso` / `civil_from_unix`（Howard Hinnant 算法，
从 chapter_persistence 提炼共享；原私有实现后续迁移引用）。

### 新建 `engine-rs/src/state/manager.rs`（~600 行，含测试）

- `StateManager { project_root }`：`books_dir` / `book_dir` / `state_dir`
- `load_book_config`（空文件报错）/ `save_book_config(_at)`（pretty + 建目录）
- `resolve_control_document_language`：**严格 `language === "zh"` 才 zh，
  缺失/其余一律 en**（TS 怪癖：新书无 book.json 时控制文档是英文默认）
- `ensure_control_documents(_at)`：5 目录（story/runtime/outline/roles 两级）
  + author_intent（入参 trim 非空优先）/ current_focus 写缺失 +
  **style_guide 方法论注入**（已含"写作方法论"/"Writing Methodology" 不重复
  注入；缺失则写全文；幂等）
- `get_next_chapter_number`：durable 连续工件链权威（`0001→0002` 断链即停），
  结构化状态仅 bootstrap 副作用不采信进度
- `get_persisted_chapter_count`：`^(\d+)_.*\.md$` 去重计数
- `load_chapter_index`：index.json 非空直用；空/损坏 → 文件重建兜底
- `rebuild_chapter_index_from_files_at`：`^(\d+)[_-]?(.*?)\.md$`，标题余部
  （去前导 `_`、`_`→空格、空 → `第N章`），状态 ready-for-review，字数 =
  **全文去空白码元数（不剥标题行）**，mtime → UTC ISO
- `save_chapter_index(_at)`：空 + 无豁免 → 重建护栏（重建仍空才落空盘）
- `snapshot_state(_at)`：7 真相文件 + state/*.json → snapshots/{n}/
  （缺失文件静默跳过）

## parity 要点（移植难点）

1. **语言判定怪癖**：`parsed.language === "zh" ? "zh" : "en"`——无三态默认
   zh；book.json 缺失 → en。与 planner/writer 的 `?? "zh"` **不同源**，勿统一
2. **索引字数含标题行**：`content.replace(/\s+/g, "").length` 对整个文件
   （含 `# 第N章` 行）——单测踩坑后固化
3. **durable 进度 vs 结构化状态**：下一章号只信连续工件链；state/*.json 的
   bootstrap 是副作用（写缺失文件）而非进度来源
4. **方法论注入的幂等判据**：包含"写作方法论"**或**"Writing Methodology"
   任一即视为已注入（TS 双语判据）
5. **空索引护栏的条件**：`index.length === 0 && !allowEmptyWithChapterFiles`
   → 重建非空替换，否则原样落盘（含真空）

## 暂缓件（后续端点按需移植，此处备案）

- `acquireBookLock` 文件锁（陈锁恢复/重试 unlink）：strangler 模式下
  Node/Rust 分域不并发写同一书，42 号 runner 先用进程内 per-book tokio 锁，
  CLI 并存场景再补文件锁
- `restoreState` / `rollbackToChapter` / `isCompleteBookDirectory` /
  `listBooks` / project config 存取

## 验证

- **lib 单测**：854 passed / 0 failed（+12 新单测：方法论骨架 / civil 三时点 /
  配置往返与空文件守卫 / 语言判定三态 / 控制文档默认+方法论注入幂等+自定义
  意图 / durable 断链进度与章节数 / 索引重建（标题/字数/状态）+空护栏 +
  手改保留 / 快照复制与静默跳过 / 文件名标题化矩阵）
- **golden 差分**：76 域全绿（无新域——state 层 fs 密集由 Rust 集成测试覆盖）
- **export-bindings**：1013 passed
- **TS 全量**：185 文件 / 1798 测试全绿
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **42 号**（write-next 收官装配）：`write_next_chapter` 编排——
  ensureControlDocuments → loadBookConfig → assertNoPendingStateRepair →
  getNextChapterNumber → prepareWriteInput（35 号 plan 持久化复用 + composer
  governed 三件；legacy 模式只传 externalContext）→ writeChapter（32 号）→
  manual 写完即停 / review-cycle（37 号）→ promotion pass → 标题去重双轮 →
  buildPersistenceOutput（40 号）→ 长跨度疲劳 → truth-validation（38 号）→
  段落形态检查 → persistChapterArtifacts（37 号）→ 通知/webhook +
  `/api/v1/books/:id/write-next` 路由（39 号 task-store + SSE 广播
  write:start/complete/error）+ 进程内 per-book 锁。
