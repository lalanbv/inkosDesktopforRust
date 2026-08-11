# 14 — Phase 2 state 域：runtime-state-store（I/O 编排主链贯通）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`13_Phase2_state域_state-bootstrap全量完成.md`
> 性质：**里程碑**——state 域 I/O 编排主链全量贯通

## 移植内容

### `state/runtime_state_store.rs`（新增，顶层编排）
移植 `runtime-state-store.ts`（164 行）——state 域 I/O 编排层的最顶层，消费 state 域全部子模块：

- [`load_runtime_state_snapshot`]：bootstrap（确保 4 JSON 合法）→ 读入 → validate → `RuntimeStateSnapshot`
- [`build_runtime_state_artifacts`]：load snapshot → `arbitrate_runtime_state_delta_hooks` 裁决 → `apply_runtime_state_delta` 归约 → 3 份 markdown 投影
- [`save_runtime_state_snapshot`]：mkdir + 写 4 JSON
- [`load_narrative_memory_seed`]：snapshot → `{summaries, hooks}`（memory-db 持久化类型，供 MemoryDB 批量写入）
- [`RuntimeStateArtifacts`] / [`NarrativeMemorySeed`] 结构

## 关键技术点

- **全模块协同**：本模块是 state 域的「集成点」，一次性串联 state_bootstrap / reducer / validator /
  hook_arbiter / projections / store 六个子模块。每个子模块独立移植 + 单测，此处仅做编排胶水。
- **bootstrap 幂等性的顶层体现**：`load_runtime_state_snapshot` 先 bootstrap——损坏的持久化 JSON
  会被自动从 markdown 重建（warning 标记），load 仍成功。这是「自愈」设计（对齐 TS），
  最初我写了「损坏应失败」的测试，实测后修正为「自愈成功」——忠实 TS 的 bootstrap 副作用语义。
- **validator 容错 + 强类型收口**：validator 接收 4 个 `serde_json::Value`（容错，解析失败记 issue 不中断），
  校验通过后 runtime_state_store 用 `serde_json::from_value` 强类型反序列化（此时必成功）。
  两阶段：先容错诊断，后强类型收口。
- **resolved_delta chapter 提取**：`apply_runtime_state_delta` 借用 `&resolved_delta`，之后 `RuntimeStateArtifacts`
  move `resolved_delta`——投影渲染需 `resolved_delta.chapter`（用于标注 stale/blocked hooks）。提取
  `let resolved_chapter = resolved_delta.chapter;`（Copy）避免 move 冲突。
- **HookStatus → memory-db 字符串**：`load_narrative_memory_seed` 把 `HookStatus` 枚举经
  `format!("{:?}", status).to_lowercase()`（open/progressing/deferved/resolved）转 memory-db 的 status 字符串——
  memory-db 的 `normalize_hook_status` 模糊正则会再次收敛（语义闭环）。
- **constraint helper**：模块内 `fn constraint(msg) -> EngineError::Constraint(msg)`，
  统一顶层编排的校验/reducer 失败错误变体。

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **5 项**端到端（bootstrap+load / self-heal / build+投影 / save 写回 / seed 映射）全绿 |
| 全量 lib 测试 | **409 passed**（上轮 404 + 本次 5） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## state 域全量 Rust 化达成（里程碑）

state 域 I/O 编排主链**全量贯通**。本会话（06–14 号）完成的 state 域模块：

| 子模块 | 里程碑 | 角色 |
|--------|--------|------|
| memory_db | 06 | rusqlite 持久化层 |
| validator | （此前） | 快照校验 |
| reducer | （此前） | 增量归约核心 |
| projections | （此前） | markdown 投影 |
| chapter_word_sync | （此前） | 字数同步纯内核 |
| **store** | 10 | StateStore fs trait 基座 |
| **state_bootstrap**（全量） | 09–13 | markdown→结构化状态引导 |
| **runtime_state_store** | 14 | **顶层 I/O 编排（本次）** |

state 域对外的能力现已完整：加载快照 / 应用 delta 产出投影 / 持久化 / 喂养 memory-db。

## 下一阶段

state 域剩余（属独立编排，非主链）：
- chapter-delete（116 行）/ chapter-workspace（163 行）—— 章节文件编排
- manager（821 行）—— 大编排（依赖 LLM client + 持久化，Phase 3 高层）

业务运行时（Phase 3，数千行）：agents / pipeline / edit-controller / project-tools / runtime。
134 HTTP 端点接线 / 37 provider 数据文件。

下一会话建议子目标：chapter-delete 或 chapter-workspace（小编排，复用 StateStore trait），
或开始 Phase 3 业务运行时（agents 域）。
