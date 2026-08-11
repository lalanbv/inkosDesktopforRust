# 06 — Phase 2 state 域：memory-db 持久化层（node:sqlite → rusqlite）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`05_Phase2_state域_投影与字数同步纯内核.md`
> 性质：state 域**首个持久化层**移植，标志着从纯函数层进入 I/O 层的关键解锁点

## 移植内容

### `state/memory_db.rs`（~400 行）
移植自 `packages/core/src/state/memory-db.ts`（359 行）。Node.js 内置 `node:sqlite`（`DatabaseSync`）→ [`rusqlite`]（`bundled` feature）：

- **三张表的 DDL 逐字对齐** TS `migrate()`（facts / chapter_summaries / hooks + 5 索引 + 列默认值），确保 Rust 实现可直接读写 TS 版产生的 `story/memory.db` 文件
- **20 个方法忠实对应** TS 同名方法，覆盖三域：
  - facts（temporal）：`add_fact`/`invalidate_fact`/`get_current_facts`/`get_facts_at`/`get_fact_history`/`get_facts_by_predicate`/`get_facts_for_characters`/`replace_current_facts`/`reset_facts`
  - summaries：`upsert_summary`/`replace_summaries`/`get_summaries`/`get_summaries_by_characters`/`get_chapter_count`/`get_recent_summaries`
  - hooks：`upsert_hook`/`replace_hooks`/`get_active_hooks`/`close`
- **类型建模**：`Fact`/`NewFact`（输入不含自增 id）/`StoredSummary`/`StoredHook`。`StoredHook` 只建模持久化的 8 列（与 SQL 完全对齐），TS 版的 Phase 7 可选元数据（dependsOn/paysOffInArc/coreHook 等）不入库——与 TS 行为一致
- **双构造入口**：`open(book_dir)` 忠实 TS（`story/memory.db` + WAL + migrate）；`open_in_memory()` 为纯 Rust 增益测试入口（TS 版无，node:sqlite 的 `:memory:` 需特殊处理），使单测无需临时文件
- **`ensure_column` 等价**：忠实 TS 的 try/catch 静默吞错——通过匹配 SQLite `SQLITE_ERROR` + "duplicate column name" 实现

### Cargo.toml / lib.rs
- 新增 `rusqlite = { version = "0.32", features = ["bundled"] }`（对齐 crate 已有「rustls-tls 避系统 OpenSSL」的自包含哲学，零系统依赖、跨平台一致）
- `EngineError` 新增 `Sqlite(#[from] rusqlite::Error)` 变体

## 关键技术点

- **SQL 逐字对齐而非重写**：所有查询的 WHERE/ORDER BY/边界条件（含中文状态黑名单 `'已回收'/'已解决'`、三键排序 `last_advanced DESC, start DESC, hook_id ASC`、temporal 窗口 `valid_from <= ch AND (valid_until IS NULL OR valid_until > ch)`）均与 TS 源 1:1，避免语义漂移
- **`params_from_iter` 处理动态 IN 子句**：`get_facts_for_characters`/`get_summaries_by_characters` 的变长参数绑定；空数组短路返回空 Vec（与 TS 一致，避免 `IN ()` 语法错误）
- **`r#type` 字段名**：`type` 是 Rust 关键字，用 `r#type` 转义，序列化/SQL 列名仍是 `type`，对前端/数据库零影响
- **RAII 关闭**：`close(self)` 消费连接，比 TS 的显式 `close()` 更安全（Drop 自动释放），无需手动调底层 close（rusqlite 未公开等价 API）
- **TDD 移植**：每个方法先写编码 TS 契约的行为测试（RED，`todo!()` panic）→ 最小实现（GREEN）→ 逐域推进；30 个单测全部"先红后绿"，证明测试能捕获缺陷

## 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| API 形态 | **同步**（非 async） | TS 源（`DatabaseSync`）本身同步；`rusqlite::Connection` 天然同步；编排层（runtime-state-store，下一阶段）在 `spawn_blocking` 包裹 |
| SQLite 绑定 | **`bundled`** | 对齐 crate 哲学（零系统依赖）；桌面应用标准实践；项目无 sqlx，无 libsqlite3-sys 版本冲突 |
| 测试入口 | **`open_in_memory()`** | 纯 Rust 增益；内存库无需临时文件，测试更快更干净 |

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **30 项**（5 schema + 10 facts + 8 summaries + 7 hooks）全绿，全部先 RED 后 GREEN |
| 全量 lib 测试 | **308 passed**（HEAD 基线 278 + 本次 30） |
| golden 差分测试 | **22 passed**（未触及） |
| clippy 双模式 | 零警告（default + `--features export-bindings`） |

### 测试计数订正（诚实记录）
本记录核实：上轮 05 号记录所写"全量 lib 测试 **409 passed**"与实际不符——在 HEAD（`ccc50f37`）实测 lib 测试为 **278 passed**。本记录采用 cargo 实际输出（308）。05 号记录的 409 疑似统计口径误差（可能混入 doc-test 占位或其它过滤），此处订正以免后续记录沿用错误基数。

## state 域现状

state 域**纯逻辑层 + 首个持久化层完成**：
- ✅ validator（快照校验）
- ✅ reducer（增量归约核心）
- ✅ projections（markdown 投影）
- ✅ chapter_word_sync（字数同步纯内核）
- ✅ **memory_db（rusqlite 持久化层）** ← 本次

state 域**剩余**（async I/O 编排 + 业务运行时）：
- runtime-state-store（164 行）—— **现已解锁**（依赖本模块 + reducer + validator）
- chapter-delete / chapter-workspace（116/163 行 I/O 编排）
- manager（821 行）/ state-bootstrap（645 行）—— 大编排

memory-db 是进入 I/O 层的关键解锁点：runtime-state-store 的 `loadNarrativeMemorySeed` 直接消费本模块的 `StoredSummary`/`StoredHook`，下一阶段可顺势移植。
