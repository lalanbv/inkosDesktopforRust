# 233 号：memory 语义精简层缺口核查——评估报告（不实施）

- 日期：2026-09-09
- 分支：develop
- 关联：83/109 号（memory_retrieval 移植）、216 号（references——同族「检索+精简」模式已移植）
- 推送核验：origin/develop 仍停 6bc65564——**201–232 共 32 提交待推送**；本批次后本地领先 33。两项默认值无新答复。

## 一、缺口确认

TS 写作链的 planner 记忆装配为**两级检索**：
1. `retrieveMemorySelection` 内 SQLite FTS5 BM25 检索（LocalSearchIndex，top-32 候选）；
2. **语义精简层**：`memorySemanticSelector`（runner 装配为 `composer.selectMemoryCandidates`，LLM 温度 0.1/2048 tokens）对候选精选——提示词强调理解否定/修正/因果/别名/改述，不按关键词重合排序；selector 缺失/失败/候选≤1 时回退纯 BM25。

Rust `memory_retrieval.rs`（1.1k 行）**无语义精简层**：`RetrieveMemoryParams` 无 selector 字段，全仓无 `select_memory`/`MemorySemantic` 对应物——但这是**有意的移植设计差异**：Rust 采用确定性词法评分（`extract_query_terms` → `select_relevant_*` 内嵌评分截断，单测镜像 TS 行为），无 BM25 候选集结构。

## 二、影响评估

- **非功能缺失**：TS 的 selector 缺失/失败路径（回退 BM25 直接结果）证明检索功能完整；Rust 恒走该回退等价路径。
- **质量差异**：Rust 词法评分无法识别否定/因果/别名/改述——记忆条目的相关性排序弱于 TS 语义精简后；可能挤占上下文预算或稀释重点。
- **溯源**：`retrievalTrace.semanticSelectedIds`（TS 可观测字段）Rust 无对应。

## 三、实施评估（如移植）

前置重构：Rust 需先把分散在各 `select_relevant_*` 内部的评分重构为「统一候选集 + BM25/词法评分 + 精简过滤」结构（TS 的 rankScores 模式），估 400-600 行 + 单测；随后语义层本身（trait + prompt + LLM 端口 + 装配）约 250 行（216 号 references 的 selector 模式可直接复用）。合计中型工程，建议与 film authoring 一并向产品确认优先级后立项。

## 四、验证

纯核查批次（零代码改动）：全部门禁于 231 号已绿。
