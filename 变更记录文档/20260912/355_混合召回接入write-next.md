# 355 号：write-next 检索链混合召回切换（350a，G1 收尾——G1 大件完整收官）

- 日期：2026-09-12
- 类型：TS 检索链接线（embedding 为可选增强，默认行为逐字节不变）
- 规划依据：[328 规划](../20260911/328_对标调研分析与改进规划.md) G1 收尾项；前置 [347](347_语义检索层契约.md)/[348](348_embedding双协议与增量向量化.md)/[349](349_混合检索端点与召回面板.md) 号

## 做了什么

1. **HybridMemorySelector**（`retrieval/hybrid-memory-selector.ts`）：
   write-next 记忆精选的混合召回实现——请求 `query`（检索链已按章任务单组装）
   与候选 excerpt 分别嵌入 → 候选指纹对比 `retrieval_chunks` 缓存**仅增量嵌入** →
   余弦 topN（默认 6，对齐 LLM 精选量级）作为精选 id 列表。
   **降级契约闭环**：embedding 任何失败（网络/超时/维度）→ 返回空数组 →
   检索链自动回退 BM25 排序，行为与无语义层完全一致。
2. **管线接线**：`PipelineConfig.embeddingClient`（optional）；
   compose 的 `memorySemanticSelector` 默认值切换——embeddingClient 存在 →
   混合召回精选；否则维持默认 LLM 精选。**显式传入的 selector 仍最优先**。
3. **core index 导出**：createHybridMemorySelector 及类型。
4. **测试** `hybrid-memory-selector.test.ts` 3 项：相似度重排 + 指纹缓存
   （首轮嵌 query+3 候选、二轮仅嵌 query——增量生效）、embedding 失败回退
   空数组、空候选短路。

## 验收

- core：tsc 干净；vitest 218 文件 / 2009 用例全绿（新增 3 项）。
- studio：tsc 干净；vitest 90 文件 / 793 用例全绿。
- engine-rs：`INKOS_DUEL=1 cargo test` 22 个测试目标全 ok。
- bindings 158 绿；bench:gate 静默窗通过（上轮 353 拒跑已补验清算）。

## 影响面

- 默认（无 embedding 配置）行为逐字节不变：selector 链与 LLM 精选完全一致；
  配置 `llm.embedding` 并传入 embeddingClient 后自动启用混合召回。
- 未触碰并行会话在途文件（package.json、rustbin.rs、audit-rust.mjs）。

## 里程碑

- **G1 语义检索大件完整收官**（347 契约层 / 348 embedding+增量 / 349 落库+端点+
  面板+Doctor / 350a 检索链接线）。
- **Phase C 剩余：G5 ✓、G6 ✓（352/353/354），G11/G12/G13/G15 择机项清算结论
  见 354 号。328 规划全部主件落地**；后续按用户反馈迭代（推送核验、真实写作
  反馈、G11/G13 启动评估、G15 UI 窗口）。
