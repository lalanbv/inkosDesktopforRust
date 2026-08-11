# 08 — Phase 2 utils 域：story-markdown 解析子集（state-bootstrap 前置）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`07_Phase2_utils域_hook-arbiter仲裁器.md`
> 性质：state-bootstrap 的解析层前置依赖（markdown → 结构化状态）

## 移植内容

### `utils/story_markdown.rs`（解析子集扩展）
移植自 `packages/core/src/utils/story-markdown.ts`（346 行）的解析函数子集。
此前（07 号）仅含 `normalize_hook_id`，本次补齐 state-bootstrap 需要的全部解析逻辑：

**表格解析**：
- [`parse_markdown_table_rows`]：markdown → `Vec<Vec<String>>`（split 行 → `|` 开头 → 排除分隔线 → split `|` 取 `slice(1,-1)` → trim → 排除全空行）
- [`parse_chapter_summaries_markdown`] → `Vec<StoredSummary>`（首列须纯数字章节号）
- [`parse_pending_hooks_markdown`] → `Vec<HookRecord>`（表格路径 + bullet fallback）
- [`parse_current_state_facts`] → `Vec<NewFact>`（字段/值表格 + bullet fallback）

**辅助函数**：`is_state_table_header_row` / `is_current_chapter_label` / `infer_fact_subject` /
`parse_integer` / `parse_strict_chapter_integer` / `parse_pending_hook_row` / `parse_depends_on` /
`parse_boolean_cell` / `parse_optional_boolean_cell` / `parse_optional_int` / `parse_hook_status`

## 关键技术点

- **强类型输出对齐**：
  - `parse_pending_hooks_markdown` 直接产出 `Vec<HookRecord>`（models 层强类型，含 Phase 7 元数据），而非 TS 的 `StoredHook`——state-bootstrap 后续零转换。TS 用 StoredHook 是因接口兼容，Rust 强类型更适合直接产目标类型。
  - `parse_current_state_facts` 产出 `Vec<NewFact>`（memory_db 的无 id 输入类型），与持久化层衔接。
- **Phase 7 多形态行解析**（load-bearing）：`parse_pending_hook_row` 按 `row.len()` 分 5 形态：
  - 7 列（legacy pre-timing）/ 8 列（Phase 5-6）/ 11 列（compact）/ 12 列（+half_life）/ 13 列（+promoted）
  - notes 列随形态变化（trailing 诊断列被跳过）；payoff_timing 经 `normalize_hook_payoff_timing` 规范化
- **严格章节号防误读**：`parse_strict_chapter_integer` 经 `normalize_hook_id` 后须 `^\d+$`，否则 0——防止「第141号文明」被宽松解析误读为 141（TS 注释明确记载此 bug 的防护意图）
- **HookStatus 枚举收敛**：TS 状态是字符串（"open"/"progressing"/"deferred"/"resolved" 等），Rust 收敛为 4 变体枚举；`parse_hook_status` 接受中英文别名（"已解决"/"已回收"/"closed"/"done" → Resolved），与 reducer 的 isResolved 语义一致
- **正则 OnceLock**：12 个正则全部 OnceLock 编译一次；CJK/中文标点（`、`/`，`）用 `\x{...}` 显式编码

## 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 渲染函数 | **不移植**（留待 pipeline） | `renderSummarySnapshot`/`renderHookSnapshot` 服务 pipeline/agents 的 ledger 快照写入，语义与 projections.rs 全量投影不同，需单独勘测；state-bootstrap 只依赖解析 |
| 解析输出类型 | 直接产 HookRecord/NewFact/StoredSummary | Rust 强类型，避免 TS 的 StoredHook→HookRecord 转换层；state-bootstrap 零转换 |
| bullet fallback | 忠实 TS 两路径 | 表格优先，无表格时 bullet（`- xxx`）→ notes-only hook / note_N fact，与 TS 1:1 |

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **22 项**（解析子集）全绿，含 5 形态 hook 行、bullet/table fallback、严格章节号防护 |
| 全量 lib 测试 | **346 passed**（上轮 324 + 本次 22） |
| golden 差分测试 | **22 passed**（未触及） |
| clippy 双模式 | 零警告（default + `--features export-bindings`，无新增警告） |

## state-bootstrap 解锁进度

state-bootstrap（645 行 async fs 编排）的依赖现已全部就位：
- ✅ `normalize_hook_id` + 解析子集（本次，story_markdown.rs）
- ✅ `normalize_hook_payoff_timing`（hook_lifecycle，已移植）
- ✅ runtime-state models（StateManifest/CurrentState/Hooks/ChapterSummaries schema）
- ✅ memory_db 类型（StoredSummary/NewFact）

state-bootstrap 本体仍待移植（async fs：readdir/readFile/writeFile + JSON schema 校验 + markdown 引导编排）。它完成后即解锁 runtime-state-store（164 行），至此 state 域 I/O 编排层主链贯通。
