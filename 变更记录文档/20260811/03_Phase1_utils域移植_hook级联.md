# 03 — Phase 1 utils 域移植：hook 级联（hook-lifecycle）

> 日期：2026-08-11
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`00_全面审查与Rust化规划.md`、`02_Phase1_utils域移植首批.md`
> 提交：`ea3a3738`

## 背景

`state-reducer` 是状态域运行时核心，依赖链：
`state/validator` ✓ → `hook-policy` ✓ → **`hook-lifecycle`**（本次）→ `hook-governance`（待）→ `state-reducer`。

`hook-lifecycle` 原本依赖 StoredHook（来自未移植的 memory-db）。为不级联触发大块移植，本次用已存在的强类型 [`HookRecord`]（字段兼容：status/promoted/lastAdvancedChapter/startChapter）替代。

## 移植内容

`engine-rs/src/utils/hook_lifecycle.rs`（~300 行），移植自 `packages/core/src/utils/hook-lifecycle.ts`：

| 函数 | 行为 |
|------|------|
| `normalize_stored_hook_status` | 状态字符串 → `HookStatus`（中英别名正则，OnceLock） |
| `filter_active_hooks` | 排除 resolved/deferred + promoted !== false |
| `is_future_planned_hook` / `is_hook_within_chapter_window` | 章节窗口判定 |
| `normalize_hook_payoff_timing` | 节奏别名（5 组，按序匹配）→ `HookPayoffTiming` |
| `infer_hook_payoff_timing` | expectedPayoff + notes 信号词推断（endgame 优先） |
| `resolve_hook_payoff_timing` | 显式优先，否则推断 |
| `localize_hook_payoff_timing` | 中/英标签 |
| `describe_hook_lifecycle` | 核心：timing→phase→age/dormancy→overdue/stale/ready_to_resolve→advance/resolve 压力 |

## 关键技术点

1. **正则 OnceLock**：6 个正则按需编译一次（TS 原文为模块级字面量）。
2. **clippy too_many_arguments**：`describe_hook_lifecycle` 8 入参，加 `#[allow(clippy::too_many_arguments)]`（与 TS 8 参数签名对齐，不宜拆参）。
3. **类型替代字符串**：`filter_active_hooks` 用 `matches!(h.status, HookStatus::...)` 而非字符串规范化（HookRecord 已是强类型，比 TS 的字符串判断更准）。
4. **依赖隔离**：用 HookRecord 避开 memory-db，保持 hook 级联可独立验证。

## 验证

- lib 单测：7 项（normalize/infer/resolve/localize/describe overdue/describe fresh）
- 全量：**366 lib + 22 golden 全绿**
- clippy 双模式（无特性 + `--features export-bindings`）零警告

## 下一步（级联）

`hook-governance`（依赖 hook-policy ✓ + hook-lifecycle ✓ + HookRecord ✓）→ 解除 `state-reducer` 阻塞。
