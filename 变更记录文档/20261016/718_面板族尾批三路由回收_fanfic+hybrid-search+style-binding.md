# 718 号：面板族尾批三路由回收——fanfic/refresh + hybrid-search + style-binding（回收清单 7→4）

> 目标线：自主持续开发（TS/Rust/桌面壳）。**fork-only 有界面（UI 活消费面）
> 本批全部清偿**——余 4 条均为无 UI 消费方/声明域（debug/tools+health=Rust
> 超集；interaction/session+prompt-packs=低优先备案）。

## 一、嫁接内容（TS 侧，Rust 侧零改动）

### 1. 三路由（fork 逐字 + 路径正典化 + isSafeBookId）
- **style-binding GET/PUT**：写法绑定面板（选档案→绑定→当前绑定投影）。
  PUT 三护栏（profileName required 400 / profile 不存在 404 / 合法落盘
  style_binding.json）；profile 存在性校验 .inkos/style-profiles/。
- **hybrid-search POST**：RecallTestCard 召回测试。空 query 400 / 无
  memory.db 缺省零结果分支 / FTS 面走**当前演进版 LocalSearchIndex**（2.0
  initializeSchema+token 限 64+parse 容错——diff 定性为演进不回滚）+
  reciprocalRankFusion 融合。
- **fanfic/refresh POST**：同人正典刷新（importFanficCanon 在树 ✓ +
  book.fanficMode ✓ + SSE 广播 fanfic:refresh:start/complete/error）。

### 2. hybrid-search 语义面降级适配（有注释的 2.0 形态）
2.0 无 MemoryDB 写链（chunk 向量写入随 fork 链退役）——listChunkVectors 无
数据源 → 语义恒空、mode 恒 fts5-fallback。FTS 面完整可用；语义缓存接线后
分支自动升级（fork 路由逐字形状保持）。

### 3. core index 导出补齐（第五批）
LocalSearchIndex/tokenizeSearchText/SearchDocument/SearchHit +
createEmbeddingClient/EmbeddingConfig（EmbeddingClient 类型在
semantic-retrieval——两文件分工别导错）+ topKBySimilarity/reciprocalRankFusion。

## 二、探针与域表（7→4，+4 探针）

- style-binding GET 基线（binding:null 双端全 body）+ **PUT 探针载荷=
  profileName 在而 profile 缺失 → 双端 404 同构**（空载荷探针会打 TS untyped
  400 vs Rust typed-extractor 422 的错误协议差伪影——705 号同类，备案不走）。
- hybrid-search/fanfic refresh 空载荷 → 双端 400 同护栏（query/sourceText
  required）。
- 度量：**共享面 68 对比 0 分歧 + fork-only 4/4 + 上游新增 9/9**。

## 三、回归锁（panel-family-recycle-routes.test.ts，3 测）

1. hybrid-search：空 query 400 / 无库缺省分支全形 / **memory.db 在位 FTS
   命中链**（LocalSearchIndex.replaceScope 播种 → 检索「祖符」命中
   chapter_summaries-1 首位）/ 坏 id 400。
2. style-binding：缺省 null + 400/404 双护栏 + 播 profile 合法绑定落盘
   往返（enabledIds 缺省不落键）。
3. fanfic/refresh：空载荷 400 + 坏 id 400（真载荷 LLM 链留走查）。

## 四、门禁与活体走查（node 腿，IAB）

- 矩阵：studio 688/688（+3）· core 932/932 零漂移 · gate:ts 八步全绿 ·
  contract-diff 68/0（gate 内复跑确认）。
- **真人链路**：API PUT style-binding（wuxia-prose profile 播种）→ **UI
  写法绑定面板「当前绑定: wuxia-prose」投影** → 召回测试框输入「祖符」→
  检索 → **「模式: 词法降级 | 无命中」**（fts5-fallback 空态 UI 投影；命中
  面由回归锁 LocalSearchIndex 播种链覆盖）。

## 五、教训

1. **bash 管道 head 伪死锁**：`node ... | grep | head -N` 输出超缓冲后 head
   提前关闭管道，主进程写阻塞假死——**长跑脚本重定向文件再 tail**（本轮
   contract-diff 卡 9 分钟的真相，文件重定向后 300s 内正常完成）。
2. 探针载荷避开双端错误协议差（TS untyped 400 vs Rust typed-extractor 422
   ——705 号备案第三次生效），选双端同构的校验层（profile 缺失 404）。
3. 「被裁」与「接线未完成」要区分：local-search.ts 是演进版（保持）、
   MemoryDB 语义缓存是 2.0 未接线（降级适配+自动升级注释）——两种形态
   两种处置。
4. 语义缓存无写链 → 降级分支是诚实契约（fts5-fallback 恒定），UI 空态
   「词法降级」投影与路由 mode 字段同源——降级不是缺陷是状态。

## 涉及文件

- packages/core/src/index.ts（检索面导出第五批）
- packages/studio/src/api/server.ts（三路由）
- packages/studio/src/__tests__/panel-family-recycle-routes.test.ts（新增 3 测）
- scripts/engine-contract-diff.mjs（SHARED_FACE +4 / FORK_ONLY −3）
