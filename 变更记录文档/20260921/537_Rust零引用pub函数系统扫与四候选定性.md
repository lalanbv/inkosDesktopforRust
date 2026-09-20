# 537 号：Rust 零引用 pub 函数系统扫与四候选定性

日期：2026-09-21
提交：见 git log（537 号）

## 选题来路

535 号备案候选「Rust pub API 死代码使用面系统扫」立项：500+ 号以来大量
TS→Rust 移植按「公开 API 一并移植」惯例，而 lib crate 顶层 `pub` 项会压制
rustc dead_code lint（对 lib 外可见即视为活），移植残留可能长期潜伏。
clippy:gate 的 `-D warnings` 只能守住 pub(crate) 及以下——`pub` 面是
lint 盲区，只能脚本化使用面扫描兜底。

## 扫描方法

python 脚本（/tmp，不落仓）：收集 engine-rs `{src,tests,benches,examples}`
（剔除 target/bindings/dist/fixtures）全部 .rs，正则抽 `pub fn <name>`
定义 854 个；对每个名字全仓 `\b<name>\b` 统计定义行外引用数，**0 引用**
才入选（宁漏勿误：注释/字符串提及都算引用，只会漏报不会误伤）。

结果：**854 个 pub fn 仅 4 个零引用候选**——移植纪律整体健康。

```
src/models/quality_governance.rs:62  continues_pipeline
src/server/segment_guard.rs:97       with_segment_guard
src/state/memory_db.rs:401           upsert_chunk_vector
src/state/memory_db.rs:441           chunk_vector_count
```

## 定性链（逐个双端对照）

### ① with_segment_guard —— 真死代码，删除

segment_guard.rs 的「便捷装配」函数，注释自称挂在路由层，但唯一装配点
server/mod.rs:895 手写 `.layer(from_fn(guard))`（需与 api_no_store 保持
洋葱序），测试 app() 也直接 layer——零消费。TS 侧无对应便捷导出。
**处置：删除**，原位留 3 行注释说明装挂点唯一的原因，防再引入单用途包装。

### ② continues_pipeline + creates_debt —— TS 消费面活，Rust 裁剪面，保留备案

- TS 对应 `verdictContinuesPipeline`（quality-governance.ts:186）/
  `verdictCreatesDebt`（:176）**都是活代码**：scheduler.ts:373/348 的
  八级 verdict 判定层消费（记债→判续→PAUSED）。
- Rust 端消费缺席根因：**daemon（72 号「Scheduler 精简生命周期」移植）
  把 TS 的 verdict 层裁剪成裸计数暂停**——ops_routes.rs:252-369 用
  `consecutive_failures >= pause_after_consecutive_failures` 两态判定，
  且**失败路径不记债**（quality_debts 表在 daemon 路径无写入，只有
  get_quality_debts 读取端点与 pacing 检测）。
- 类型面活：`GovernanceConfig` 被书配置引用（models/book.rs:159 serde 面）；
  `resolve_quality_verdict` 有测试锁语义（同文件 253-259 行）。
- **处置：保留**，两方法 doc 注释补「勿按零引用清理」备案。
- **差点误判**：首轮 grep 用 `continuesPipeline` 找 TS 对应零命中——
  TS 函数名是 `verdictContinuesPipeline`（大写 C 开头的驼峰段），
  大小写敏感子串匹配漏掉了它。若据此删 Rust 方法即破坏双端模型对齐。

### ③ upsert_chunk_vector + chunk_vector_count —— 双端同构「已建成未接线」，保留备案

- TS memory-db.ts:399/429 同名方法存在；TS 消费链 =
  hybrid-memory-selector.ts:89 的 `options.cache?.upsertChunkVector(...)`
  （检索时 fingerprint 增量嵌入写回）——但 **createHybridMemorySelector
  全仓零调用**（runner.ts:11 仅 import），TS 侧整条 hybrid 语义检索组件
  本身就是「已建成未接线」死路径（349 号 G1 时期组件）。
- Rust 端忠实移植了同样未接线的 API 面：retrieval_chunks 表
  只读不写（唯一读方 ops_routes.rs:1114 召回测试端点语义分支，恒空表
  降级 fts5——双端等价地空，差分器因此不报分歧）。
- **处置：保留**（与 list_chunk_vectors 构成完整 API 面、双端同构），
  补备案注释。

## 门禁（全绿）

- `cargo test --all-targets`：**1807 用例全绿**（落盘复读统计，与 536 轮基线一致）；
- `pnpm clippy:gate`：双 crate 0 告警；
- `pnpm verify:engine-bindings`：186 导出全绿；
- `INKOS_DUEL=1` duel 真跑：**10/10**（40.96s）；
- `node scripts/node-fallback-smoke.mjs`：双引擎一致性绿；
- `node scripts/engine-contract-diff.mjs`：**41 端点 0 分歧**；
- studio 面零 diff，TS 门禁按 535 轮先例裁剪（本轮无 TS 改动）。

## 立案专项（下轮候选）

1. **daemon verdict 化**：对齐 TS scheduler.ts:337-383 八级判定+记债面
   （resolveQualityVerdict/verdictCreatesDebt/verdictContinuesPipeline
   全链接线，quality_debts 账本 daemon 路径入账）——行为变更专项，
   须带活体差分验证；
2. **hybrid 语义检索接线**（双端）：349 号组件盘活或明示退役——若接线，
   retrieval_chunks 写面（upsert）随 activation；若退役，双端同步删
   （memory-db.ts + hybrid-memory-selector.ts + memory_db.rs 三处）；
3. TS Scheduler 消费面复核：daemon/start 路径 TS 与 Rust 的
   verdict 语义分歧已由专项 1 吸收，无需单独处理。

## 教训

1. **grep 找 TS 对应必须驼峰段变体全试**（continuesPipeline /
   verdictContinuesPipeline / ContinuesPipeline）——大小写敏感子串匹配
   在驼峰命名前缀变化时会假阴性，本次差点据此误删活对齐面；
2. **零引用 ≠ 死代码**：lib crate 的 pub 面消费可能在 ①外部消费者
   （bindings/HTTP/serde）②未接线的已建成组件（TS 侧同样零调用）
   ③缺席的消费面（TS 活、Rust 裁剪）——处置前必须双端对照+git 考古定性；
3. **双端同构是删除决策的最高优先约束**：TS 端死路径的忠实移植
   （upsert/chunk_vector_count）删除即破坏 API 面对齐，处置方向转为
   「双端同步接线或双端同步退役」的专项裁决。

## 编号警戒

开号前双查：目录最大 536、git log 最大 536（ff9c86e7 之后），537 空闲；
写档前复查发现并行会话已落 538 号（15ab0dce 注释撞号清偿，无归档文档），
537 仍未被占用——本号安全，收口提交前再查一次。
