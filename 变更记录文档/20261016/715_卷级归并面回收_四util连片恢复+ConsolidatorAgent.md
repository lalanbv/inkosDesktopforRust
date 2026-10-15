# 715 号：卷级归并面回收——四 util 连片恢复 + ConsolidatorAgent（回收清单 15→14）

> 目标线：自主持续开发（TS/Rust/桌面壳）。714 号据实切批的 consolidate 面
> 本批补齐：四 util ~1175 行 fork 逐字移植 + 双端 golden 对照 + 归并真人链路。

## 一、连片移植清单（fork 逐字，import 源适配）

| 模块 | 行数 | 依赖适配 |
| --- | --- | --- |
| utils/hook-policy.ts | 115 | 零依赖纯常量（HOOK_TIMING_PROFILES/PHASE/PRESSURE/ACTIVITY 等 11 导出） |
| utils/hook-lifecycle.ts | 255 | StoredHook→promise-ledger（HookPayoffTiming/HookStatus→runtime-state ✓） |
| utils/story-markdown.ts | 376 | StoredHook→promise-ledger / StoredSummary→memory-db-readonly / Fact→CurrentStateFact（runtime-state 同构，id 字段不消费） |
| utils/hook-promotion.ts | 314 | StoredHook→promise-ledger（纯函数模块） |
| agents/consolidator.ts | 226 | readVolumeMap 当前单参无容错→`.catch(() => "")` 对齐 fork 双参容错语义（无卷图=空边界零归档分支） |

连带类型面最小收敛（Rust 真源镜像）：
- `runtime-state.ts` 补 `HookPayoffTimingSchema`/`HookPayoffTiming`（5 值枚举，
  fork 19-26 行逐字——hook-lifecycle/hook-policy 消费面）。
- `memory-db-readonly.ts` StoredSummary 补 `conflictLevel?/revealLevel?`（R2/358
  张力列；Rust memory_db.rs StoredSummary 同字段 serde default None——render
  渲染缺分空单元格）。

## 二、双端 golden 对照（方向反转：Rust 已验证向量反向断言 TS）

- **leaf.json 直接复用**：`engine-rs/tests/golden/utils/leaf.json` 的
  `render_hook_snapshot`（2 案例）+`render_summary_snapshot`（3 案例）——
  golden_leaf.rs `render_*_matches_ts` 的断言源。TS 恢复移植后以同向量反向
  断言逐字节一致（`golden-story-markdown.test.ts` 2 测）。
- render_hook_snapshot 的回收节奏列经 hook-lifecycle `resolveHookPayoffTiming`
  推导——**同一向量同时活体覆盖 hook-lifecycle 链**（向量 hooks 无
  payoffTiming 字段=推导路径必经）。
- parse 族（parsePendingHooksMarkdown/rerunPromotionPass）无现成向量——由
  consolidate 回归锁的 promote 翻转写回断言（parse→derive→render 往返）覆盖。

## 三、嫁接与探针

- **server POST /books/:id/consolidate**（fork 逐字：ConsolidatorAgent 构造
  {client, model, projectRoot} + state.bookDir + broadcast
  consolidate:complete/error）；isSafeBookId 守卫。
- core index 导出：ConsolidatorAgent/ConsolidationResult + story-markdown
  渲染解析族 + rerunPromotionPass。**parseChapterSummariesMarkdown 不重复
  导出**（tension-curve.js 已有同构版本在导出面——2.0 独立演进，去重备案）。
- 域表：FORK_ONLY 移 consolidate → **14**；SHARED_FACE 加 consolidate POST
  （fixture 无 volume_map → 双端幂等零归档分支 200/200，不打 LLM）。
- 度量：**共享面 56 对比 0 分歧 + fork-only 14/14 + 上游新增 9/9**。

## 四、回归锁（consolidate-recycle-routes.test.ts，2 测）

1. 无卷边界幂等零归档（不打 LLM 的确定面）+ 坏 id 400。
2. **promote 重跑独立生效**：pending_hooks.md 台账（H01 advancedCount 达阈
   未升级/H02 未推进）+ 摘要表 hookActivity 列两行推进 → consolidate →
   `promotedHookCount:1` + 台账写回「是/否」翻转断言（parsePendingHooks→
   deriveAdvancedCounts→renderHookSnapshot 全往返）。

## 五、门禁与活体走查（node 腿，714 同标准）

- 矩阵：studio 680/680（+2）· core 932/932（+2 golden）· gate:ts 八步全绿 ·
  contract-diff 56/0（gate 内复跑确认）。
- **归并真人链路**：播卷图（第一卷 1-1）+ 第 1 章摘要行 → API consolidate →
  **volume_summaries.md 生成**（mock narrative summarizer 压缩段写入 +
  「## 第一卷 坊市风云 (Ch.1-1)」头）+ **summaries_archive/vol_1-1.md 归档**
  + chapter_summaries.md 重写（retained 表头）→ **UI「归并记忆」点击**
  （alert 弹出=成功路径；mtime 实证写盘）→ 幂等复跑零归档。
- 走查小坑：卷图卷头「第一卷 坊市风云（第1-1章）」的区间正则不认空格
  （「第 1-1 章」不匹配 rangePattern）——**fork 解析器的格式契约要按正则
  字面喂**（fixture 数据也是契约面）。

## 六、教训

1. **golden 方向反转复用**：恢复移植场景 Rust 已有实现+已验证向量——直接
   消费对方 golden 文件反向断言，比手造向量便宜且权威（双端同源可证）。
2. **类型源适配三选一**：StoredHook(promise-ledger)/StoredSummary(
   memory-db-readonly)/Fact(CurrentStateFact)——移植模块的 import 面是
   fork→2.0 的类型拓扑图，逐个核字段而非整目录假设。
3. **连带类型收敛点要顺路清**（StoredSummary 张力列/HookPayoffTiming）——
   移植暴露的消费面缺口按 Rust 真源补可选键，不扩散改 schema。
4. 导出面去重先 grep（parseChapterSummariesMarkdown 双版本同构——重复导出
   是合并冲突隐患）。
5. 同型容错语义迁移要找到 fork 原形态（readVolumeMap 双参第二参=""=容错
   回退），适配处注明等价关系。

## 涉及文件

- packages/core/src/utils/{hook-policy,hook-lifecycle,hook-promotion,story-markdown}.ts（新增 4）
- packages/core/src/agents/consolidator.ts（新增）
- packages/core/src/models/runtime-state.ts（HookPayoffTiming 补齐）
- packages/core/src/state/memory-db-readonly.ts（StoredSummary 张力列）
- packages/core/src/index.ts（归并族导出）
- packages/studio/src/api/server.ts（consolidate 路由）
- packages/core/src/__tests__/golden-story-markdown.test.ts（新增 2 测）
- packages/studio/src/__tests__/consolidate-recycle-routes.test.ts（新增 2 测）
- scripts/walkthrough-mock.mjs（narrative summarizer 分派支）
- scripts/engine-contract-diff.mjs（SHARED_FACE +1 / FORK_ONLY −1）
