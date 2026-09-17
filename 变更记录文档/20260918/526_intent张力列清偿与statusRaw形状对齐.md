# 526 号：intent 摘要表张力列清偿 + hooks.json statusRaw 形状对齐——工件备案 4→1 项

日期：2026-09-18　分支：develop　基线：7d313a89（525 号）

## 背景

525 备案清单的 intent 摘要表张力列（ch1 行冲突/揭示强度 TS=6|4、Rust 空）与 hooks.json serde 空字段形状差异的根治专项。

## 定位与修复

### 1. intent 摘要表张力列（memory_retrieval.rs）

双腿现场逐环节排查：mock SETTLER delta **带** conflictLevel 6/4 → serde 反序列化正常 → reducer `apply_summary_delta` clone 无损 → 结构化 `chapter_summaries.json` rows 双端一致（都含张力值）。**丢失环节定位在检索层**：`summary_from_row` 把 `conflict_level`/`reveal_level` **硬编码 `None`**——写前记忆检索的摘要行恒缺张力，intent 的记忆证据块（governed 块由检索行渲染）随之缺列。

修复：透传 `row.conflict_level`/`reveal_level`（`Option<u32>→Option<i64>` 转换）。TS 零改动（TS 检索行本就携带张力）。

### 2. hooks.json `statusRaw` 键（models/runtime_state.rs）

Rust `HookRecord.status_raw`（markdown 原文保留，如 H04 的 `'open'`）序列化进 state JSON；TS `StoredHook` 无此键——形状分叉。`skip_serializing_if = "String::is_empty"` 不足以挡非空原文，改为 **`#[serde(skip_serializing)]`**：原文只在内存判定链流转（`hook_status_text` 空串回退枚举规范名，判定等价）；反序列化走 default 空串，可由 status 重导出。

配套更新 `retrieve_memory_selection_markdown_path` 测试断言（原文跨 json 往返保留 → 回退空串）——TS 语义本不持久化原文。

## 差分器工件备案清单收敛（4→1 项）

- ✅ intent.md 张力列（本号清偿）、hooks.json ×3（statusRaw 清偿）
- ✅ 归一化消除（前序已清）：plan root 路径、rule-stack yaml 格式、index 时间戳、0001 尾换行
- 剩余备案：**仅 node 有 runtime/chapter-0001 intent/plan/rule-stack**（resync 留痕面，519 裁决现状）
- **落盘工件内容面全绿**（「落盘工件内容一致」输出首次达成）

## 验证

- 差分器 **41 对照项 0 分歧**；工件内容面全绿。
- engine-rs 全量 cargo test **1805 绿**（含更新后的 memory_retrieval 断言）；`pnpm clippy:gate` 双 crate 0 告警。
- 双活体套件绿；`gate:ts:fast` 全绿（门禁轮偶发一红为系统负载下交互测试超时抖动，单测 189ms 绿+复跑全绿证实）；`pnpm bench:gate` 零回退（守卫拦截后回落重跑）。

## 教训

- **硬编码 None 是静默丢数据的常态形态**：`summary_from_row` 的一对 `None` 让张力值在检索层整层消失，上游五层全部无损——数据链排查应从终端产物反推到源头，逐层验证「值还在不在」。
- skip_serializing_if 只挡特定空值——非空内部态照样泄漏进契约 JSON；内部中间态字段应默认 `skip_serializing`。

## 关联

- 525（优先级修复与快照断言——本号清偿其备案项）、524（实测优先原则——passed 判定同型）、R23/394（status_raw 引入）、359/409（tension 与 canonical 同型链）。
- 剩余备案 1 项：resync 留痕面（519 裁决现状保留）；工件对照转硬门禁待该项专项清偿后一并。
