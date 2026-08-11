# 24 — Phase 1 utils 域：hook-health（伏笔健康度分析）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`23_Phase3_agents域_continuity类型层与sensitive-words.md`

## 移植内容

### `utils/hook_health.rs`
移植 `hook-health.ts`（192 行）——伏笔债务健康度检查：

- [`analyze_hook_health`]：4 类 warning 检查
  1. 活跃伏笔数超 `maxActiveHooks`（12）上限
  2. 压力区（stale / overdue / readyToResolve）伏笔本章未推进/回收/延后
  3. 连续 `noAdvanceWindow`（5）章无真实推进（仅在无压力区未处理时）
  4. `newHookBurstThreshold`（2）个新伏笔但无回收
- [`build_pressure_description`]：压力区 top3 摘要 + 剩余计数
- `localize_pressure_label`：overdue/ready/stale 中英标签
- `HookHealthParams` 输入结构

### `utils/hook_policy.rs`（追加）
- `HOOK_HEALTH_DEFAULTS` 常量 + `HookHealthDefaults` 结构（maxActiveHooks=12 / staleAfterChapters=10 / noAdvanceWindow=5 / newHookBurstThreshold=2）

## 关键技术点

- **枚举↔字符串桥接**：`describe_hook_lifecycle` / `normalize_stored_hook_status` 接收 `&str`，
  但 `HookRecord.status` / `payoff_timing` 是枚举。用 `hook_status_str` / `payoff_timing_str`
  把枚举经 serde 名转字符串（open/progressing/mid-arc 等），与 lifecycle 模块的字符串接口对齐。
- **压力区判定**：`stale_hook_ids`（collect_stale_hook_debt）∪ `ready_to_resolve` ∪ `overdue`。
  `unresolved` = 压力区中本章 disposition 为 none/mention 的（未真正处理）。
- **noAdvanceWindow 仅在无 unresolved 时检查**：避免与压力区警告重复（TS else 分支语义）。
- **newHookBurst 计算**：`delta.hook_ops.upsert` 中 id 不在 existingHookIds 但在 resulting hooks 的数量
  （真正新增的），且 resolve 为空 → 警告。
- **HookHealthParams 生命周期**：`<'a>` 引用 hooks/delta/existing_hook_ids，零 clone（除 collect_stale_hook_debt
  需 owned Vec 的一次 clone——其签名收 `&[HookRecord]` 但过滤后返回 owned）。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **7 项**（少量无 issue / 超上限 / noAdvanceWindow 停滞 / 英文 / newHookBurst / resolved+deferred 排除 / 压力描述截断）全绿 |
| 全量 lib 测试 | **520 passed**（上轮 513 + 本次 7） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## utils hook 域补全

hook 域工具链完整：policy（+HEALTH_DEFAULTS）/ lifecycle / governance / arbiter / stale_detection /
promotion / **ledger_validator** / **health**（本次）。hook 子链的全部纯逻辑工具均已 Rust 化。

## 会话累计（23 个里程碑）

lib 测试 278 → **520**（+242 测试，0 回归），22 golden 全绿，clippy 双模式零警告。
