# 798 号：promises 正典 state 派生——账本解耦（695 工项第二端点收敛）

日期：2026-11-01。前置：795 号共享面 worklist（promises 恒空 vs rust 满数据）+ 794 号「state 为正典真源」确立。

## 一、修复：promises 处理器正典 state 派生

旧实现：读 `books/<id>/story/memory.db`（MemoryDBReadonly getAllHooks/getSummaries）——双重失效：①路径=legacy 副本（2.0 活书在 works 树）；②2.0 写链不产账本表（node memory.db 仅 retrieval_documents 检索表，无 ledger schema——797 号分诊实锤）→ promises 恒空。

新实现：从 `state.bookDir(id)/story/state/` 正典派生——
- hooks：`state/hooks.json`（HookRecord 全字段）+ pending_hooks.md 台账第 14 列 kind 补齐（R23/396 号语义不变）；
- summaries：`state/chapter_summaries.json` rows（chapter/hookActivity）；
- currentChapter：末章+1（rust get_summaries max+1 同语义）；
- 投影函数不变：buildPromiseTimeline/detectPacingDebts/detectWeakHookRuns（promise-ledger.ts 同源）。

依据：rust memory.db ledger 本身就是 settle delta 的投影（与 hooks state 同源）——读正典 state 与读 ledger 等价且免去 ledger 缺表分支。

## 二、活体验证

- diff 报告模式：promises 从「恒空（currentChapter 0/timeline []）」变为**有数据且 currentChapter 3=3 双腿一致**。
- 分歧收窄到 /timeline 内容：node 6 条（hook/hook-2/hook-3/hook-4...——**reducer 对 newHookCandidates 派生 id**）vs rust 4 条（H01-H04——**rust reducer 接受显式 id upsert**）。这是真 reducer 语义差（新钩子 id 派生策略），登记 695 主工程。

## 三、剩余 worklist（695 主工程，按需开工）

| 分歧 | 根因 | 形态 |
|---|---|---|
| promises /timeline hook id | TS reducer 对 candidates 派生 id（hook-N）vs rust 接受显式 id upsert | reducer 语义对齐 |
| quality-trend currentChapter 0 | listReviewMetrics 账本表 node 不产（审计历史记录面） | 审计指标记录移植 |
| roster-candidates 0 vs 1 | 候选生成逻辑移植 | 生成逻辑移植 |
| eval 空（node 书目状态被清） | plan/compose→preserveFailedWork 回滚链（797 号） | episode 注册/回滚语义 |
| detect 404 | 检测结果存储移植 | 存储移植 |

## 四、教训

1. **账本是投影不是真源**：rust memory.db ledger 与 hooks state 同源（同一 settle delta 的两个落点）——读正典 state 免去「账本缺表」整条分支，且天然挂载 works 活书路径。
2. **路径修复与数据源修复要一次到位**：仅改路径（books→works）不换数据源，memory.db 照样空——分诊时把「路径层」与「数据源层」分开定性（797 号分诊表的价值）。
