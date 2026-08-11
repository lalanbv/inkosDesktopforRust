# 05 — Phase 2 state 域：投影渲染 + 字数同步纯内核

> 日期：2026-08-11
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`04_Phase2_state域_hook级联完整与reducer.md`
> 提交：`e5adc8e1`（projections 三件套）、`ccc50f37`（chapter-word-sync）

## 移植内容

### 1. `utils/hook_promotion.rs`（最小子集）
`defaultHalfLifeChapters` + `resolveHalfLifeChapters`（immediate/near-term=10, mid-arc=30, slow-burn/endgame=80）。
晋升 pass / escapeRegex / 中文数字解析属 consolidator/runner 运行时，后续阶段移植。

### 2. `utils/hook_stale_detection.rs`（~210 行）
移植自 `hook-stale-detection.ts`（168 行）：
- `compute_hook_diagnostics`：stale（distance>halfLife 且未回收且已种下）+ blocked（depends_on 未清）+ blockedDistance（min 引用章节 → max 距离）
- `render_hook_diagnostic_marker`：zh/en 双语 token（`过期`/`受阻于`/`已阻 N 章`，格式 load-bearing，reviewer 读取）
- 用 `HookRecord`（强类型）替代 `HookLike` 联合类型；`isResolved` 用变体匹配替代 regex

### 3. `state/projections.rs`（~330 行）
移植自 `state-projections.ts`（255 行）：
- `render_hooks_projection`：13 列 hooks 表（含 depends_on/pays_off_in_arc/core_hook/half_life/promoted）+ diagnostics marker 嵌入 status 单元格
- `render_chapter_summaries_projection`：8 列章节摘要表（章节升序）
- `render_current_state_projection`：6 槽位（location/state/goal/constraint/alliances/conflict）别名匹配 + note_N 额外事实排序
- `escape_table_cell`（`|`→`\|`）；resolveHookPayoffTiming→localize 双层

### 4. `state/chapter_word_sync.rs`（纯内核）
移植自 `chapter-word-sync.ts`（93 行）的纯逻辑：
- `parse_chapter_number_from_filename`：`^(\d+)[_-]?.*\.md$`
- `compute_word_count_changes`：索引 + 内容快照 → 变更/缺失/next_index（写回准备）
- fs I/O（readdir/readFile/saveChapterIndex）留给调用方注入；时间戳外部传入（内核不依赖时钟）

## 关键技术点

- **强类型替代 regex**：`isResolved` 用 `matches!(status, HookStatus::Resolved)` 替代 TS 5-pattern regex（serde 反序列化时已收敛 closed/done/已回收 → Resolved）
- **load-bearing token 格式**：`render_hook_diagnostic_marker` 的输出格式被 reviewer prompt 逐字读取，必须 1:1（`已阻 N 章`/`blocked N chapters`）
- **纯内核 + I/O 注入**：chapter-word-sync 将副作用剥离，纯函数可单测；这是后续 async 编排文件（manager/state-bootstrap/memory-db）的移植范式
- **不可变 + UTF-16**：projections 全程产出新 String；alias 匹配用 `to_lowercase`（对齐 TS `.toLowerCase()`）

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | 20 项（4+7+8+1）全绿 |
| 全量 lib 测试 | **409 passed** |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## state 域现状

state 域**纯逻辑层全部完成**：
- ✅ validator（快照校验）
- ✅ reducer（增量归约核心）
- ✅ projections（markdown 投影）
- ✅ chapter_word_sync（字数同步纯内核）

state 域**剩余**（async I/O 编排 + 持久化，属不同性质工作）：
- memory-db（node:sqlite → rusqlite，359 行）
- runtime-state-store（164 行）
- chapter-delete / chapter-workspace（116/163 行 I/O 编排）
- manager（821 行）/ state-bootstrap（645 行）—— 大编排

这些需 tokio + 文件系统/sqlite + trait 依赖注入，单测需集成 harness，列为独立子阶段。
