# 959 号 · 召回测试面板双端修复轮：RRF 实参倒置（349 号起双端同患）+ Node 端响应形态回归致整页崩溃

> 958 号「写面响应形态是差分器盲区」教训的按族扫：UI 读取响应体的 29 个写面
> 调用（POST/PUT，差分器只比状态码）逐一对照 TS 2.0 handler 与 Rust handler。
> 族扫定型三处分叉：①hybrid-search（本轮清偿）②translation run 缺 skillIds
> （960 候选）③audit 端点上游自身 UI/服务端不一致（961 候选）；余 26 处
> MATCH/MINOR（UI 不读差异字段）。
> 验证：Rust ops_routes 新用例必红/绿+TS panel-family-recycle 3/3（hybrid
> 用例改为新契约断言，stash 恰 1 红）+RecallTestCard 交互 2/2+testgate
> engine-rs 2088（+1）/src-tauri 584+audit:rust 双 0（自检活体）+clippy 双 0+
> gate:ts 十步全绿+U+FFFD 净。

## 一、缺陷定型

- **P1 RRF 实参倒置（双端同患，默认 Rust 后端用户可见）**：core 融合函数签名
  `reciprocal_rank_fusion(semantic_ranking, fts_ranking, k)` /
  `reciprocalRankFusion(semanticRanking, ftsRanking, k = RRF_K)`（golden 向量锁
  正确），但两端端点调用面自 349 号（faa7756b）起均写成 `(fts, semantic, …)`
  ——纯词法命中被标 `sources: ["semantic"]`。召回测试面板在「模式: 词法降级」
  下把词法命中显示为「·语义」，自相矛盾。分数对称（1/(k+rank) 累加）不受影响，
  仅来源标注错位——golden 向量只锁函数不锁调用面实参序。
- **P2 Node 端响应形态回归（Node 回退端整页崩溃）**：718 号（b422c644）「fork
  逐字回收」时实际把命中分支改成 `{results:[{id,score,title,excerpt,source}],
  mode}`（349 号原形为 `{fts,semantic,fused,mode}`，无库分支与 Rust 一直是后者）。
  RecallTestCard 读 `result.fused.length`——而 TS 2.0 写作链 memory-retrieval
  （`LocalSearchIndex(story/memory.db)`）每写一章即建出 memory.db，故 Node 端
  写过章的书一检索即 `Cannot read properties of undefined (reading 'length')`，
  React 错误边界把**整个书籍设置页**替换成「界面遇到了问题」。718 的 TS 用例
  断言的正是 `{results}` 形态（回声室锁定回归）。
- **P3 Node 端 RRF 常数误用**：718 号把请求 top-k（UI 发 10）当 RRF 常数传入
  （1/(10+rank)，Rust 恒 60）——双端同查询分数差 5.5 倍（活体 0.0909 vs 0.0164）。

## 二、修复

- Rust `ops_routes.rs::post_hybrid_search`：`reciprocal_rank_fusion(&semantic,
  &fts, 60)`。
- TS `server.ts` hybrid-search 命中分支：`reciprocalRankFusion(semanticRanked,
  ftsRanked)`（缺省 RRF_K=60，top-k 只作检索 limit）+响应回到 349 号契约
  `{fts:[{id,score,source,title}], semantic, fused, mode}`——与无库分支、Rust
  逐键同形。

## 三、测试与必红

- Rust ops_routes +1 `hybrid_search_labels_lexical_hits_as_fts5`：Rust 布局
  books/b1/story/memory.db 以 LocalSearchIndex 播种 story-memory 语料 → 响应四键
  `{fts,fused,mode,semantic}`+fts 元素源/标题+fused[0]=1/61+全部 sources=
  `["fts5"]`。必红：临时还原实参序（NEUTER959 标记）→ 恰该例红（`sources:
  ["semantic"]` 实证），复原后绿，残留 0。
- TS panel-family-recycle hybrid 用例改为新契约断言（四键/fts 四键/fused[0]=
  1/61 证 RRF 常数 60/全部 fts5）；必红：stash server.ts → 恰该例红（旧形态
  `{results,mode}`），pop 后 3/3。双端断言同形（同语料同查询同期望）=双锁。
- RecallTestCard 交互 2/2（UI 契约 mock 与端点契约现已一致）。

## 四、真引擎活体 + GUI 真人走查

- Rust 端（8959，二进制先核验新鲜）：curl 祖符 → fts 2 命中、fused
  `[1/61, 1/62]` 均 `["fts5"]`；IAB 书籍设置页召回测试「模式: 词法降级」+
  两行「·词法」。
- Node 回退端（8960，works 布局 memory.db 由 core LocalSearchIndex 播种）：
  修复前 curl 得 `{results…score 0.0909}`，GUI 检索后**整页崩溃**（错误边界
  「界面遇到了问题 Cannot read properties of undefined (reading 'length')」）；
  修复后 curl 与 Rust 逐值相同（fts 分数、fused 分数、sources 全等），GUI 两行
  「·词法」正常渲染。
- 环境清偿：双端服务杀净、临时根删除、IAB 标签关闭、stash 栈 0。

## 五、族扫其余定型（备案出口）

- **960 候选 translation run**：TS 2.0 `{…result, skillIds}`，UI 读
  `skillIds` 拼「Skill：…」；Rust 返回 `{projectId,translatedSegments,
  reviewedChapters,reportPath}` 且翻译/审校链**未激活 inkos-translation 技能**
  （TS 2.0 经 runWithAgentContext 注入）——默认后端翻译缺专业技能指导，质量面
  漂移（非仅展示）。
- **961 候选 audit**：TS 2.0（fb8c8e3e）改返 `{summary, observations}`，上游
  自己的 BookDetail 审计按钮仍读 `passed/issues`（上游自身不一致）；Rust 返旧形
  与 UI 一致——Node 回退端每次审计都显示「Audit failed: 0 issues」。收敛需 UI
  与双端同改（Rust 单改即破默认后端），需定方案。
- plan/compose：UI 读 `title` 双端均不返（UI 侧空标题，非漂移）；generate-image/
  project artifacts/import chapters/translations create·export/agent：差异字段
  UI 不读（MINOR）。

## 六、门禁

- clippy 双 0；testgate engine-rs 2088（+1）/src-tauri 584（INKOS_DUEL=1）+
  audit:rust 双 0+必红自检活体；gate:ts 十步全绿（engine-contract-diff 76/0）；
  U+FFFD 净。

## 七、结论

召回测试面板在默认 Rust 后端恢复来源标注正确（词法降级下不再标「语义」），在
Node 回退端从「写过章即整页崩溃」恢复正常并与 Rust 逐值同形；golden 只锁函数
不锁调用面实参序的盲区以双端端点级同形断言补锁。

**下一号自 960 起；出口=960 翻译技能激活与 skillIds 对齐 / 961 审计响应形态
上游不一致定案 / 写法绑定 B 面；用户侧待办=Fork 推送（含 955-959）。**
