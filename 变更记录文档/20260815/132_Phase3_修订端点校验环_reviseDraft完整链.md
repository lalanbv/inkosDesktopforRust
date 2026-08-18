# 132 号（Phase3）：修订端点校验环——reviseDraft 完整链

日期：2026-08-18 · 前置：131 号（修订端点仅对齐了真相覆盖的 settle 数据源）

## 背景

131 号把修订端点的真相来源改为结算器（reviser 契约瘦身），但 TS reviseDraft
的**状态校验环**未接：settle 产物与章前快照做矛盾校验，失败走仅重试结算层
（retrySettlementAfterValidationFailure），degraded 则保持原章。本轮闭合。

## 实现

### A. 端口层 baseline 透传

- `SettleRequest`/`SettlementRetryParams` += `baseline_chapter`（TS retry
  settle 的 baselineChapter——重试结算同样走章前快照重放，否则 reducer 拒绝
  既有章重结算）。
- `RoutedSettler`/`RepairSettle`（SettlePort 两实现）透传；`TruthValidationParams`
  += baseline_chapter（写新章链 None）。
- 全构造点补齐（repair-state / resync / write-next 真相校验链 None 语义）。

### B. 修订端点校验环（TS runner reviseDraft 1488-1555 逐字）

1. **基准读取**：`story/snapshots/{N-1}` 的 current_state.md/pending_hooks.md；
   缺失即拒绝修订（"Cannot revise chapter N safely: baseline snapshot M is
   unavailable (...)"——真实书由书创建链的 snapshot_state_at(0) 与每章落盘
   快照保证存在；e2e fixture 手工建书同款补齐）。
2. **首验**：state-validator（validate：diff→LLM→解析，repairRequired 三态）。
3. **失败（!passed || repairRequired）→ 仅重试结算层**：
   settle(allowReapply + 反馈 + baseline 重放) → 复验。
4. **Degraded（复验仍 !passed）→ 保持原章**：unchanged + skippedReason
   "Revision kept the original chapter because state settlement did not
   validate after retry." + diagnostics（standard "Revision text and derived
   story state must both validate before any file is replaced."，before/after
   均 pre 计数，remainingIssues 由复验警告映射）——真相与章文件零改动。
5. **Recovered**：settled 替换为重试产物，继续 post merged audit 主链。

### C. 测试

- sub132 双路径 e2e（mock validator 按序 FAIL→PASS / 恒 FAIL）：
  `revise_recovers_when_retry_settlement_validates`（首验 FAIL→重试→复验
  PASS→applied）与 `revise_degrades_to_unchanged_when_retry_still_fails`
 （unchanged + TS 文案逐字 + 章文件保持原文断言）。
- fixture_project 补 snapshots/0（书创建链真实形态对齐）。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1176** 过 |
| `cargo test --test e2e_write_next_contract` | **190** 过（+2） |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1334** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. runner harness 类重构（agentCtxFor/worker-agent/production harness）与
   skills 生产模式绑定——上游 e7c04465 主体结构，影响面最大。
2. writeProductionRunSnapshot（生产运行快照持久化）。
3. think 剥离器（MiniMax 内联 think 块）与 trajectory 遥测头。
