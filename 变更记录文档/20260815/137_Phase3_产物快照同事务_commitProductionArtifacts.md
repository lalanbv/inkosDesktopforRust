# 137 号（Phase3）：产物快照同事务——commitProductionArtifacts 完整面

日期：2026-08-18 · 前置：136 号（writeProductionRunSnapshot 空集形态）

## 勘测结论（范围裁定）

TS `commitProductionArtifacts` 的**真实使用者**是 script-storyboard 与
translation runner（产物 + 权威完成快照同一原子事务 + validate 校验钩子 +
deletes）；write-next 面的产物落盘走 chapter-persistence 自己的原子集，
快照为独立二次发布（136 号已按此对齐）。因此本轮补齐**函数完整面**并
单测验收同事务性；write-next 面保持 TS 等价（不做同事务合并——那会
偏离 TS 行为、引入 duel 差分风险），同事务面为 script/translation 移植
轮的现成积木。

## 实现（`src/production/mod.rs`）

- `commit_production_artifacts`（TS 逐字）：validate 先行（拒即零写入）→
  writes = [...artifacts, 快照末位]（completed 的运行绝不指向半写产物集）→
  deletes 随事务提交。
- `write_production_run_snapshot` 重构为它的**空集薄包装**（TS 同构——
  136 号的内联实现收敛）。

## 测试（+3）

- 同事务提交（TS production-harness.test.ts 同款）：产物与完成快照同落、
  validate 先行计数。
- validate 拒绝 → **零写入**（draft/status 均不存在）。
- deletes 随事务生效（旧稿删除 + 快照就位）。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1190** 过（+3） |
| `cargo test --test e2e_write_next_contract` | **191** 过 |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1348** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. runner harness 类重构（agentCtxFor/worker-agent）与 skills 生产模式绑定
   ——合并 e7c04465 剩余主体。
2. script-storyboard / translation runner 的 Rust 移植（届时接入本同事务面）。
