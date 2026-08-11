# 12 — Phase 2 state 域：state-bootstrap markdown 引导层

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`11_Phase2_state域_state-bootstrap编排中间件.md`
> 性质：state-bootstrap 主入口的最后一块前置（markdown → 结构化状态聚合）

## 移植内容

### `state/state_bootstrap.rs`（markdown 引导层扩展）
移植从 markdown 重建结构化状态的完整链路：

- [`parse_pending_hooks_state_markdown`]：`pending_hooks.md` → [`HooksState`]（调用 `parse_pending_hooks_markdown` + `normalize_hook_type`）
- [`parse_current_state_state_markdown`]：`current_state.md` → [`CurrentStateState`]（字段/值表格 + bullet fallback，chapter 经 `parse_integer_with_fallback`）
- [`load_markdown_summaries_state`]：读 `chapter_summaries.md` → [`ChapterSummariesState`]（去重 + 升序）
- [`load_markdown_hooks_state`] / [`load_markdown_current_state`]：读对应 md → 状态对象
- [`load_markdown_bootstrap_state`]：聚合三者 + `durable_story_progress`（`max(显式 fallback, 章节产物前缀)`）
- [`MarkdownBootstrapState`]：聚合状态结构

## 关键技术点

- **status 二次 normalize 的等价性**：TS `parsePendingHooksStateMarkdown` 对 status 走 `normalizeHookStatus`（模糊正则）；
  Rust 的 `parse_pending_hooks_markdown`（08 号）已用精确匹配收敛为 `HookStatus` 枚举，故此处不再二次 normalize——
  语义等价（精确匹配是模糊正则的子集命中），避免枚举↔字符串的有损往返。只对 type 走 `normalize_hook_type`。
- **类型分流（i64 → u32 clamp）**：story_markdown 的 `parse_chapter_summaries_markdown` 产 `StoredSummary`（i64 chapter，
  对齐 memory-db）；state-bootstrap 转 `ChapterSummaryRow`（u32 chapter，对齐 runtime-state models）。
  转换处 `chapter.max(0) as u32` clamp，负数归 0。
- **CurrentStateFact vs NewFact**：`CurrentStateState.facts` 用 `CurrentStateFact`（u32 chapter），
  memory-db 的 `NewFact` 用 i64——两个建模分别对齐消费方（runtime-state models vs rusqlite 持久化）。
  `parse_current_state_facts`（story_markdown）产 NewFact，`parse_current_state_state_markdown`（本次）产 CurrentStateState——
  各服务其消费路径，避免跨域类型耦合。
- **book_dir 与 story_dir 分离**：`load_markdown_bootstrap_state(book_dir, story_dir, ...)` 分开传，
  而非 TS 的 `story_dir = join(book_dir, "story")`。这是 StateStore trait 的灵活性——
  测试可用任意目录前缀（`book`/`story` 独立），生产由调用方拼合。零运行时开销。
- **durable_progress 反幻觉**：`authoritative_progress = fallback.max(durable_artifact_progress)`，
  current_state 的 fallback 用此值（而非 current_state.chapter），确保不被 markdown 幻觉数字污染。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **9 项**（parse 4 + load/aggregate 5）全绿 |
| 全量 lib 测试 | **398 passed**（上轮 389 + 本次 9） |
| golden 差分测试 | **22 passed**（未触及） |
| clippy 双模式 | 零警告 |

## state-bootstrap 移植进度（主入口解锁在即）

- ✅ StateStore trait（10 号）
- ✅ resolve_durable_story_progress（10 号）
- ✅ 纯逻辑 normalization（09 号）
- ✅ story_markdown 解析（08 号）
- ✅ 编排中间件 language/repair/load（11 号）
- ✅ **markdown 引导层**（本次：parse + load + aggregate）
- ⬜ `load_or_bootstrap_*`（4 个：读 JSON 或从 markdown 引导，写回）
- ⬜ **主入口 `bootstrap_structured_state_from_markdown`**（编排 4 文件 + manifest）

主入口的所有**前置函数已全部移植**（durable progress / language / repair / load_json / load_hooks /
markdown 引导 / aggregate）。剩余的 `load_or_bootstrap_*` 与主入口是**编排胶水**——
读 JSON 失败时从 markdown bootstrap 状态填入 + 写回文件。下一目标即完成主入口，state-bootstrap 全量 Rust 化，
解锁 runtime-state-store（164 行）。
