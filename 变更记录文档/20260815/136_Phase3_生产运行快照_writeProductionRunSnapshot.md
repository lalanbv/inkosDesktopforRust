# 136 号（Phase3）：生产运行快照——writeProductionRunSnapshot

日期：2026-08-18 · 前置：130 号备案项（合并 e7c04465 引入）

## 背景

TS `production/harness.ts`（新模块）+ runner `writeNextChapter` 包装层的
运行快照三点发布：运维真值——completed 的运行绝不指向半写产物集。Rust 缺失。

## 实现

### A. harness 模块（`src/production/mod.rs`，TS 逐字）

- 类型：`ProductionKind`（7 类）/`ProductionRunStatus`（6 态）/
  `ProductionObservation`（metric/expected/actual/severity/evidence?/repairable）
  /`ProductionRunSnapshot`（version=1/kind/id/status/stage/artifacts/
  observations/model?/skillIds?/resumeCursor?/error?/updatedAt——可选键
  省略序列化）。
- `create`（TS createProductionRunSnapshot）：补 version=1 与 updatedAt
 （JS `toISOString()` 同款毫秒 UTC——复用 chapter_workspace 的
  `millis_to_iso` pub 化）；入参 `CreateRunInput` 结构体（TS 对象入参形态）。
- `create_range_observation`：区间观测——在区间=info；出区间 hard=false →
  warning 否则 blocking；repairable=!inRange；expected/actual JSON 形态逐字。
- `write_production_run_snapshot`：`commit_atomic_file_set` 单文件原子发布
 （pretty JSON + 尾随换行）。

### B. write-next 包装层三点发布（write_next_chapter，TS 1912-1995 逐字）

Rust 的 `write_next_chapter`（锁包装层）↔ TS `writeNextChapter`、
`write_next_chapter_locked` ↔ `_executeNextChapterLocked` 结构本就对应——
快照加在包装层：

1. **running**：锁后即发（`story/runtime/chapter-NNNN.run.json`；runId
   `{bookId}:chapter-{padded:04}`；skillIds ["inkos-long-writing"]；
   resumeCursor=章号）。
2. **成功**：ready-for-review → complete，否则 needs-review；artifacts 六项
   清单（章文件 `{padded}_*.md` 目录扫描/chapters/index.json/current_state.md/
   pending_hooks.md/snapshots/{N}/trace.json）+ chapter-length 区间观测
  （lengthTelemetry ?? buildLengthSpec(wordCount ?? book 基准)——观测的
   spec 由遥测重构或回退计算）；章文件缺失 → 失败快照 + 报错。
3. **失败/取消**：abort 信号判定 cancelled/failed + error 消息；快照写失败
   吞掉（不覆盖原始错误，TS `.catch(() => undefined)` 逐字）。
- **差异备案**：model 键省略——Rust 装配无单值 config.model（多 agent 各自
  解析端点模型）。

### C. 测试（+3）

- harness 单测 2：区间观测严重度矩阵（info/blocking/warning×hard=false +
  expected/actual JSON 形态）；快照文件 pretty+尾换行/可选键省略/updatedAt
  ISO 毫秒格式。
- e2e 1（sub136）：write-next 全链后断言 run.json 完整形态——终态 complete、
  id/stage/skillIds/resumeCursor、artifacts 六项逐项、chapter-length 观测
  （evidence=章路径、repairable=severity≠info）、updatedAt 24 位 ISO 毫秒、
  error/model 键省略。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1187** 过 |
| `cargo test --test e2e_write_next_contract` | **191** 过（+1） |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1345** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. runner harness 类重构（agentCtxFor/worker-agent/production harness 类形态）
   与 skills 生产模式绑定——合并 e7c04465 剩余主体。
2. commitProductionArtifacts 的 validate+deletes 完整面（当前 write-next 面
   仅用空集调用形态）。
