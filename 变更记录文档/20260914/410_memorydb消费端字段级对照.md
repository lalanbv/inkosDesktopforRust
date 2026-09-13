# 410 · memory.db 消费端字段级对照（轻量纯复核，零缺陷结论）

日期：2026-09-14　性质：轻量质量面复核（推送等待期增量）　复核范围：story/memory.db 全部表 × 双端消费端

## 对照矩阵（表 × 写入者 × 消费端 × 结论）

| 表 | 写入面（TS / Rust） | 消费端 | 结论 |
| --- | --- | --- | --- |
| hooks | 检索链 replaceHooks（memory-retrieval） / write_next 投影重建（406 号补） | GET /promises、/quality-trend、hybrid-search | ✓ 406/407/409 三批修复后字段级对齐（kind 走台账真相源补齐，timing 走 canonical 映射） |
| chapter_summaries | 同上（TS seed 8 字段 / Rust seed 8 字段+conflict/reveal 显式 None） | weakRuns、/promises currentChapter | ✓ 一致；张力列两侧投影均为空（真相源在 markdown，tension-curve 端点直读 markdown 不受影响） |
| facts | 检索链 replaceCurrentFacts（TS 146 / RS 245） | current-state facts 查询（get_facts_*） | ✓ 双端写入点对称 |
| retrieval_chunks | hybrid 混合召回链（348/349） | /hybrid-search、RecallTestCard | ✓ 双端对称 |
| review_metrics | write-next 收尾直接写（content_hash 幂等，361 号） | /quality-trend | ✓ 双端对称 |
| quality_debts | scheduler 治理链（337 号） | /quality-debts、QualityDebtsCard | ✓ 双端对称 |

## 结论

伏笔分类链之外的 memory.db 全部表（facts / retrieval_chunks / review_metrics /
quality_debts / chapter_summaries）经字段级对照**零缺陷**；hooks 表的缺陷
已由 406（投影重建）/ 407（分类列）/ 409（枚举序列化）三批闭环。407 后
的 kind 端到端断言已入 walkthrough-fixture 哨兵。

本批为纯复核记录，零代码变更。
