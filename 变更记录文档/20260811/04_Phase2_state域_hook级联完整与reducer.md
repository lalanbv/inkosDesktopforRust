# 04 — Phase 2 state 域：hook 级联完整移植 + state-reducer 归约核心

> 日期：2026-08-11
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`00_全面审查与Rust化规划.md`、`03_Phase1_utils域移植_hook级联.md`
> 提交：`560cad3b`（hook-governance）、`b3694e8c`（state-reducer）

## 背景

`state-reducer` 是状态机归约核心（应用 RuntimeStateDelta → 新快照），依赖链：
`hook-policy` ✓ → `hook-lifecycle` ✓（03 已交付）→ **`hook-governance`**（本次）→ `state/validator` ✓ → **`state/reducer`**（本次）。

本次完成整条级联的最后两环，state 域归约逻辑全部 Rust 化。

## 移植内容

### 1. `engine-rs/src/utils/hook_governance.rs`（~300 行）
移植自 `packages/core/src/utils/hook-governance.ts`（200 行）：

| 函数 | 行为 |
|------|------|
| `collect_stale_hook_debt` | lifecycle.stale‖overdue 或 staleAfterChapters 覆盖；按 lastAdvanced/startChapter/hookId 升序 |
| `evaluate_hook_admission` | normalize_text（`[^a-z0-9\u{4e00}-\u{9fff}]`）→ 英文 ≥4 词/中文 {2,6}/中文二元组重叠去重 |
| `classify_hook_disposition` | defer > resolve > advance(match chapter) > mention > none |

关键点：
- `STOP_WORDS` 内联为 `matches!`（避免 `HashSet` 字面量）
- `HookPayoffTiming` → `Option<&'static str>` 转换器（避免 Cow 复杂度）
- 10 单测（含中文二元组滑动窗口验证）

### 2. `engine-rs/src/state/reducer.rs`（~480 行）
移植自 `packages/core/src/state/state-reducer.ts`（274 行）：

| 函数 | 行为 |
|------|------|
| `apply_runtime_state_delta` | 守卫（倒退/一致性/重复）+ 应用 + 最终 validateRuntimeState |
| `apply_hook_ops` | upsert（merge / 准入 / duplicate_family 合并）+ resolve + defer；三键排序 |
| `merge_hook_record` | prefer_richer_text + merge_hook_status + resolveHookPayoffTiming |
| `apply_current_state_patch` | 6 字段别名表（zh/en 双序）+ 倒序删除旧 fact |
| `apply_summary_delta` | allowReapply 覆盖语义 |

关键点：
- **不可变**：输入快照不被修改，所有变更产出新结构（对齐 coding-style 不可变原则）
- **UTF-16 长度**：`prefer_richer_text` 用 `encode_utf16().count()` 对齐 JS `.length`（迁移模式）
- **Zod→serde**：TS `Schema.parse()` 的运行时校验由 Rust 静态类型 + 最终 `validate_runtime_state` 双层覆盖
- **PartialEq**：`RuntimeStateSnapshot` derive PartialEq 以支持 `assert_eq!(Err(...))` 比对
- **thiserror**：`StateReducerError` 5 变体，语义对齐 TS throw 分支
- 13 单测（含 duplicate_family 新 upsert 合并进既有 hook 的关键路径）

## 验证

| 维度 | 结果 |
|------|------|
| hook_governance 单测 | 10/10 |
| state::reducer 单测 | 13/13 |
| 全量 lib 测试 | **389 passed** |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告（no-feat + `--features export-bindings`） |

## 累计交付

engine-rs 至此：
- **utils 域**：18 个子模块（含 hook_policy/hook_lifecycle/hook_governance）
- **state 域**：validator + reducer（归约核心完整）
- **lib 测试**：389 项全绿
- **golden**：22 项 TS↔Rust 差分守门

## 下一步

state 域归约已完成；剩余 state 域工作为 **memory-db**（node:sqlite → rusqlite 持久化层）+ **state-projections** + **manager/state-bootstrap**（持久化编排）。这些依赖 rusqlite 已在 shell 中可用，但量大，列为独立子阶段。
