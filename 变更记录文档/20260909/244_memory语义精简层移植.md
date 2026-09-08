# 244 号：memory 语义精简层移植——BM25 候选 → LLM 精选 → rankScores 对齐 TS

- 日期：2026-09-09
- 分支：develop
- 关联：233/243 号（评估——本批立项实施）、241 号（LocalSearchIndex 内核复用）、216 号（selector 模式复用）
- 推送核验：origin/develop 仍停 6bc65564——**201–239 共 39 提交待推送**；本批次后本地领先 45。两项默认值无新答复。

## 一、移植内容

1. **候选文档构建**（TS buildMemorySearchDocuments 逐字）：四类记忆（chapter-summary/hook/fact/volume-summary）→ SearchDocument，doc id `summary:{chapter}`/`hook:{hookId}`/`fact:{index}`/`volume-summary:{index}`；searchable hooks = 非 resolved（休眠种子仍可检索）。
2. **语义精简**：`MemorySemanticSelector` trait + `select_semantic_candidate_ids`（selector 缺失/候选 ≤1/失败 → None 回退 BM25 全量，白名单过滤编造 id）。
3. **rankScores 贯穿**：`build_rank_scores`（命中序位 ×10）→ 四个 select_relevant_* 全部重写为 rankScores + 确定性优先级模式（TS 394-527 逐字：summaries recent 3+recalled 1、hooks primary 6+stale 2、facts 优先谓词/阈值 14、volume 末位保底取 2）。
4. **composer**：`LlmMemorySelector`（TS selectMemoryCandidates prompt 逐字：温度 0.1/2048 tokens、理解否定/因果/别名/改述、严格 JSON）。
5. **装配**：write-next 与 compose 端点两处 `ComposeChapterInput.memory_semantic_selector`；旧 assemble_db_selection 的 DB 检索双路径合并为统一链（MemoryDb 保留空库回填副作用，对齐 TS）。

## 二、验证

- engine：lib **1325**（+6：rankScores 语义下 summaries/hooks 选择断言重写 + LlmMemorySelector 契约测试〔prompt 形态/白名单过滤编造 id〕+ local_search 4 测试）、集成 **196**、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**。
- TS 零改动；core 1918 / studio 793 / 双 typecheck 已绿。

## 三、效果

写作链 planner 记忆装配从「确定性词法评分」升级为 TS 同构的「BM25 检索 → LLM 语义精选（否定/因果/别名/改述理解）→ 确定性优先级」两级链；语义层失败自动回退（检索永不被阻断）。
