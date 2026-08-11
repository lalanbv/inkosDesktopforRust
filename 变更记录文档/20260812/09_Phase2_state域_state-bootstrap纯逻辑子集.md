# 09 — Phase 2 state 域：state-bootstrap 纯逻辑子集

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`08_Phase2_utils域_story-markdown解析子集.md`
> 性质：state-bootstrap 的可单测纯逻辑切片（async fs 编排的前置）

## 移植内容

### `state/state_bootstrap.rs`（纯逻辑子集）
移植自 `packages/core/src/state/state-bootstrap.ts`（645 行）的纯逻辑函数。async fs 编排
（`bootstrapStructuredStateFromMarkdown` / `loadOrBootstrap*` / `loadMarkdown*State` /
`resolveRuntimeLanguage` / `resolveDurableStoryProgress`）与 JSON 修复（`repairHooksStateInput`，
操作反序列化前的 `unknown`）留待后续阶段——后者在 Rust 强类型下需重新设计为 deserialize 后验证。

**已移植**：
- [`resolve_contiguous_chapter_prefix`]：连续章节前缀（durable progress 核心，export）
- [`deduplicate_summary_rows`]：按 chapter 去重（后出现覆盖）+ 升序
- [`normalize_hook_status`]：四类模糊正则归一（resolved/deferred/progressing/open + 中英文同义词）+ warning
- [`normalize_hook_type`] / [`parse_strict_integer_with_warning`] / [`parse_integer_with_fallback`] /
  [`parse_strict_integer_cell`] / [`normalize_explicit_chapter`] / [`append_warning`] / [`unique_strings`]

## 关键技术点

- **`normalize_hook_status` 模糊归一**：4 组正则（resolved/deferred/progressing/open），每组含中英文同义词
  与分隔符变体（`paid[_ -]?off`），按序首匹配；未识别 → warning + 回落 open。与 TS 逐字一致——
  决定 hooks.json 反序列化后的状态收敛，影响 stale-detection/governance 判断（load-bearing）。
- **与 story_markdown::parse_hook_status 的分工**：
  - `story_markdown::parse_hook_status`（08 号）：精确匹配，服务 `parsePendingHookRow` 的表格行初步归一
  - `state_bootstrap::normalize_hook_status`（本次）：模糊正则 + warning，服务持久化前的最终归一
  - 两者并存，对应 TS 里 story-markdown 保留原始串、state-bootstrap 做模糊归一的两阶段设计
- **泛型 + 闭包提取器**：`deduplicate_summary_rows<T: Clone>(rows, chapter_of: impl Fn(&T) -> i64)`——
  TS 的 `<T extends {chapter: number}>` 结构约束在 Rust 转为显式 chapter 提取闭包，避免为每个结构体
  实现 trait，更灵活且零开销（闭包内联）
- **warning 去重**：`append_warning` 按 string 全等去重——`normalize_hook_type` 连续两次相同 warning
  只记一次（测试验证此语义）
- **正则 OnceLock**：6 个正则（4 状态组 + digits + strict_integer）OnceLock 编译一次

## 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 范围切片 | 纯逻辑优先，async fs 后续 | 纯逻辑可单测、零 harness；async 编排需 tempfile 集成测试，独立阶段更清晰 |
| repairHooksStateInput | 暂不移植 | 操作反序列化前 `unknown`，Rust 强类型下需重新设计为 `Result<HooksState, _>` 的 deserialize 后验证，属 async 阶段 |
| 泛型去重签名 | `Fn(&T) -> i64` 闭包 | 比 `AsRef<SummaryChapter>` trait 更轻量，避免为每个含 chapter 的结构体实现 trait |

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **16 项**全绿（含四类状态正则、去重排序、严格整数防护、warning 去重） |
| 全量 lib 测试 | **362 passed**（上轮 346 + 本次 16） |
| golden 差分测试 | **22 passed**（未触及） |
| clippy 双模式 | 零警告 |

## state 域 I/O 编排层进度

state-bootstrap 完整移植所需的两类资源：
- ✅ **纯逻辑**（本次）：normalization/dedup/contiguous-prefix
- ⬜ **async fs 编排**：markdown 引导（`bootstrapStructuredStateFromMarkdown`）、loadOrBootstrap* 系列、
  resolveRuntimeLanguage、resolveDurableStoryProgress + loadDurableArtifactChapterNumbers
- ⬜ **JSON 修复**：repairHooksStateInput（需 deserialize 后验证的重新设计）

async fs 编排完成后即解锁 runtime-state-store（164 行）。该编排需 tokio + tempfile 测试 harness，
列为下一独立子目标。
